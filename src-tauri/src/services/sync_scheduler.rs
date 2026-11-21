//! Background library sync scheduler.
//!
//! Periodically checks if a library sync is due based on user settings
//! and triggers automatic synchronisation with Jellyfin.

use crate::artwork::{artwork_version, cache_artwork};
use crate::commands::SyncMetrics;
use crate::db::{
    current_timestamp, settings::keys, CachedAlbum, CachedArtist, CachedTrack, Database,
};
use crate::error::AppError;
use crate::jellyfin::JellyfinClient;
use crate::mediaserver::subsonic::SubsonicClient;
use crate::mediaserver::{MediaClient, ServerKind};
use crate::state::AppState;
use futures_util::stream::{self, StreamExt};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};
use tracing::Instrument;

/// Check interval for the scheduler (1 minute).
///
/// The scheduler checks periodically whether a sync is due, rather than
/// sleeping for the full interval, so it can respond to setting changes.
const CHECK_INTERVAL_SECS: u64 = 60;

/// How long a synced scrobble-queue row is kept before the scheduler deletes
/// it. Nothing reads synced rows (the pending count filters them out), so this
/// is only a debugging window.
const SYNCED_QUEUE_RETENTION_SECS: i64 = 7 * 24 * 60 * 60;

/// Failed probe rounds, [`UNREACHABLE_RETRY_DELAY`] apart, before the app is
/// declared unreachable (and the "can't reach your server" toast shown).
const UNREACHABLE_CONFIRMATIONS: u32 = 3;
const UNREACHABLE_RETRY_DELAY: Duration = Duration::from_secs(5);
/// Budget for one probe of one backend. The clients' own 30 s request timeout
/// would make a black-holed server cost minutes before the outage is reported.
/// How often a running probe checks whether Offline Mode was switched on.
const STOP_POLL: Duration = Duration::from_millis(250);
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

/// Batch size for processing albums during sync.
const ALBUM_BATCH_SIZE: usize = 100;

/// Maximum number of album-track fetches to run concurrently during sync.
///
/// Kept deliberately low: a sync fans out ~one request per album (~2000
/// for a 24k-track library), and a self-hosted Jellyfin under that load starves
/// playback stream-opens (a play during sync measured ~13s vs sub-second idle).
/// Lower steady concurrency trades a longer wall-clock sync for a server that
/// stays responsive to playback.
const CONCURRENT_FETCH_LIMIT: usize = 4;

/// Maximum number of artwork downloads in flight in the detached warm task.
///
/// Kept small so warming (which now overlaps PHASE B track fetch) does not push
/// a self-hosted Jellyfin over its connection limit: peak concurrency is
/// `ARTWORK_CONCURRENCY + CONCURRENT_FETCH_LIMIT` (= 7), well below the old
/// unbounded burst that provoked 503 storms.
const ARTWORK_CONCURRENCY: usize = 3;

/// Per-item timeout for a single artwork download in the detached warm task, so
/// one stalled image can never hold a warm slot open indefinitely.
const ARTWORK_ITEM_TIMEOUT: Duration = Duration::from_secs(15);

/// Maximum number of playlist-track prefetches in flight in the detached warm
/// task. Matches `ARTWORK_CONCURRENCY` for the same reason: the prefetch
/// fans out one `get_playlist_tracks` request per playlist, and this task is
/// spawned at the end of a sync so it does not overlap PHASE B — keeping peak
/// in-flight requests well under the ceiling that provoked 503 storms.
const PLAYLIST_PREFETCH_CONCURRENCY: usize = 3;

/// Per-item timeout for a single playlist-track prefetch. Generous relative to
/// the trimmed request so a large playlist on a slow server still
/// completes, while one stalled fetch can never pin a warm slot open.
const PLAYLIST_PREFETCH_ITEM_TIMEOUT: Duration = Duration::from_secs(20);

/// Event emitted to the frontend once the playlist-track cache has been warmed,
/// so the sidebar re-reads the now-populated cache and swaps the generic
/// icon for the collage without the user opening each playlist first.
const PLAYLISTS_WARMED_EVENT: &str = "playlists-cache-warmed";

/// Event emitted when a library sync starts, on the ONE path both the manual
/// `sync_library` command and this scheduler funnel through
/// ([`perform_sync_for_client`]) — so the frontend has a single, trigger-agnostic
/// "a sync is running" signal instead of three independent local booleans (the
/// toolbar button, the Settings sync button, and a scheduled sync the frontend
/// previously had no visibility into at all). Carries the backend being synced.
const SYNC_STARTED_EVENT: &str = "library-sync-started";
/// Event emitted when a library sync finishes successfully (including a
/// coalesced no-op — `metrics.skipped`). Carries the backend and [`SyncMetrics`].
const SYNC_COMPLETED_EVENT: &str = "library-sync-completed";
/// Event emitted when a library sync fails. Carries the backend and the error.
const SYNC_FAILED_EVENT: &str = "library-sync-failed";

/// Event emitted when automatic network reachability changes:
/// distinct from, and never overriding, the user's manual `offline_mode`
/// toggle — this is the "lost the connection" signal Spotify shows
/// automatically ("downloads play automatically when you lose internet"),
/// alongside its separate manual Offline Mode. Carries `{ reachable: bool }`.
const CONNECTIVITY_CHANGED_EVENT: &str = "connectivity-changed";

/// Payload for [`SYNC_STARTED_EVENT`] / part of the other sync lifecycle events.
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SyncBackendEvent<'a> {
    backend: &'a str,
}

/// Payload for [`SYNC_COMPLETED_EVENT`].
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SyncCompletedEvent<'a> {
    backend: &'a str,
    metrics: SyncMetrics,
}

/// Payload for [`SYNC_FAILED_EVENT`].
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SyncFailedEvent<'a> {
    backend: &'a str,
    error: String,
}

/// Payload for [`CONNECTIVITY_CHANGED_EVENT`].
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ConnectivityChangedEvent {
    reachable: bool,
}

/// Emit a frontend event if the `AppHandle` is installed. Best-effort and
/// silent otherwise, matching every other emit in this module.
fn emit_event<T: serde::Serialize + Clone>(state: &AppState, event: &str, payload: T) {
    if let Some(app) = state.app_handle() {
        if let Err(e) = app.emit(event, payload) {
            tracing::debug!("Failed to emit {} event: {}", event, e);
        }
    }
}

/// Start the background sync scheduler.
///
/// Spawns an async task that periodically checks if a library sync is due
/// based on the user's configured interval. The task runs until the app exits.
pub fn start_sync_scheduler(app_handle: tauri::AppHandle) {
    // One-shot: warm the playlist-track cache at startup so sidebar
    // collages render on EVERY launch, independent of whether a full library
    // sync is due — a full sync can be hours away, or disabled (interval 0), so
    // hanging the prefetch only off end-of-sync left collages unpopulated on a
    // fresh launch. Detached + bounded like the end-of-sync prefetch.
    let startup_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        prefetch_playlists_at_startup(&startup_handle).await;
    });

    tauri::async_runtime::spawn(async move {
        tracing::info!("Sync scheduler started");
        run_scheduler_loop(app_handle).await;
    });
}

/// Warm the playlist-track cache once at startup, independent of
/// library-sync due-ness, so the sidebar collage renders on every launch.
///
/// Skipped while offline; runs for each credentialed backend via the same
/// ephemeral-client path as the scheduler. Best-effort and non-fatal — a failure
/// just leaves the generic icon, exactly as before the prefetch existed. Emits
/// `playlists-cache-warmed` so the sidebar re-reads the now-warm cache.
async fn prefetch_playlists_at_startup(app_handle: &tauri::AppHandle) {
    let state = match app_handle.try_state::<AppState>() {
        Some(s) => s,
        None => return,
    };

    let offline = state
        .db
        .get_setting(keys::OFFLINE_MODE)
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);
    if offline {
        tracing::debug!("Startup playlist prefetch skipped: offline mode is on");
        return;
    }

    let mut warmed_any = false;
    for kind in [ServerKind::Jellyfin, ServerKind::OpenSubsonic] {
        let client = match client_for_background_sync(&state, kind).await {
            Ok(Some(client)) => client,
            _ => continue,
        };
        if warm_playlist_cache(client, state.db.clone(), kind).await > 0 {
            warmed_any = true;
        }
    }

    if warmed_any {
        if let Err(e) = app_handle.emit(PLAYLISTS_WARMED_EVENT, ()) {
            tracing::debug!("Failed to emit {} event: {}", PLAYLISTS_WARMED_EVENT, e);
        }
    }
}

/// Main scheduler loop.
async fn run_scheduler_loop(app_handle: tauri::AppHandle) {
    // One probe at startup: don't wait a full CHECK_INTERVAL_SECS for
    // the first automatic online/offline signal. Honours the same "offline mode
    // promises no background network activity" contract as the loop below.
    if let Some(state) = app_handle.try_state::<AppState>() {
        probe_connectivity(&state).await;
    }

    loop {
        tokio::time::sleep(Duration::from_secs(CHECK_INTERVAL_SECS)).await;

        let state = match app_handle.try_state::<AppState>() {
            Some(s) => s,
            None => continue,
        };

        // Local housekeeping, no network: synced queue rows would otherwise grow
        // without limit.
        if let Err(e) = state
            .db
            .cleanup_synced(current_timestamp() - SYNCED_QUEUE_RETENTION_SECS)
        {
            tracing::warn!("Failed to clean up synced queue rows: {}", e);
        }

        // Offline mode promises no background network activity — checked once
        // up front so no backend is touched while offline. The automatic
        // connectivity probe honours the same promise: it never runs while the
        // user has chosen offline mode.
        if offline_mode_on(&state) {
            tracing::debug!("Scheduled sync skipped: offline mode is on");
            continue;
        }

        probe_connectivity(&state).await;

        // Evaluate due-ness per backend and sync each credentialed
        // backend on its own interval — including a backend that is not the
        // active `state.media` client, via an ephemeral read-only client. The
        // syncs run sequentially per tick; each stamps its own timestamp key so
        // their intervals advance independently.
        for kind in [ServerKind::Jellyfin, ServerKind::OpenSubsonic] {
            if !is_sync_due_for(&state.db, kind).await {
                continue;
            }

            let client = match client_for_background_sync(&state, kind).await {
                Ok(Some(client)) => client,
                Ok(None) => {
                    tracing::debug!(
                        "Scheduled sync skipped for {}: no connected/credentialed client",
                        kind.as_str()
                    );
                    continue;
                }
                Err(e) => {
                    tracing::warn!(
                        "Scheduled sync skipped for {}: could not build client: {}",
                        kind.as_str(),
                        e
                    );
                    continue;
                }
            };

            tracing::info!("Starting scheduled library sync for {}...", kind.as_str());
            match perform_sync_for_client(&state, client).await {
                Ok(metrics) => {
                    tracing::info!(
                        "Scheduled sync ({}) complete in {}ms (fetch: {}ms, db: {}ms): {} artists, {} albums, {} tracks, {} API requests",
                        kind.as_str(),
                        metrics.duration_ms,
                        metrics.fetch_time_ms,
                        metrics.db_time_ms,
                        metrics.artist_count,
                        metrics.album_count,
                        metrics.track_count,
                        metrics.api_requests,
                    );
                    if metrics.stale_artists > 0
                        || metrics.stale_albums > 0
                        || metrics.stale_tracks > 0
                    {
                        tracing::info!(
                            "Cleaned up stale entries ({}): {} artists, {} albums, {} tracks",
                            kind.as_str(),
                            metrics.stale_artists,
                            metrics.stale_albums,
                            metrics.stale_tracks,
                        );
                    }
                    if metrics.failed_fetches > 0 {
                        tracing::warn!(
                            "{} album track fetches failed ({})",
                            metrics.failed_fetches,
                            kind.as_str()
                        );
                    }
                }
                Err(e) => {
                    tracing::error!("Scheduled library sync ({}) failed: {}", kind.as_str(), e);
                }
            }
        }
    }
}

/// Obtain a client to sync `kind` in the background.
///
/// If the live `state.media` client already speaks to `kind`, clone it (reuse
/// the callback-installed, active client). Otherwise, if that backend has
/// stored credentials, build an EPHEMERAL client from the keyring: no proxy, no
/// auth-expired callback (— a 401 on a backend the user isn't looking at
/// must not fire a global reconnect prompt), and crucially without touching
/// `state.media`, the proxy, or `active_source`, so a background refresh of the
/// inactive backend never disturbs the active playback session. `Ok(None)` when
/// the backend has no credentials.
async fn client_for_background_sync(
    state: &AppState,
    kind: ServerKind,
) -> Result<Option<MediaClient>, AppError> {
    // Reuse the live client when it already speaks to this backend.
    {
        let guard = state.media.read().await;
        if let Some(client) = guard.as_ref() {
            if client.kind() == kind {
                return Ok(Some(client.clone()));
            }
        }
    }

    // Build a read-only ephemeral client from stored credentials.
    match kind {
        ServerKind::Jellyfin => match crate::jellyfin::load_credentials()? {
            Some(credentials) => {
                let client = JellyfinClient::from_credentials(&credentials)?;
                Ok(Some(MediaClient::Jellyfin(client)))
            }
            None => Ok(None),
        },
        ServerKind::OpenSubsonic => match crate::mediaserver::subsonic::load_credentials()? {
            Some(credentials) => {
                let client = SubsonicClient::from_credentials(&credentials)?;
                Ok(Some(MediaClient::Subsonic(client)))
            }
            None => Ok(None),
        },
    }
}

/// Probe whether any credentialed backend is currently reachable, and update
/// `state.network_reachable` (emitting [`CONNECTIVITY_CHANGED_EVENT`] on
/// change) — the automatic online/offline signal, alongside
/// (never instead of) the user's manual offline-mode toggle.
///
/// Reuses [`MediaClient::test_connection`], the same lightweight probe already
/// used at connect time. A success on ANY credentialed backend marks the app
/// reachable; a [`AppError::Network`] failure on every credentialed backend
/// marks it unreachable — but only after [`UNREACHABLE_CONFIRMATIONS`] failed
/// rounds, [`UNREACHABLE_RETRY_DELAY`] apart, because a probe fired at launch
/// or on wake runs before the network or DNS is back and fails for a server
/// that is perfectly up. A non-network failure (bad credentials, a
/// server-side error) says nothing about reachability — the server answered —
/// so it counts as reachable. With no credentialed backend at all this is a
/// no-op: there is nothing to be "offline" from.
async fn probe_connectivity(state: &AppState) {
    if offline_mode_on(state) {
        return;
    }
    let mut clients = Vec::new();
    for kind in [ServerKind::Jellyfin, ServerKind::OpenSubsonic] {
        if let Ok(Some(client)) = client_for_background_sync(state, kind).await {
            clients.push(client);
        }
    }
    if clients.is_empty() {
        return;
    }

    let reachable = confirm_reachable(
        UNREACHABLE_CONFIRMATIONS,
        UNREACHABLE_RETRY_DELAY,
        || offline_mode_on(state),
        || any_reachable(clients.iter().map(|c| c.test_connection()), PROBE_TIMEOUT),
    )
    .await;
    apply_probe_result(state, reachable);
}

/// Whether the user has switched Offline Mode on.
fn offline_mode_on(state: &AppState) -> bool {
    state
        .db
        .get_setting(keys::OFFLINE_MODE)
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false)
}

/// Record a finished probe, unless the user chose Offline Mode while it ran:
/// the probe promised no background network activity and its late verdict
/// must not toast or flip the signal under that choice.
fn apply_probe_result(state: &AppState, reachable: bool) {
    if offline_mode_on(state) {
        return;
    }
    set_network_reachable(state, reachable);
}

/// Probe every backend at once, each within `timeout`; reachable as soon as
/// one answers. A timeout or a network error counts as unreachable, any other
/// error as reachable (the server answered).
async fn any_reachable<F>(probes: impl Iterator<Item = F>, timeout: Duration) -> bool
where
    F: std::future::Future<Output = Result<(), AppError>>,
{
    probes
        .map(|probe| async move {
            !matches!(
                tokio::time::timeout(timeout, probe).await,
                Err(_) | Ok(Err(AppError::Network(_)))
            )
        })
        .collect::<stream::FuturesUnordered<_>>()
        .any(|reachable| async move { reachable })
        .await
}

/// Run `round` until it reports reachable, at most `attempts` times, `delay`
/// apart. Reachable on the first success; unreachable only when every round
/// failed. `stop` is polled before each round and every [`STOP_POLL`] while
/// a round or the delay is in flight; once true the probe abandons the work at
/// once and reports unreachable (the caller discards that verdict).
async fn confirm_reachable<S, F, Fut>(attempts: u32, delay: Duration, stop: S, mut round: F) -> bool
where
    S: Fn() -> bool,
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let until_stopped = |work: Fut| async {
        tokio::pin!(work);
        loop {
            tokio::select! {
                result = &mut work => return Some(result),
                _ = tokio::time::sleep(STOP_POLL) => {
                    if stop() {
                        return None;
                    }
                }
            }
        }
    };
    for attempt in 0..attempts {
        if attempt > 0 {
            let waited = until_stopped_sleep(delay, &stop).await;
            if !waited {
                return false;
            }
        }
        if stop() {
            return false;
        }
        match until_stopped(round()).await {
            Some(true) => return true,
            Some(false) => {}
            None => return false,
        }
    }
    false
}

/// Sleep `delay`, waking every [`STOP_POLL`] to check `stop`. False if stopped.
async fn until_stopped_sleep(delay: Duration, stop: &impl Fn() -> bool) -> bool {
    let deadline = tokio::time::Instant::now() + delay;
    loop {
        if stop() {
            return false;
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return true;
        }
        tokio::time::sleep((deadline - now).min(STOP_POLL)).await;
    }
}

/// Update `state.network_reachable`, emitting [`CONNECTIVITY_CHANGED_EVENT`]
/// only when the value actually changes — a dedup identical in spirit to
/// `AuthExpiredGuard`, so a steady online (or steady offline) run never spams
/// the frontend with a no-op event every tick.
fn set_network_reachable(state: &AppState, reachable: bool) {
    let previous = state
        .network_reachable
        .swap(reachable, std::sync::atomic::Ordering::SeqCst);
    if previous != reachable {
        tracing::info!(
            "Network reachability changed: {} -> {}",
            previous,
            reachable
        );
        emit_event(
            state,
            CONNECTIVITY_CHANGED_EVENT,
            ConnectivityChangedEvent { reachable },
        );
    }
}

/// Check if a library sync is due for `kind` based on settings.
///
/// Each backend keeps its own last-sync timestamp, so Jellyfin being
/// up-to-date never makes OpenSubsonic "not due" (and vice-versa). The legacy
/// global [`keys::LAST_LIBRARY_SYNC`] predates multi-backend support and only
/// ever recorded Jellyfin's sync time, so it is a read-only fallback for
/// Jellyfin alone: an older install upgrading does not trigger a
/// redundant immediate Jellyfin re-sync. A backend other than Jellyfin — most
/// concretely a freshly-added OpenSubsonic backend with no per-backend
/// timestamp of its own — must never inherit that unrelated global timestamp,
/// or its first sync is silently deferred by up to a full interval. No write
/// ever happens on this read path.
async fn is_sync_due_for(db: &Arc<Database>, kind: ServerKind) -> bool {
    // Get the sync interval setting (0 = manual only)
    let interval: u64 = match db.get_setting(keys::LIBRARY_SYNC_INTERVAL) {
        Ok(Some(val)) => val.parse().unwrap_or(0),
        _ => 0,
    };

    if interval == 0 {
        return false;
    }

    // Get the last sync timestamp for this backend, falling back to the legacy
    // global key only for Jellyfin, the backend it actually belongs to.
    let read = |key: &str| -> Option<i64> {
        db.get_setting(key)
            .ok()
            .flatten()
            .and_then(|v| v.parse::<i64>().ok())
    };
    let last_sync: i64 = read(&keys::last_library_sync_key(kind))
        .or_else(|| {
            if kind == ServerKind::Jellyfin {
                read(keys::LAST_LIBRARY_SYNC)
            } else {
                None
            }
        })
        .unwrap_or(0);

    if last_sync == 0 {
        // Never synced, sync is due
        return true;
    }

    // Check if enough time has passed
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let elapsed = now - last_sync;
    elapsed >= interval as i64
}

/// Run `op` over every id with a bounded number in flight and a per-item
/// timeout, discarding results.
///
/// `for_each_concurrent` hard-caps in-flight futures at `concurrency`;
/// `tokio::time::timeout` cancels any single item that outlives
/// `per_item_timeout` so one stalled request cannot pin a slot. Generic and
/// side-effect-free so it can be unit-tested without any network.
async fn bounded_warm<F, Fut>(
    ids: Vec<String>,
    concurrency: usize,
    per_item_timeout: Duration,
    op: F,
) where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    stream::iter(ids)
        .for_each_concurrent(concurrency, |id| {
            let fut = op(id.clone());
            async move {
                if tokio::time::timeout(per_item_timeout, fut).await.is_err() {
                    tracing::debug!("Warm task item timed out for {}", id);
                }
            }
        })
        .await;
}

/// Warm the on-disk artwork cache for `ids`, detached from the sync critical
/// path.
///
/// This runs in a spawned background task that OVERLAPS PHASE B track fetch
/// rather than preceding it. The resolved URL is intentionally discarded: DB
/// rows already store the remote Jellyfin URL and the frontend renders
/// cache-first with a transparent remote fallback (`AlbumCard`/`ArtistCard`
/// `onerror` -> `artwork_url`), so a not-yet-warmed image simply falls back
/// until the next paint. A failed or slow download therefore never affects
/// sync metrics or stale cleanup.
async fn warm_artwork_cache(
    client: MediaClient,
    versions: std::collections::HashMap<String, String>,
    cache_dir: std::path::PathBuf,
) {
    let ids: Vec<String> = versions.keys().cloned().collect();
    let versions = Arc::new(versions);
    bounded_warm(ids, ARTWORK_CONCURRENCY, ARTWORK_ITEM_TIMEOUT, |id| {
        let client = client.clone();
        let cache_dir = cache_dir.clone();
        let versions = versions.clone();
        async move {
            let _ = cache_artwork(&client, &id, &versions[&id], &cache_dir).await;
        }
    })
    .await;
}

/// Prefetch every playlist's tracks into the local cache, detached from
/// the sync critical path like [`warm_artwork_cache`].
///
/// Populates `cached_playlist_tracks` so the sidebar collage renders on a
/// cold launch without the user opening each playlist first. Bounded + per-item
/// timeout so N playlists can never storm a self-hosted server, and every
/// write is stamped with the pinned `source` — never `db.active_source()`
/// — so a background sync of the inactive backend can't mislabel the other
/// backend's rows. Returns the number of playlists successfully written to the cache.
async fn warm_playlist_cache(client: MediaClient, db: Arc<Database>, source: ServerKind) -> u32 {
    let playlists = match client.get_playlists().await {
        Ok(playlists) => playlists,
        Err(e) => {
            // Non-fatal: the sidebar simply keeps the generic icon until the
            // next sync, exactly as it did before this prefetch existed.
            tracing::debug!("Playlist prefetch skipped ({}): {}", source.as_str(), e);
            return 0;
        }
    };

    let ids: Vec<String> = playlists.into_iter().map(|p| p.jellyfin_id).collect();
    if ids.is_empty() {
        return 0;
    }

    let warmed = Arc::new(AtomicU32::new(0));
    let warmed_counter = warmed.clone();
    bounded_warm(
        ids,
        PLAYLIST_PREFETCH_CONCURRENCY,
        PLAYLIST_PREFETCH_ITEM_TIMEOUT,
        |id| {
            let client = client.clone();
            let db = db.clone();
            let warmed = warmed_counter.clone();
            async move {
                match client.get_playlist_tracks(&id).await {
                    Ok(detail) => match db.replace_playlist_tracks(&id, &detail.tracks, source) {
                        Ok(()) => {
                            warmed.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(e) => {
                            tracing::debug!("Failed to cache prefetched playlist {}: {}", id, e)
                        }
                    },
                    Err(e) => tracing::debug!("Playlist track prefetch failed for {}: {}", id, e),
                }
            }
        },
    )
    .await;

    warmed.load(Ordering::Relaxed)
}

/// Fetch and map one album's tracks, updating the shared sync counters.
///
/// Runs as one unit of work inside the concurrent PHASE B stream. Every call
/// counts exactly one API request (success or failure); a failure is also
/// tallied so [`cleanup_stale_entries`] can defer stale deletion. On error the
/// album is logged and skipped by returning an empty Vec, so a single bad album
/// never aborts the whole sync. The field mapping mirrors the previous
/// sequential code exactly.
///
/// This is a named `async fn` rather than an inline `async move` closure so its
/// borrowed parameters get correct higher-ranked lifetime elision — a closure
/// returning an async block that borrows its argument fails to type-check here.
async fn fetch_album_tracks_into_cached(
    client: &MediaClient,
    album: &crate::models::Album,
    api_counter: &AtomicU32,
    failure_counter: &AtomicU32,
) -> Vec<CachedTrack> {
    // Capture server-side play counts in the same browse request where the
    // backend reports them (OpenSubsonic's inline `playCount`) — free during
    // sync, surfaced later in the visible "Plays" column. Jellyfin
    // returns an empty map here (its counts come from a separate UserData fetch).
    match client
        .get_album_tracks_with_play_counts(&album.jellyfin_id)
        .await
    {
        Ok((tracks, play_counts)) => {
            api_counter.fetch_add(1, Ordering::Relaxed);
            tracks
                .iter()
                .map(|t| CachedTrack::from_track(t, play_counts.get(&t.jellyfin_id).copied()))
                .collect()
        }
        Err(e) => {
            api_counter.fetch_add(1, Ordering::Relaxed);
            failure_counter.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                "Skipping tracks for album '{}' ({}): {}",
                album.name,
                album.jellyfin_id,
                e
            );
            Vec::new()
        }
    }
}

/// Perform a library sync of the ACTIVE backend with full metrics collection.
///
/// Thin wrapper over [`perform_sync_for_client`] used by the manual
/// `sync_library` command: it reads the active client from `state.media`
/// (preserving the "Not connected" error surfaced to the UI) and delegates.
pub async fn perform_sync_with_metrics(state: &AppState) -> Result<SyncMetrics, String> {
    // Clone the active client so we can drop the lock before the sync body.
    let client = state
        .media
        .read()
        .await
        .as_ref()
        .ok_or_else(|| "Not connected to Jellyfin server".to_string())?
        .clone();

    perform_sync_for_client(state, client).await
}

/// Perform a library sync for a specific `client` with full metrics collection.
///
/// Used by the manual sync command (via [`perform_sync_with_metrics`]) and the
/// background scheduler, which may pass an ephemeral client for a backend that
/// is not the active `state.media` one. Every write is stamped with
/// `client.kind()` (never `db.active_source()`) so a background sync of
/// the inactive backend can never mislabel or delete the other backend's rows.
pub async fn perform_sync_for_client(
    state: &AppState,
    client: MediaClient,
) -> Result<SyncMetrics, String> {
    let backend = client.kind().as_str();

    // One span over the whole sync, so Tempo shows its several hundred client
    // spans as one trace instead of that many roots. On the audit target
    // because that is the only target that leaves the device, and content-free
    // by construction: a backend name, durations and counts, never a title.
    let span = tracing::info_span!(
        target: crate::audit::TARGET,
        "library sync",
        backend,
        otel.status_code = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
        api_requests = tracing::field::Empty,
        track_count = tracing::field::Empty,
        failed_fetches = tracing::field::Empty,
    );

    // Emitted on the ONE path both the manual `sync_library` command and the
    // scheduler funnel through: the frontend's single, trigger-
    // agnostic "a sync is running" signal, replacing three independent local
    // booleans (toolbar, Settings, and a scheduled sync the frontend previously
    // had no visibility into at all — its result never reached the in-memory
    // library state until the user happened to trigger their own sync).
    emit_event(state, SYNC_STARTED_EVENT, SyncBackendEvent { backend });

    let metrics = sync_backend(state, client).instrument(span.clone()).await;

    match &metrics {
        Ok(m) => {
            span.record("duration_ms", m.duration_ms);
            span.record("api_requests", m.api_requests);
            span.record("track_count", m.track_count);
            span.record("failed_fetches", m.failed_fetches);
            emit_event(
                state,
                SYNC_COMPLETED_EVENT,
                SyncCompletedEvent {
                    backend,
                    metrics: m.clone(),
                },
            );
        }
        // The rendered AppError is safe by the time it reaches here: every
        // Jellyfin/Subsonic call site that could carry a request URL strips
        // it before building the `AppError` in the first place (`without_url`
        // in `jellyfin::library` and `mediaserver::subsonic::client`), so the
        // aggregated message below is never more than that.
        Err(e) => {
            span.record("otel.status_code", "ERROR");
            telemetry::report_error_with_cause(
                "sync failed",
                &crate::error::MessageError(e.clone()),
            );
            emit_event(
                state,
                SYNC_FAILED_EVENT,
                SyncFailedEvent {
                    backend,
                    error: e.clone(),
                },
            );
        }
    }
    metrics
}

async fn sync_backend(state: &AppState, client: MediaClient) -> Result<SyncMetrics, String> {
    // Pin the backend being synced. Every write in this sync — the
    // upserts AND the stale deletes — is tagged with this snapshot, NOT the live
    // `db.active_source()`.
    let sync_source = client.kind();

    // Per-backend single-flight. Both the manual `sync_library`
    // command and the scheduler funnel through here, and the frontend has
    // several independent sync triggers (first load, toolbar button, settings
    // button, reconnect, plus the scheduler) with no cross-coordination. A
    // non-blocking `try_lock` on THIS backend's guard coalesces any concurrent
    // trigger for the same backend into a benign no-op rather than running a
    // second full ~2000-request scan that doubles the load on that host. Keyed
    // per backend so a long background sync of the inactive backend never
    // coalesces the user's manual sync of the active one. The guard is held for
    // the whole sync via RAII, released on every `?` early-return and the final
    // `Ok`.
    let sync_lock = state.sync_lock_for(sync_source);
    let _sync_guard = match sync_lock.try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            tracing::info!(
                "Library sync for {} already in progress; coalescing this trigger into a no-op",
                sync_source.as_str()
            );
            return Ok(SyncMetrics::skipped());
        }
    };

    let overall_start = Instant::now();
    let mut fetch_time_ms: u64 = 0;
    let mut db_time_ms: u64 = 0;
    let mut api_requests: u32 = 0;
    let mut total_failures: u32 = 0;

    let sync_start = current_timestamp();
    let artwork_cache_dir = &state.artwork_cache_dir;

    // A mid-sync backend toggle/disconnect flips `db.active_source()`
    // concurrently; stamping writes from it would mislabel this backend's real
    // rows as the other backend's, and a later sync of that other backend would
    // then permanently delete them. That is why `sync_source` above is pinned
    // from `client.kind()` and every upsert/stale-delete below uses it. Only
    // cache READS use the live active source.

    // Sync artists
    let fetch_start = Instant::now();
    let artists = client.get_artists().await.map_err(|e| e.to_string())?;
    fetch_time_ms += fetch_start.elapsed().as_millis() as u64;
    api_requests += 1;

    let cached_artists: Vec<CachedArtist> = artists
        .iter()
        .map(|a| CachedArtist {
            jellyfin_id: a.jellyfin_id.clone(),
            artist_mbid: a.mbid.clone(),
            name: a.name.clone(),
            artwork_url: a.artwork_url.clone(),
            backdrop_url: a.backdrop_url.clone(),
            album_count: Some(a.album_count),
            sort_name: a.sort_name.clone(),
        })
        .collect();

    let db_start = Instant::now();
    state
        .db
        .upsert_artists(&cached_artists, sync_source, Some(sync_start))
        .map_err(|e| format!("Failed to cache artists: {}", e))?;
    db_time_ms += db_start.elapsed().as_millis() as u64;

    // Sync albums
    let fetch_start = Instant::now();
    let albums = client.get_albums().await.map_err(|e| e.to_string())?;
    fetch_time_ms += fetch_start.elapsed().as_millis() as u64;
    api_requests += 1;

    let total_albums = albums.len();
    let mut total_tracks = 0;

    // Detach artwork warming from the sync critical path. Previously
    // artist and album artwork were downloaded via an unbounded fan-out that was
    // awaited inline before track fetch, so one 30s image stall stalled the whole
    // sync and hundreds of concurrent image requests provoked Jellyfin 503 bursts.
    // The resolved URLs were discarded anyway (DB rows keep the remote Jellyfin
    // URL; the frontend renders cache-first with remote fallback), so this is pure
    // disk-cache warming. Spawn it bounded + timed-out and never await it: sync
    // metrics, duration, and stale cleanup are computed independently of artwork.
    // The cache version is the image tag (or Subsonic's versioned coverArt id)
    // inside the artwork URL, never the URL itself: a host or scheme change
    // must not refetch every cover.
    let artwork_ids: std::collections::HashMap<String, String> = artists
        .iter()
        .filter_map(|a| {
            Some((
                a.jellyfin_id.clone(),
                artwork_version(a.artwork_url.as_deref()?)?,
            ))
        })
        .chain(albums.iter().filter_map(|a| {
            Some((
                a.jellyfin_id.clone(),
                artwork_version(a.artwork_url.as_deref()?)?,
            ))
        }))
        .collect();
    if !artwork_ids.is_empty() {
        let art_client = client.clone();
        let art_cache_dir = artwork_cache_dir.clone();
        tauri::async_runtime::spawn(async move {
            warm_artwork_cache(art_client, artwork_ids, art_cache_dir).await;
        });
    }

    // PHASE A: upsert album metadata in batches. Track fetching is deferred to
    // PHASE B so it can run concurrently rather than serially per album.
    for album_batch in albums.chunks(ALBUM_BATCH_SIZE) {
        let cached_albums: Vec<CachedAlbum> = album_batch
            .iter()
            .map(|a| CachedAlbum {
                jellyfin_id: a.jellyfin_id.clone(),
                release_mbid: a.release_mbid.clone(),
                name: a.name.clone(),
                artist_name: Some(a.artist_name.clone()),
                artist_jellyfin_id: Some(a.artist_jellyfin_id.clone()),
                year: a.year,
                genre: a.genre.clone(),
                artwork_url: a.artwork_url.clone(),
                track_count: Some(a.track_count),
                duration_ms: Some(a.duration_ms),
                // The server's own date-added — this is what makes the Recently
                // Added view mean anything.
                date_created: a.date_created,
            })
            .collect();

        let db_start = Instant::now();
        state
            .db
            .upsert_albums(&cached_albums, sync_source, Some(sync_start))
            .map_err(|e| format!("Failed to cache albums: {}", e))?;
        db_time_ms += db_start.elapsed().as_millis() as u64;
    }

    // PHASE B: fetch tracks for every album concurrently, capped at
    // CONCURRENT_FETCH_LIMIT in flight. Each future maps its source tracks to
    // CachedTrack (identical field mapping to the previous sequential code) and
    // returns them; a per-album failure is logged and counted but yields an
    // empty Vec so a single bad album never aborts the whole sync.
    //
    // Accounting runs through atomics because the futures execute concurrently:
    // every album fetch is one API request (success or failure), and failures
    // are tallied separately so cleanup_stale_entries can defer stale deletion.
    // The client and counters are bound by shared reference before the stream —
    // a `&T` is Copy, so each per-album future can borrow them without moving the
    // owned client/atomics (which would not compile).
    let concurrent_api_requests = AtomicU32::new(0);
    let concurrent_failures = AtomicU32::new(0);
    let client_ref = &client;
    let api_counter = &concurrent_api_requests;
    let failure_counter = &concurrent_failures;

    // Build the futures eagerly so each borrows its `album` at one concrete
    // lifetime; a lazy `.map()` closure over the stream would instead demand a
    // higher-ranked bound the borrow checker cannot satisfy for the spawned task.
    let track_futures: Vec<_> = albums
        .iter()
        .map(|album| {
            fetch_album_tracks_into_cached(client_ref, album, api_counter, failure_counter)
        })
        .collect();

    let fetch_start = Instant::now();
    let results: Vec<Vec<CachedTrack>> = stream::iter(track_futures)
        .buffer_unordered(CONCURRENT_FETCH_LIMIT)
        .collect::<Vec<_>>()
        .await;
    fetch_time_ms += fetch_start.elapsed().as_millis() as u64;

    api_requests += concurrent_api_requests.load(Ordering::Relaxed);
    total_failures += concurrent_failures.load(Ordering::Relaxed);

    // Batch-insert the fetched tracks, flattening each chunk of per-album Vecs.
    for track_batch in results.chunks(ALBUM_BATCH_SIZE) {
        let cached_tracks: Vec<CachedTrack> = track_batch.iter().flatten().cloned().collect();
        if !cached_tracks.is_empty() {
            let db_start = Instant::now();
            state
                .db
                .upsert_tracks(&cached_tracks, sync_source, Some(sync_start))
                .map_err(|e| format!("Failed to cache tracks: {}", e))?;
            db_time_ms += db_start.elapsed().as_millis() as u64;
            total_tracks += cached_tracks.len();
        }
    }

    // Clean up stale entries (scoped to the synced backend)
    let db_start = Instant::now();
    let (stale_tracks, stale_albums, stale_artists) =
        cleanup_stale_entries(&state.db, sync_start, sync_source, total_failures)?;
    db_time_ms += db_start.elapsed().as_millis() as u64;

    // Store the sync timestamp keyed per backend so each backend's
    // interval advances independently.
    state
        .db
        .set_setting(
            &keys::last_library_sync_key(sync_source),
            &sync_start.to_string(),
        )
        .map_err(|e| format!("Failed to store sync timestamp: {}", e))?;

    // Prefetch playlist tracks into the local cache so the sidebar collage
    // renders on a cold launch without the user opening each playlist first.
    // Detached and bounded like the artwork warmer above so it never
    // blocks this sync's completion or the UI; spawned at the END of the sync
    // (not overlapping PHASE B) to keep peak server concurrency low. Runs in the
    // shared client path, so it covers both Jellyfin and OpenSubsonic and both
    // manual and scheduled syncs. On completion it emits an event so the sidebar
    // re-reads the now-warm cache in the same session.
    {
        let pl_client = client.clone();
        let pl_db = state.db.clone();
        let pl_source = sync_source;
        let pl_app = state.app_handle().cloned();
        tauri::async_runtime::spawn(async move {
            let warmed = warm_playlist_cache(pl_client, pl_db, pl_source).await;
            if warmed > 0 {
                if let Some(app) = pl_app {
                    if let Err(e) = app.emit(PLAYLISTS_WARMED_EVENT, ()) {
                        tracing::debug!("Failed to emit {} event: {}", PLAYLISTS_WARMED_EVENT, e);
                    }
                }
            }
        });
    }

    let duration_ms = overall_start.elapsed().as_millis() as u64;

    Ok(SyncMetrics {
        duration_ms,
        fetch_time_ms,
        db_time_ms,
        artist_count: cached_artists.len() as u32,
        album_count: total_albums as u32,
        track_count: total_tracks as u32,
        api_requests,
        stale_artists,
        stale_albums,
        stale_tracks,
        failed_fetches: total_failures,
        skipped: false,
    })
}

/// Delete entries not stamped by this sync pass.
///
/// Staleness is only trustworthy when every album fetch succeeded: a failed
/// fetch leaves that album's tracks unstamped this round, and deleting them
/// erodes the cache every time the server wobbles (a burst of 503s once
/// deleted 311 live tracks). With any failure the whole cleanup is deferred
/// to the next fully-successful sync — including albums and artists, so a
/// genuinely removed album can never orphan still-cached track rows.
fn cleanup_stale_entries(
    db: &crate::db::Database,
    sync_start: i64,
    source: ServerKind,
    failed_fetches: u32,
) -> Result<(u32, u32, u32), String> {
    if failed_fetches > 0 {
        tracing::warn!(
            "Skipping stale-entry cleanup: {} album track fetches failed this sync",
            failed_fetches
        );
        return Ok((0, 0, 0));
    }
    let source = source.as_str();
    let stale_tracks = db
        .delete_stale_tracks(sync_start, source)
        .map_err(|e| format!("Failed to clean stale tracks: {}", e))?;
    let stale_albums = db
        .delete_stale_albums(sync_start, source)
        .map_err(|e| format!("Failed to clean stale albums: {}", e))?;
    let stale_artists = db
        .delete_stale_artists(sync_start, source)
        .map_err(|e| format!("Failed to clean stale artists: {}", e))?;
    Ok((stale_tracks, stale_albums, stale_artists))
}

#[cfg(test)]
mod tests {
    use super::{
        any_reachable, apply_probe_result, cleanup_stale_entries, confirm_reachable,
        is_sync_due_for, probe_connectivity, set_network_reachable,
    };
    use crate::db::{settings::keys, CachedAlbum, CachedTrack, Database};
    use crate::error::AppError;
    use crate::mediaserver::ServerKind;
    use crate::state::AppState;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::time::Duration;

    const TRACK_ID: &str = "track1234567890123456789012345678";

    fn now_secs() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    /// Per-backend due-ness: a never-synced backend is due; a backend
    /// synced within the interval is not; after the interval it is again; and
    /// Jellyfin being up-to-date does NOT make OpenSubsonic "not due".
    #[tokio::test]
    async fn is_sync_due_for_is_per_backend() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        // 1h interval.
        db.set_setting(keys::LIBRARY_SYNC_INTERVAL, "3600").unwrap();

        // Interval of 0 (manual only) is never due, regardless of history.
        db.set_setting(keys::LIBRARY_SYNC_INTERVAL, "0").unwrap();
        assert!(!is_sync_due_for(&db, ServerKind::Jellyfin).await);
        db.set_setting(keys::LIBRARY_SYNC_INTERVAL, "3600").unwrap();

        // Never synced → both backends due.
        assert!(is_sync_due_for(&db, ServerKind::Jellyfin).await);
        assert!(is_sync_due_for(&db, ServerKind::OpenSubsonic).await);

        // Jellyfin just synced → Jellyfin not due, but OpenSubsonic still is.
        db.set_setting(
            &keys::last_library_sync_key(ServerKind::Jellyfin),
            &now_secs().to_string(),
        )
        .unwrap();
        assert!(!is_sync_due_for(&db, ServerKind::Jellyfin).await);
        assert!(
            is_sync_due_for(&db, ServerKind::OpenSubsonic).await,
            "Jellyfin being up-to-date must not make OpenSubsonic 'not due'"
        );

        // OpenSubsonic synced well in the past (2h ago, interval 1h) → due again.
        db.set_setting(
            &keys::last_library_sync_key(ServerKind::OpenSubsonic),
            &(now_secs() - 7200).to_string(),
        )
        .unwrap();
        assert!(is_sync_due_for(&db, ServerKind::OpenSubsonic).await);
    }

    /// Fallback: the global `LAST_LIBRARY_SYNC` holds Jellyfin's sync time
    /// alone. With
    /// the per-backend key absent, `is_sync_due_for` uses it as a read-only
    /// fallback for Jellyfin ALONE (so a freshly-upgraded install does not
    /// immediately re-sync Jellyfin) and writes nothing. A backend other than
    /// Jellyfin — most concretely a freshly-added OpenSubsonic backend with no
    /// per-backend timestamp of its own — must NOT inherit that unrelated
    /// global timestamp: it is due immediately, even though the legacy value
    /// exists and is recent.
    #[tokio::test]
    async fn is_sync_due_for_honours_legacy_timestamp_for_jellyfin_only() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        db.set_setting(keys::LIBRARY_SYNC_INTERVAL, "3600").unwrap();
        // Legacy global timestamp = just now; per-backend keys are absent.
        db.set_setting(keys::LAST_LIBRARY_SYNC, &now_secs().to_string())
            .unwrap();

        assert!(
            !is_sync_due_for(&db, ServerKind::Jellyfin).await,
            "a recent legacy timestamp must suppress an immediate re-sync on upgrade for Jellyfin"
        );
        assert!(
            is_sync_due_for(&db, ServerKind::OpenSubsonic).await,
            "a freshly-added backend with no per-backend timestamp must not inherit \
             Jellyfin's legacy global timestamp, and must sync immediately"
        );

        // The read path must not have materialised any per-backend key.
        assert!(db
            .get_setting(&keys::last_library_sync_key(ServerKind::Jellyfin))
            .unwrap()
            .is_none());
        assert!(db
            .get_setting(&keys::last_library_sync_key(ServerKind::OpenSubsonic))
            .unwrap()
            .is_none());
    }

    /// An already-synced backend must not be forced to re-sync early merely
    /// because a stale legacy global timestamp is also present: the
    /// per-backend timestamp always takes priority over the legacy fallback.
    #[tokio::test]
    async fn is_sync_due_for_prefers_per_backend_timestamp_over_legacy() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        db.set_setting(keys::LIBRARY_SYNC_INTERVAL, "3600").unwrap();
        // Legacy global timestamp is stale (2h ago, interval 1h) ...
        db.set_setting(keys::LAST_LIBRARY_SYNC, &(now_secs() - 7200).to_string())
            .unwrap();
        // ... but OpenSubsonic has its own recent per-backend timestamp.
        db.set_setting(
            &keys::last_library_sync_key(ServerKind::OpenSubsonic),
            &now_secs().to_string(),
        )
        .unwrap();

        assert!(
            !is_sync_due_for(&db, ServerKind::OpenSubsonic).await,
            "a backend with its own recent timestamp must not be forced to re-sync early"
        );
    }

    /// Seed one album and one track, both stamped at `stamp`.
    fn seed(db: &Database, stamp: i64) {
        let album = CachedAlbum {
            jellyfin_id: "album123456789012345678901234567".to_string(),
            release_mbid: None,
            name: "Seed Album".to_string(),
            artist_name: Some("Seed Artist".to_string()),
            artist_jellyfin_id: None,
            year: None,
            genre: None,
            artwork_url: None,
            track_count: Some(1),
            duration_ms: None,
            date_created: None,
        };
        db.upsert_album(&album, ServerKind::Jellyfin, Some(stamp))
            .unwrap();
        let track = CachedTrack {
            jellyfin_id: TRACK_ID.to_string(),
            track_mbid: None,
            artist_mbid: None,
            release_mbid: None,
            name: "Seed Track".to_string(),
            artist_name: Some("Seed Artist".to_string()),
            album_jellyfin_id: Some(album.jellyfin_id.clone()),
            track_number: Some(1),
            disc_number: Some(1),
            duration_ms: Some(200_000),
            genre: None,
            artwork_url: None,
            cached_file_path: None,
            file_size_bytes: None,
            normalization_gain_db: None,
            server_play_count: None,
        };
        db.upsert_tracks(&[track], ServerKind::Jellyfin, Some(stamp))
            .unwrap();
    }

    /// A failed album fetch leaves that album's tracks unstamped; deleting
    /// them would erode the cache on every server wobble. Any failure must
    /// defer the whole cleanup.
    #[test]
    fn cleanup_deferred_when_any_fetch_failed() {
        let db = Database::open_in_memory().unwrap();
        seed(&db, 1_000);

        let (tracks, albums, artists) =
            cleanup_stale_entries(&db, 2_000, ServerKind::Jellyfin, 1).unwrap();

        assert_eq!((tracks, albums, artists), (0, 0, 0));
        assert!(
            db.get_track(TRACK_ID).unwrap().is_some(),
            "unstamped track must survive a sync with fetch failures"
        );
    }

    #[test]
    fn cleanup_removes_stale_entries_on_clean_sync() {
        let db = Database::open_in_memory().unwrap();
        seed(&db, 1_000);

        let (tracks, _albums, _artists) =
            cleanup_stale_entries(&db, 2_000, ServerKind::Jellyfin, 0).unwrap();

        assert_eq!(tracks, 1);
        assert!(
            db.get_track(TRACK_ID).unwrap().is_none(),
            "stale track must be removed by a fully-successful sync"
        );
    }

    /// The detached artwork warmer must (a) never exceed its concurrency cap and
    /// (b) cancel any item that outlives the per-item timeout. Drives the generic
    /// `bounded_warm` with a fake slow endpoint (no network): every 5th id sleeps
    /// past the timeout, the rest finish well inside it.
    #[tokio::test]
    async fn bounded_warm_caps_concurrency_and_times_out_slow_items() {
        use std::sync::atomic::{AtomicUsize, Ordering as O};
        use std::sync::Arc;
        use std::time::Duration;

        // Decrements the in-flight counter even when the future is cancelled by
        // the per-item timeout (Drop runs on cancellation at the await point).
        struct Guard(Arc<AtomicUsize>);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.fetch_sub(1, O::SeqCst);
            }
        }

        let ids: Vec<String> = (0..20).map(|i| i.to_string()).collect();
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));

        {
            let in_flight = in_flight.clone();
            let max_in_flight = max_in_flight.clone();
            let completed = completed.clone();
            // Wide margins (1ms fast vs 100ms timeout vs 2000ms slow) keep the
            // completed==16 assertion robust even on a heavily loaded CI runner.
            super::bounded_warm(ids, 4, Duration::from_millis(100), move |id| {
                let in_flight = in_flight.clone();
                let max_in_flight = max_in_flight.clone();
                let completed = completed.clone();
                async move {
                    let n = in_flight.fetch_add(1, O::SeqCst) + 1;
                    let _g = Guard(in_flight.clone());
                    max_in_flight.fetch_max(n, O::SeqCst);
                    // Fake slow endpoint: every 5th id (0,5,10,15) far exceeds the
                    // 100ms per-item timeout and is cancelled; the rest complete
                    // well inside it.
                    let idx: usize = id.parse().unwrap();
                    let delay = if idx.is_multiple_of(5) { 2_000 } else { 1 };
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    // Only reached when NOT cancelled by the per-item timeout.
                    completed.fetch_add(1, O::SeqCst);
                }
            })
            .await;
        }

        assert!(
            max_in_flight.load(O::SeqCst) <= 4,
            "bounded_warm must cap in-flight futures at 4, saw {}",
            max_in_flight.load(O::SeqCst)
        );
        assert_eq!(
            completed.load(O::SeqCst),
            16,
            "the 4 slow ids (0,5,10,15) must be cancelled by the per-item timeout"
        );
        assert_eq!(
            in_flight.load(O::SeqCst),
            0,
            "every future (completed or cancelled) must release its slot"
        );
    }

    /// A coalesced (single-flight) sync returns a zeroed, flagged result so the
    /// UI can treat it as a benign no-op rather than a "synced 0 items" success.
    #[test]
    fn skipped_metrics_are_zeroed_and_flagged() {
        let m = crate::commands::SyncMetrics::skipped();
        assert!(m.skipped, "coalesced sync must be flagged skipped");
        assert_eq!(
            (m.track_count, m.album_count, m.artist_count, m.api_requests),
            (0, 0, 0, 0),
            "a skipped sync must report zero work"
        );
    }

    fn test_state() -> AppState {
        AppState::new_without_audio(Database::open_in_memory().unwrap(), std::env::temp_dir())
    }

    /// `network_reachable` defaults to optimistic `true`: a cold
    /// start must never show a false "offline" banner before the first probe
    /// completes.
    #[test]
    fn network_reachable_defaults_to_true() {
        let state = test_state();
        assert!(state.network_reachable.load(Ordering::SeqCst));
    }

    /// `set_network_reachable` actually updates the flag, both directions.
    #[test]
    fn set_network_reachable_updates_the_flag() {
        let state = test_state();
        set_network_reachable(&state, false);
        assert!(!state.network_reachable.load(Ordering::SeqCst));
        set_network_reachable(&state, true);
        assert!(state.network_reachable.load(Ordering::SeqCst));
    }

    /// With no `AppHandle` installed (as in every unit test here), emitting the
    /// change event is a silent no-op rather than a panic — the same contract
    /// every other emit in this module already relies on.
    #[test]
    fn set_network_reachable_is_safe_without_an_app_handle() {
        let state = test_state();
        // Would panic on an unwrap of a None app_handle if the no-app-handle
        // path were not actually guarded.
        set_network_reachable(&state, false);
        set_network_reachable(&state, false);
        set_network_reachable(&state, true);
    }

    /// With no credentialed backend at all, the probe is a no-op: there is
    /// nothing to be "offline" from, so the optimistic default is left alone
    /// rather than being dragged to "unreachable" by an absent connection.
    #[tokio::test]
    async fn probe_connectivity_is_a_noop_with_no_credentials() {
        let state = test_state();
        probe_connectivity(&state).await;
        assert!(
            state.network_reachable.load(Ordering::SeqCst),
            "no credentials means nothing was probed, so the flag must stay at its default"
        );
    }

    /// A probe that fails before the network is up must not condemn the
    /// server: one later success inside the confirmation window wins.
    #[tokio::test]
    async fn confirm_reachable_survives_a_transient_failure() {
        let calls = std::sync::atomic::AtomicU32::new(0);
        let reachable = confirm_reachable(
            3,
            Duration::ZERO,
            || false,
            || async { calls.fetch_add(1, Ordering::SeqCst) >= 1 },
        )
        .await;
        assert!(reachable);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "stops at the first success"
        );
    }

    #[tokio::test]
    async fn confirm_reachable_declares_unreachable_only_after_every_round_fails() {
        let calls = std::sync::atomic::AtomicU32::new(0);
        let reachable = confirm_reachable(
            3,
            Duration::ZERO,
            || false,
            || async {
                calls.fetch_add(1, Ordering::SeqCst);
                false
            },
        )
        .await;
        assert!(!reachable);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    /// Offline Mode switched on mid-probe ends it at once: not after the
    /// in-flight round's budget, and no further round starts.
    #[tokio::test]
    async fn confirm_reachable_stops_at_once_when_offline_mode_is_switched_on() {
        let calls = std::sync::atomic::AtomicU32::new(0);
        let offline = std::sync::atomic::AtomicBool::new(false);
        let started = std::time::Instant::now();
        let reachable = confirm_reachable(
            3,
            Duration::from_secs(5),
            || offline.load(Ordering::SeqCst),
            || async {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    offline.store(true, Ordering::SeqCst);
                }
                std::future::pending::<bool>().await
            },
        )
        .await;
        assert!(!reachable);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    /// A probe that never answers is cut at its own budget, not the client's
    /// 30 s request timeout.
    #[tokio::test]
    async fn a_blackholed_probe_is_cut_at_its_own_timeout() {
        let reachable = any_reachable(
            std::iter::once(std::future::pending::<Result<(), AppError>>()),
            Duration::from_millis(20),
        )
        .await;
        assert!(!reachable);
    }

    /// One backend answering is enough, even beside a hung one.
    #[tokio::test]
    async fn one_answering_backend_is_reachable_beside_a_hung_one() {
        type Probe = std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), AppError>>>>;
        let hung: Probe = Box::pin(std::future::pending());
        let up: Probe = Box::pin(async { Ok(()) });
        let reachable = any_reachable([hung, up].into_iter(), Duration::from_secs(5)).await;
        assert!(reachable);
    }

    /// A late probe verdict never overrides Offline Mode.
    #[test]
    fn a_late_probe_result_respects_offline_mode() {
        let state = test_state();
        state.db.set_setting(keys::OFFLINE_MODE, "true").unwrap();
        apply_probe_result(&state, false);
        assert!(
            state.network_reachable.load(Ordering::SeqCst),
            "offline mode on: the probe's verdict is discarded"
        );
        state.db.set_setting(keys::OFFLINE_MODE, "false").unwrap();
        apply_probe_result(&state, false);
        assert!(!state.network_reachable.load(Ordering::SeqCst));
    }
}
