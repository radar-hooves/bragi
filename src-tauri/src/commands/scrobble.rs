//! Scrobbling-related Tauri commands.
//!
//! Handles recording track listens to the music service,
//! with offline queue support for when the service is unavailable.

use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::audit;
use crate::db::{keys, ScrobblePayload, SkipPayload, SyncOperation};
use crate::error::CommandResult;
use crate::services::{
    clear_music_service_credentials, store_music_service_credentials, ListenHistoryEntry,
    MusicServiceClient, MusicServiceCredentials, ScrobbleRequest, SkipRequest, SyncStatus,
};
use crate::state::AppState;

/// Maximum number of retry attempts before giving up on a sync operation.
const MAX_RETRY_ATTEMPTS: u32 = 5;

/// Base delay for exponential backoff (in seconds).
const BASE_BACKOFF_SECONDS: i64 = 30;

/// Cooldown between Jellyfin MBID re-resolve attempts for an already-held
/// scrobble. A brand-new held candidate (no `last_attempt_at` yet) always
/// bypasses this so it gets an immediate resolve attempt in the same drain
/// that discovered it; only a REVISIT of an already-held row is throttled, so
/// a busy library doesn't hammer Jellyfin with an `Items` call on every drain.
const HELD_RESOLVE_COOLDOWN_SECONDS: i64 = 15 * 60;

/// Global flag to prevent concurrent sync operations.
static SYNC_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// Scrobble data from the frontend.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrobbleData {
    /// MusicBrainz track ID (optional).
    pub track_mbid: Option<String>,
    /// MusicBrainz artist ID (optional).
    pub artist_mbid: Option<String>,
    /// MusicBrainz release ID (optional).
    pub release_mbid: Option<String>,
    /// Jellyfin item ID.
    pub jellyfin_id: String,
    /// Artist name.
    pub artist_name: String,
    /// Track name.
    pub track_name: String,
    /// Album name (optional).
    pub album_name: Option<String>,
    /// Duration played in milliseconds.
    pub duration_ms: u64,
    /// Unix seconds when the track finished playing.
    /// Provided by the audio engine so offline-queued scrobbles carry the
    /// original play time, not the sync time.
    pub finished_at: i64,
}

/// Skip data from the frontend.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkipData {
    /// MusicBrainz track ID (optional).
    pub track_mbid: Option<String>,
    /// MusicBrainz artist ID (optional).
    pub artist_mbid: Option<String>,
    /// MusicBrainz release ID (optional).
    pub release_mbid: Option<String>,
    /// Jellyfin item ID.
    pub jellyfin_id: String,
    /// Artist name.
    pub artist_name: String,
    /// Track name.
    pub track_name: String,
    /// Album name (optional).
    pub album_name: Option<String>,
    /// Duration played before skip in milliseconds.
    pub played_ms: u64,
    /// Total track duration in milliseconds.
    pub track_duration_ms: u64,
    /// Unix seconds when the track was skipped.
    pub skipped_at: i64,
}

/// Music service connection status.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicServiceStatus {
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_url: Option<String>,
}

/// Get device ID, creating one if it doesn't exist.
pub(crate) fn get_or_create_device_id(state: &AppState) -> Result<String, String> {
    // Try to get existing device ID
    if let Ok(Some(device_id)) = state.db.get_setting(keys::DEVICE_ID) {
        return Ok(device_id);
    }

    // Generate a new device ID
    let device_id = format!("bragi-{}", uuid::Uuid::new_v4());
    state
        .db
        .set_setting(keys::DEVICE_ID, &device_id)
        .map_err(|e| format!("Failed to save device ID: {}", e))?;

    tracing::info!("Generated new device ID: {}", device_id);
    Ok(device_id)
}

/// Record a scrobble (track listen).
///
/// This will attempt to send the scrobble to the music service.
/// If the service is unavailable or not configured, it queues
/// the scrobble locally for later sync.
#[tauri::command]
pub async fn scrobble_track(
    state: tauri::State<'_, AppState>,
    data: ScrobbleData,
) -> CommandResult<bool> {
    tracing::info!("Scrobbling: {} - {}", data.artist_name, data.track_name);

    // Use the timestamp from the audio engine (when the track actually finished),
    // not the current time. This ensures offline-queued scrobbles carry the
    // original play time, not the sync time.
    let listened_at = DateTime::from_timestamp(data.finished_at, 0).unwrap_or_else(Utc::now);

    // A blank Jellyfin id can never be re-resolved by id — log it
    // loudly rather than let it queue and go silently, permanently stuck.
    // The scrobble is still queued and never dropped ("never lose a
    // scrobble"): the metadata-based fallback in `sync_all_inner` is what
    // actually recovers it.
    if data.jellyfin_id.is_empty() {
        tracing::error!(
            "Queuing a scrobble with an empty Jellyfin id for '{}' - '{}'; \
                 will only resolve by matching artist/album/title against the library",
            data.artist_name,
            data.track_name
        );
    }

    // Persist first — the local queue is the durability guarantee ("never
    // lose a scrobble"), and every wire submission flows through the one
    // native sync path rather than a separate live endpoint.
    let scrobble = ScrobblePayload {
        track_mbid: data.track_mbid,
        artist_mbid: data.artist_mbid,
        release_mbid: data.release_mbid,
        jellyfin_id: data.jellyfin_id,
        artist: data.artist_name,
        track: data.track_name,
        album: data.album_name,
        listened_at: listened_at.timestamp(),
        duration_ms: data.duration_ms,
    };

    state
        .db
        .queue_scrobble(&scrobble)
        .map_err(|e| format!("Failed to queue scrobble: {}", e))?;

    // Drain immediately when a service is configured; on failure the entry
    // stays queued with exponential backoff.
    if state.music_service.read().await.is_some() {
        if SYNC_IN_PROGRESS.swap(true, Ordering::SeqCst) {
            tracing::debug!("Sync already in progress; scrobble will ride the next drain");
        } else {
            let result = sync_all_inner(&state).await;
            SYNC_IN_PROGRESS.store(false, Ordering::SeqCst);
            if let Err(e) = result {
                tracing::warn!("Immediate scrobble sync failed; queued for retry: {}", e);
            }
        }
    }

    Ok(true)
}

/// Record a skip (track skipped before scrobble threshold).
///
/// This will attempt to send the skip to the music service.
/// If the service is unavailable or not configured, it queues
/// the skip locally for later sync.
#[tauri::command]
pub async fn skip_track(state: tauri::State<'_, AppState>, data: SkipData) -> CommandResult<bool> {
    tracing::info!(
        "Recording skip: {} - {} (played {}ms of {}ms)",
        data.artist_name,
        data.track_name,
        data.played_ms,
        data.track_duration_ms
    );

    let device_id = get_or_create_device_id(&state)?;
    let skipped_at = DateTime::from_timestamp(data.skipped_at, 0).unwrap_or_else(Utc::now);
    let skipped_at_str = skipped_at.to_rfc3339();

    // Check if music service is configured and extract what we need
    // without holding the lock during the HTTP request.
    let maybe_client = {
        let client = state.music_service.read().await;
        client.as_ref().map(|service| service.clone_for_request())
    };

    if let Some((http_client, base_url)) = maybe_client {
        // Try to send directly (lock is released)
        let request = SkipRequest {
            track_mbid: data.track_mbid.clone(),
            artist_name: data.artist_name.clone(),
            track_name: data.track_name.clone(),
            album_name: data.album_name.clone(),
            skipped_at: skipped_at_str.clone(),
            played_ms: data.played_ms,
            track_duration_ms: data.track_duration_ms,
            device_id: device_id.clone(),
            jellyfin_id: Some(data.jellyfin_id.clone()),
            artist_mbid: data.artist_mbid.clone(),
            release_mbid: data.release_mbid.clone(),
        };

        match MusicServiceClient::record_skip_with_client(&http_client, &base_url, &request).await {
            Ok(success) => {
                tracing::info!("Skip sent successfully");
                return Ok(success);
            }
            Err(e) => {
                tracing::warn!("Failed to send skip, queuing for later: {}", e);
                // Fall through to queue
            }
        }
    }

    // Same defensive log as `scrobble_track`: still queued, never
    // dropped, but a blank id can only recover via the metadata fallback.
    if data.jellyfin_id.is_empty() {
        tracing::error!(
            "Queuing a skip with an empty Jellyfin id for '{}' - '{}'; \
                 will only resolve by matching artist/album/title against the library",
            data.artist_name,
            data.track_name
        );
    }

    // Queue for later (either service not configured or send failed)
    let skip = SkipPayload {
        track_mbid: data.track_mbid,
        artist_mbid: data.artist_mbid,
        release_mbid: data.release_mbid,
        jellyfin_id: data.jellyfin_id,
        artist: data.artist_name,
        track: data.track_name,
        album: data.album_name,
        skipped_at: skipped_at.timestamp(),
        played_ms: data.played_ms,
        track_duration_ms: data.track_duration_ms,
    };

    state
        .db
        .queue_skip(&skip)
        .map_err(|e| format!("Failed to queue skip: {}", e))?;

    tracing::info!("Skip queued for later sync");
    Ok(true)
}

/// Sync all pending scrobbles and skips to the music service.
///
/// Returns the number of scrobbles synced successfully.
/// Uses a mutex to prevent concurrent sync operations, which could
/// lead to duplicate entries being sent.
#[tauri::command]
pub async fn sync_scrobbles(state: tauri::State<'_, AppState>) -> CommandResult<u32> {
    // Prevent concurrent sync operations using atomic flag
    if SYNC_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        tracing::info!("Sync already in progress, skipping");
        return Err("Sync already in progress".into());
    }

    // Ensure we always release the lock
    let result = sync_all_inner(&state).await;
    SYNC_IN_PROGRESS.store(false, Ordering::SeqCst);
    result
}

/// How often the background drain looks at the queue. Cheap when idle (one
/// local query); it is what retries a held listen once its cooldown elapses.
const BACKGROUND_DRAIN_INTERVAL_SECONDS: u64 = 60;

/// The `tracing` target for scrobble-queue state, allow-listed in `lib.rs`.
/// Its events carry counts only, never a title, artist or album.
pub const SCROBBLE_TARGET: &str = "bragi::scrobble";

/// Event emitted when a background drain submitted something, so the frontend
/// refreshes its pending count.
const SCROBBLE_QUEUE_DRAINED_EVENT: &str = "scrobble-queue-drained";

/// Drain the queue on a timer. Without it a queued or held listen is only
/// retried when a new listen or skip arrives or the user clicks sync, so a
/// listen held at the last play of a session sat unsent until the next one.
pub fn start_background_drain(app: tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(
            BACKGROUND_DRAIN_INTERVAL_SECONDS,
        ));
        loop {
            ticker.tick().await;
            let state = app.state::<AppState>();
            if state.music_service.read().await.is_none() {
                continue;
            }
            if SYNC_IN_PROGRESS.swap(true, Ordering::SeqCst) {
                continue;
            }
            let result = sync_all_inner(&state).await;
            SYNC_IN_PROGRESS.store(false, Ordering::SeqCst);
            let pending = state.db.get_pending_count().unwrap_or(0);
            match result {
                Ok(synced) if synced > 0 => {
                    tracing::info!(
                        target: SCROBBLE_TARGET,
                        synced,
                        pending,
                        "scrobble drain"
                    );
                    let _ = app.emit(SCROBBLE_QUEUE_DRAINED_EVENT, synced);
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("Background scrobble drain failed: {}", e),
            }
        }
    });
}

/// A media-library rebuild can replace a non-empty item id. Those rows need
/// the same metadata lookup as rows queued without an id.
fn needs_metadata_lookup(
    item_id: &str,
    live: Option<&std::collections::HashMap<String, Option<String>>>,
) -> bool {
    item_id.is_empty() || live.is_some_and(|items| !items.contains_key(item_id))
}

/// Re-resolve a held row whose item id is absent from the current library:
/// the local cache first, then an exact metadata match in live search.
///
/// Returns the matched track's (jellyfin_id, mbid) the moment either source
/// turns up a hit; `mbid` may still be `None` if the matched track itself
/// has no MusicBrainz recording ID yet, in which case the caller patches
/// just the id and lets the ordinary by-id path pick up the mbid later.
async fn resolve_by_metadata(
    state: &tauri::State<'_, AppState>,
    artist: &str,
    album: Option<&str>,
    title: &str,
    retired_id: &str,
) -> Option<(String, Option<String>)> {
    if let Ok(Some(track)) = state.db.find_track_by_metadata(artist, album, title) {
        if track.jellyfin_id != retired_id {
            return Some((track.jellyfin_id, track.mbid));
        }
    }

    let media_client = {
        let guard = state.media.read().await;
        guard.as_ref().cloned()
    };
    let client = media_client?;
    let results = client.search(title).await.ok()?;

    unique_metadata_match(results.tracks, artist, album, title, retired_id)
}

fn unique_metadata_match(
    tracks: Vec<crate::models::Track>,
    artist: &str,
    album: Option<&str>,
    title: &str,
    retired_id: &str,
) -> Option<(String, Option<String>)> {
    let mut matches = tracks.into_iter().filter(|t| {
        t.jellyfin_id != retired_id
            && t.name.eq_ignore_ascii_case(title)
            && t.artist_name.eq_ignore_ascii_case(artist)
            && album.is_none_or(|a| t.album_name.eq_ignore_ascii_case(a))
    });
    let track = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some((track.jellyfin_id, track.mbid))
}

/// Filter operations ready for retry based on exponential backoff.
///
/// A held scrobble OR skip (`held_no_mbid`) is exempt from the
/// attempts/backoff gate entirely — it is never `record_attempt`'d, so
/// `attempts` stays 0 forever — and instead gates on a flat cooldown against
/// `last_attempt_at` (reused as the last re-resolve stamp), so it is
/// revisited every drain until resolved rather than eventually hitting
/// `MAX_RETRY_ATTEMPTS` and disappearing. Operation-agnostic: it filters
/// `Vec<QueuedOperation>` without caring whether the caller passed scrobbles
/// or skips.
fn filter_ready_operations(
    operations: Vec<crate::db::QueuedOperation>,
    now: i64,
) -> Vec<crate::db::QueuedOperation> {
    operations
        .into_iter()
        .filter(|op| {
            if op.held_no_mbid {
                return match op.last_attempt_at {
                    Some(last) => now >= last + HELD_RESOLVE_COOLDOWN_SECONDS,
                    None => true,
                };
            }

            // Skip operations that have exceeded max attempts
            if op.attempts >= MAX_RETRY_ATTEMPTS {
                return false;
            }

            // If never attempted, it's ready
            if op.attempts == 0 || op.last_attempt_at.is_none() {
                return true;
            }

            // Calculate backoff: base * 2^attempts (capped at ~16 minutes)
            let backoff_seconds = BASE_BACKOFF_SECONDS * (1_i64 << op.attempts.min(5));
            let next_retry_at = op.last_attempt_at.unwrap() + backoff_seconds;

            now >= next_retry_at
        })
        .collect()
}

/// Whether "no recording id found" is a real answer: a Jellyfin lookup that
/// failed, or no server connected, says nothing about the track, while a
/// server with no recording ids to give (Subsonic) never will.
fn lookup_settled(media_client: Option<&crate::mediaserver::MediaClient>) -> bool {
    !matches!(
        media_client,
        None | Some(crate::mediaserver::MediaClient::Jellyfin(_))
    )
}

/// A play with no recording id waits only while the lookup that could still
/// give it one has not answered; once it has, the play goes by name.
fn hold_for_lookup(lookup_settled: bool, lookup_answered: bool) -> bool {
    !(lookup_settled || lookup_answered)
}

/// Inner sync implementation for both scrobbles and skips.
///
/// Separated to ensure the sync flag is always released.
async fn sync_all_inner(state: &tauri::State<'_, AppState>) -> CommandResult<u32> {
    tracing::info!("Syncing pending scrobbles and skips...");

    // Extract client info without holding the lock during HTTP requests
    let maybe_client = {
        let client = state.music_service.read().await;
        client.as_ref().map(|service| service.clone_for_request())
    };

    let (http_client, base_url) = match maybe_client {
        Some(client_info) => client_info,
        None => {
            tracing::warn!("Music service not configured, cannot sync");
            return Err("Music service not configured".into());
        }
    };

    let now = crate::db::utils::current_timestamp();
    let device_id = get_or_create_device_id(state)?;

    // Get pending scrobbles
    let pending_scrobbles = state
        .db
        .get_pending_operations_by_type(SyncOperation::Scrobble)
        .map_err(|e| format!("Failed to get pending scrobbles: {}", e))?;
    let ready_scrobbles = filter_ready_operations(pending_scrobbles, now);

    // Get pending skips
    let pending_skips = state
        .db
        .get_pending_operations_by_type(SyncOperation::Skip)
        .map_err(|e| format!("Failed to get pending skips: {}", e))?;
    let ready_skips = filter_ready_operations(pending_skips, now);

    if ready_scrobbles.is_empty() && ready_skips.is_empty() {
        tracing::info!("No pending operations ready to sync");
        return Ok(0);
    }

    tracing::info!(
        "Found {} scrobbles and {} skips ready for sync",
        ready_scrobbles.len(),
        ready_skips.len()
    );

    // Convert scrobbles to API requests, holding any without a MusicBrainz
    // recording ID back for Jellyfin re-resolution rather than sending them —
    // The scrobble server rejects an mbid-less scrobble/skip instead of
    // storing it, and Bragi must never fabricate one or silently mark it
    // synced.
    let mut scrobble_requests = Vec::new();
    let mut scrobble_op_ids = Vec::new();
    let mut held_scrobble_candidates: Vec<(i64, ScrobblePayload)> = Vec::new();

    for op in &ready_scrobbles {
        let payload: ScrobblePayload = serde_json::from_str(&op.payload)
            .map_err(|e| format!("Failed to parse scrobble payload: {}", e))?;

        if payload.track_mbid.is_none() {
            held_scrobble_candidates.push((op.id, payload));
            continue;
        }

        let listened_at = DateTime::from_timestamp(payload.listened_at, 0)
            .ok_or_else(|| "Invalid timestamp".to_string())?
            .to_rfc3339();

        scrobble_requests.push(ScrobbleRequest {
            track_mbid: payload.track_mbid,
            artist_name: payload.artist,
            track_name: payload.track,
            album_name: payload.album,
            listened_at,
            duration_ms: payload.duration_ms,
            device_id: device_id.clone(),
            jellyfin_id: Some(payload.jellyfin_id),
            artist_mbid: payload.artist_mbid,
            release_mbid: payload.release_mbid,
        });
        scrobble_op_ids.push(op.id);
    }

    // Convert skips to API requests, holding mbid-less ones back the same way
    // — the scrobble server's `/client/skips` also requires a track_mbid and rejects a
    // skip without one.
    let mut skip_requests = Vec::new();
    let mut skip_op_ids = Vec::new();
    let mut held_skip_candidates: Vec<(i64, SkipPayload)> = Vec::new();

    for op in &ready_skips {
        let payload: SkipPayload = serde_json::from_str(&op.payload)
            .map_err(|e| format!("Failed to parse skip payload: {}", e))?;

        if payload.track_mbid.is_none() {
            held_skip_candidates.push((op.id, payload));
            continue;
        }

        let skipped_at = DateTime::from_timestamp(payload.skipped_at, 0)
            .ok_or_else(|| "Invalid timestamp".to_string())?
            .to_rfc3339();

        skip_requests.push(SkipRequest {
            track_mbid: payload.track_mbid,
            artist_name: payload.artist,
            track_name: payload.track,
            album_name: payload.album,
            skipped_at,
            played_ms: payload.played_ms,
            track_duration_ms: payload.track_duration_ms,
            device_id: device_id.clone(),
            jellyfin_id: Some(payload.jellyfin_id),
            artist_mbid: payload.artist_mbid,
            release_mbid: payload.release_mbid,
        });
        skip_op_ids.push(op.id);
    }

    // Re-resolve held scrobbles AND skips together against LIVE Jellyfin
    // metadata (the local cached_tracks MBID is the stale data that caused
    // the hold in the first place) — at most ONE batched `Items` call per
    // drain covering both, rather than one call per operation type. Covers
    // both brand-new held candidates and already-held rows whose cooldown
    // elapsed. A resolved item is patched and rides THIS SAME drain; an
    // unresolved skip is (re-)marked held so the next drain revisits it, and an
    // unresolved scrobble goes by artist and title.
    if !held_scrobble_candidates.is_empty() || !held_skip_candidates.is_empty() {
        let media_client = {
            let guard = state.media.read().await;
            guard.as_ref().cloned()
        };
        let ids: std::collections::HashSet<String> = held_scrobble_candidates
            .iter()
            .map(|(_, payload)| payload.jellyfin_id.clone())
            .chain(
                held_skip_candidates
                    .iter()
                    .map(|(_, payload)| payload.jellyfin_id.clone()),
            )
            .filter(|id| !id.is_empty())
            .collect();
        let ids: Vec<String> = ids.into_iter().collect();
        // Whether "no id found" is a real answer: a Jellyfin lookup that
        // failed, or no server connected, says nothing about the track, and a
        // server with no recording ids to give (Subsonic) never will.
        let lookup_settled = lookup_settled(media_client.as_ref());
        let resolved = match media_client {
            Some(crate::mediaserver::MediaClient::Jellyfin(client)) => {
                match client.get_track_mbids(&ids).await {
                    Ok(items) => Some(items),
                    Err(error) => {
                        telemetry::report_error_with_cause(
                            "Held-track lookup failed; retaining queued listens",
                            &error,
                        );
                        None
                    }
                }
            }
            _ => None,
        };

        // Recover missing AND replaced item ids after the live by-id batch. A
        // non-empty but retired id otherwise remains held forever, even when
        // the replacement track already carries a recording id.
        for (id, payload) in &mut held_scrobble_candidates {
            if needs_metadata_lookup(&payload.jellyfin_id, resolved.as_ref()) {
                if let Some((jellyfin_id, mbid)) = resolve_by_metadata(
                    state,
                    &payload.artist,
                    payload.album.as_deref(),
                    &payload.track,
                    &payload.jellyfin_id,
                )
                .await
                {
                    state
                        .db
                        .patch_held_jellyfin_id(*id, &jellyfin_id)
                        .map_err(|e| {
                            format!("Failed to patch metadata-resolved scrobble id: {}", e)
                        })?;
                    payload.jellyfin_id = jellyfin_id;
                    if let Some(mbid) = mbid {
                        state
                            .db
                            .update_scrobble_payload_mbid(*id, &mbid)
                            .map_err(|e| {
                                format!("Failed to patch metadata-resolved scrobble mbid: {}", e)
                            })?;
                        payload.track_mbid = Some(mbid);
                    }
                }
            }
        }
        for (id, payload) in &mut held_skip_candidates {
            if needs_metadata_lookup(&payload.jellyfin_id, resolved.as_ref()) {
                if let Some((jellyfin_id, mbid)) = resolve_by_metadata(
                    state,
                    &payload.artist,
                    payload.album.as_deref(),
                    &payload.track,
                    &payload.jellyfin_id,
                )
                .await
                {
                    state
                        .db
                        .patch_held_jellyfin_id(*id, &jellyfin_id)
                        .map_err(|e| format!("Failed to patch metadata-resolved skip id: {}", e))?;
                    payload.jellyfin_id = jellyfin_id;
                    if let Some(mbid) = mbid {
                        state
                            .db
                            .update_scrobble_payload_mbid(*id, &mbid)
                            .map_err(|e| {
                                format!("Failed to patch metadata-resolved skip mbid: {}", e)
                            })?;
                        payload.track_mbid = Some(mbid);
                    }
                }
            }
        }

        let mut still_held_ids = Vec::new();

        for (id, payload) in held_scrobble_candidates {
            // Prefer an mbid the metadata pass above already found and
            // persisted (works even with Jellyfin unreachable); otherwise
            // fall back to this drain's live by-id batch result. A play still
            // without one is sent by artist and title rather than held: a live
            // recording has no MusicBrainz entry to wait for.
            let already_patched = payload.track_mbid.is_some();
            let mbid = payload.track_mbid.clone().or_else(|| {
                resolved
                    .as_ref()?
                    .get(&payload.jellyfin_id)
                    .cloned()
                    .flatten()
            });
            if mbid.is_none() && hold_for_lookup(lookup_settled, resolved.is_some()) {
                still_held_ids.push(id);
                continue;
            }
            if let Some(mbid) = &mbid {
                if !already_patched {
                    state
                        .db
                        .update_scrobble_payload_mbid(id, mbid)
                        .map_err(|e| format!("Failed to patch resolved scrobble mbid: {}", e))?;
                }
                tracing::info!(
                    "Resolved MusicBrainz recording ID for held scrobble '{}' - '{}'; scrobbling now",
                    payload.artist,
                    payload.track
                );
            } else {
                tracing::info!(
                    "No MusicBrainz recording ID for held scrobble '{}' - '{}'; scrobbling by artist and title",
                    payload.artist,
                    payload.track
                );
            }

            let listened_at = DateTime::from_timestamp(payload.listened_at, 0)
                .ok_or_else(|| "Invalid timestamp".to_string())?
                .to_rfc3339();

            scrobble_requests.push(ScrobbleRequest {
                track_mbid: mbid,
                artist_name: payload.artist,
                track_name: payload.track,
                album_name: payload.album,
                listened_at,
                duration_ms: payload.duration_ms,
                device_id: device_id.clone(),
                jellyfin_id: Some(payload.jellyfin_id),
                artist_mbid: payload.artist_mbid,
                release_mbid: payload.release_mbid,
            });
            scrobble_op_ids.push(id);
        }

        for (id, payload) in held_skip_candidates {
            let already_patched = payload.track_mbid.is_some();
            let mbid = payload.track_mbid.clone().or_else(|| {
                resolved
                    .as_ref()?
                    .get(&payload.jellyfin_id)
                    .cloned()
                    .flatten()
            });
            match mbid {
                Some(mbid) => {
                    if !already_patched {
                        state
                            .db
                            .update_scrobble_payload_mbid(id, &mbid)
                            .map_err(|e| format!("Failed to patch resolved skip mbid: {}", e))?;
                    }

                    tracing::info!(
                        "Resolved MusicBrainz recording ID for held skip '{}' - '{}'; sending now",
                        payload.artist,
                        payload.track
                    );

                    let skipped_at = DateTime::from_timestamp(payload.skipped_at, 0)
                        .ok_or_else(|| "Invalid timestamp".to_string())?
                        .to_rfc3339();

                    skip_requests.push(SkipRequest {
                        track_mbid: Some(mbid),
                        artist_name: payload.artist,
                        track_name: payload.track,
                        album_name: payload.album,
                        skipped_at,
                        played_ms: payload.played_ms,
                        track_duration_ms: payload.track_duration_ms,
                        device_id: device_id.clone(),
                        jellyfin_id: Some(payload.jellyfin_id),
                        artist_mbid: payload.artist_mbid,
                        release_mbid: payload.release_mbid,
                    });
                    skip_op_ids.push(id);
                }
                // The scrobble server's `/client/skips` refuses a skip with no recording
                // id for good, so once the lookup has answered there is
                // nothing to wait for. A skip is a statistic, not a play:
                // drop it rather than hold it in the pending count forever.
                None if lookup_settled || resolved.is_some() => {
                    tracing::warn!(
                        "Dropping skip '{}' - '{}': no MusicBrainz recording ID to record it under",
                        payload.artist,
                        payload.track
                    );
                    state
                        .db
                        .mark_synced(id)
                        .map_err(|e| format!("Failed to drop unrecordable skip: {}", e))?;
                }
                None => still_held_ids.push(id),
            }
        }

        if !still_held_ids.is_empty() {
            tracing::warn!(
                "{} scrobble/skip(s) still have no MusicBrainz recording ID after \
                 re-resolving against Jellyfin; holding until the lookup answers or the track is tagged",
                still_held_ids.len()
            );
            state
                .db
                .mark_held_no_mbid(&still_held_ids)
                .map_err(|e| format!("Failed to mark scrobbles/skips held: {}", e))?;
        }
    }

    // Send to service
    let (scrobble_outcome, skips_synced) = MusicServiceClient::sync_with_skips_with_client(
        &http_client,
        &base_url,
        &scrobble_requests,
        &skip_requests,
    )
    .await
    .map_err(|e| format!("Sync failed: {}", e))?;

    if scrobble_outcome.rejected_no_mbid > 0 {
        // Defensive net only: every scrobble reaching this call already
        // carries an mbid or goes by name, so this should never fire.
        tracing::warn!(
            "The scrobble server rejected {} scrobble(s) for missing MusicBrainz recording ID even \
             though Bragi holds mbid-less scrobbles back before sending — investigate",
            scrobble_outcome.rejected_no_mbid
        );
    }

    // Mark what the service accepted, even when a later chunk failed: the
    // outcome's positions index `scrobble_requests`, which `scrobble_op_ids`
    // mirrors. The server count is informational.
    let drained_set: std::collections::HashSet<usize> =
        scrobble_outcome.drained.iter().copied().collect();
    let drained: Vec<i64> = scrobble_outcome
        .drained
        .iter()
        .map(|&i| scrobble_op_ids[i])
        .collect();
    state
        .db
        .mark_synced_batch(&drained)
        .map_err(|e| format!("Failed to mark scrobbles as synced: {}", e))?;

    // Everything else still owed (refused plays, and any not reached) takes an
    // attempt, so backoff and the retry ceiling bound it.
    for (pos, id) in scrobble_op_ids.iter().enumerate() {
        if !drained_set.contains(&pos) {
            let _ = state.db.record_attempt(*id);
        }
    }

    // Skips drain in order and may stop early on a transient failure; the
    // count is the drained prefix.
    let skip_synced_count = (skips_synced as usize).min(skip_op_ids.len());
    if skip_synced_count > 0 {
        let synced_ids = &skip_op_ids[..skip_synced_count];
        state
            .db
            .mark_synced_batch(synced_ids)
            .map_err(|e| format!("Failed to mark skips as synced: {}", e))?;
    }
    for id in &skip_op_ids[skip_synced_count..] {
        let _ = state.db.record_attempt(*id);
    }

    if let Some(e) = scrobble_outcome.error {
        // The music service's auth is a header (`Authorization`), never a
        // query-string token, so `e`'s message carries no credential.
        telemetry::report_error_with_cause("scrobble queue failure", &e);
        return Err(format!("Sync failed: {}", e).into());
    }

    // Update last sync timestamp
    let now = crate::db::utils::current_timestamp();
    let _ = state
        .db
        .set_setting(keys::LAST_LIBRARY_SYNC, &now.to_string());

    let scrobbles_synced = scrobble_outcome.synced;
    tracing::info!(
        "Synced {} scrobbles and {} skips",
        scrobbles_synced,
        skips_synced
    );
    Ok(scrobbles_synced + skips_synced)
}

/// Get current sync status.
#[tauri::command]
pub async fn get_sync_status(state: tauri::State<'_, AppState>) -> CommandResult<SyncStatus> {
    let pending_count = state
        .db
        .get_pending_count()
        .map_err(|e| format!("Failed to get pending count: {}", e))?;

    // Extract client info without holding the lock during the HTTP request
    let maybe_client = {
        let client = state.music_service.read().await;
        client.as_ref().map(|service| service.clone_for_request())
    };

    let is_configured = maybe_client.is_some();

    // Check if we can reach the service (lock is released)
    let is_online = if let Some((http_client, base_url)) = maybe_client {
        MusicServiceClient::test_connection_with_client(&http_client, &base_url)
            .await
            .is_ok()
    } else {
        false
    };

    Ok(SyncStatus {
        is_configured,
        is_online,
        pending_count,
        last_sync_at: None,
        last_error: None,
    })
}

/// Get listening history from the music service.
#[tauri::command]
pub async fn get_listening_history(
    state: tauri::State<'_, AppState>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> CommandResult<Vec<ListenHistoryEntry>> {
    // Extract client info without holding the lock during the HTTP request
    let maybe_client = {
        let client = state.music_service.read().await;
        client.as_ref().map(|service| service.clone_for_request())
    };

    let (http_client, base_url) = match maybe_client {
        Some(client_info) => client_info,
        None => {
            return Err("Music service not configured".into());
        }
    };

    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);

    let response =
        MusicServiceClient::get_history_with_client(&http_client, &base_url, limit, offset)
            .await
            .map_err(|e| format!("Failed to get history: {}", e))?;

    Ok(response.items)
}

/// Connect to the music service.
#[tauri::command]
pub async fn connect_music_service(
    state: tauri::State<'_, AppState>,
    service_url: String,
    api_token: String,
) -> CommandResult<()> {
    let cid = audit::correlation_id();
    audit::log_connection_event("music_service", "connect_attempt", &service_url, &cid);

    // Create and test client
    let client = match MusicServiceClient::new(&service_url, &api_token) {
        Ok(c) => c,
        Err(e) => {
            audit::log_auth_event(
                "music_service",
                "connect",
                false,
                audit::error_kind(&e),
                &cid,
            );
            return Err(format!("Failed to create client: {}", e).into());
        }
    };

    if let Err(e) = client.test_connection().await {
        audit::log_auth_event(
            "music_service",
            "connection_test",
            false,
            audit::error_kind(&e),
            &cid,
        );
        return Err(format!("Connection test failed: {}", e).into());
    }

    // Store credentials
    let credentials = MusicServiceCredentials {
        service_url,
        api_token,
    };
    match store_music_service_credentials(&credentials) {
        Ok(()) => audit::log_credential_event("music_service", "store", true, &cid),
        Err(e) => {
            audit::log_credential_event("music_service", "store", false, &cid);
            return Err(format!("Failed to store credentials: {}", e).into());
        }
    }

    // Update state
    let mut service_state = state.music_service.write().await;
    *service_state = Some(client);

    audit::log_auth_event("music_service", "connect", true, "connected", &cid);
    Ok(())
}

/// Disconnect from the music service.
#[tauri::command]
pub async fn disconnect_music_service(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    let cid = audit::correlation_id();
    audit::log_connection_event("music_service", "disconnect", "", &cid);

    // Clear credentials
    match clear_music_service_credentials() {
        Ok(()) => audit::log_credential_event("music_service", "clear", true, &cid),
        Err(e) => {
            audit::log_credential_event("music_service", "clear", false, &cid);
            return Err(format!("Failed to clear credentials: {}", e).into());
        }
    }

    // Clear state
    let mut service_state = state.music_service.write().await;
    *service_state = None;

    audit::log_auth_event("music_service", "disconnect", true, "disconnected", &cid);
    Ok(())
}

/// Get music service connection status.
#[tauri::command]
pub async fn get_music_service_status(
    state: tauri::State<'_, AppState>,
) -> CommandResult<MusicServiceStatus> {
    let client = state.music_service.read().await;

    match *client {
        Some(ref service) => Ok(MusicServiceStatus {
            connected: true,
            service_url: Some(service.base_url().to_string()),
        }),
        None => Ok(MusicServiceStatus {
            connected: false,
            service_url: None,
        }),
    }
}

/// Test connection to the music service.
#[tauri::command]
pub async fn test_music_service_connection(
    state: tauri::State<'_, AppState>,
) -> CommandResult<bool> {
    // Extract client info without holding the lock during the HTTP request
    let maybe_client = {
        let client = state.music_service.read().await;
        client.as_ref().map(|service| service.clone_for_request())
    };

    match maybe_client {
        Some((http_client, base_url)) => {
            match MusicServiceClient::test_connection_with_client(&http_client, &base_url).await {
                Ok(()) => Ok(true),
                Err(e) => {
                    tracing::warn!("Music service connection test failed: {}", e);
                    Ok(false)
                }
            }
        }
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Database, QueuedOperation};

    #[test]
    fn retired_nonempty_item_id_is_repaired_from_current_library_metadata() {
        let db = Database::open_in_memory().unwrap();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO cached_albums (jellyfin_id, name, last_synced_at) VALUES ('album', 'Hard-Off', 1)",
                [],
            )?;
            conn.execute(
                "INSERT INTO cached_tracks (jellyfin_id, name, artist_name, album_jellyfin_id, track_mbid, last_synced_at) VALUES ('replacement', 'Dimes', 'Bloodhound Gang', 'album', 'ee959f12-e731-43c0-8a6f-5859c7238587', 1)",
                [],
            )?;
            Ok(())
        }).unwrap();

        let live = std::collections::HashMap::from([("replacement".to_string(), None)]);
        assert!(needs_metadata_lookup(
            "dd636f43347adac39aa6063b0ffae822",
            Some(&live)
        ));
        assert!(needs_metadata_lookup("", Some(&live)));
        assert!(!needs_metadata_lookup("replacement", Some(&live)));
        assert!(!needs_metadata_lookup("replacement", None));
        let replacement = db
            .find_track_by_metadata("Bloodhound Gang", Some("Hard-Off"), "Dimes")
            .unwrap()
            .unwrap();
        assert_eq!(replacement.jellyfin_id, "replacement");
        assert_eq!(
            replacement.mbid.as_deref(),
            Some("ee959f12-e731-43c0-8a6f-5859c7238587")
        );
        assert!(db
            .find_track_by_metadata("Bloodhound Gang", Some("Hard-Off"), "Different song")
            .unwrap()
            .is_none());
    }

    #[test]
    fn stale_cached_id_and_ambiguous_recordings_are_not_trusted() {
        let db = Database::open_in_memory().unwrap();
        db.with_conn(|conn| {
            conn.execute("INSERT INTO cached_albums (jellyfin_id, name, last_synced_at) VALUES ('studio', 'Studio', 1), ('live', 'Live', 1)", [])?;
            conn.execute("INSERT INTO cached_tracks (jellyfin_id, name, artist_name, album_jellyfin_id, track_mbid, last_synced_at) VALUES ('old', 'Song', 'Artist', 'studio', 'studio-mbid', 1), ('live', 'Song', 'Artist', 'live', 'live-mbid', 1)", [])?;
            Ok(())
        }).unwrap();
        assert!(db.get_track("old").unwrap().is_some());
        assert!(needs_metadata_lookup(
            "old",
            Some(&std::collections::HashMap::new())
        ));
        assert!(!needs_metadata_lookup("old", None));
        assert!(db
            .find_track_by_metadata("Artist", None, "Song")
            .unwrap()
            .is_none());
        let studio = db.get_track("old").unwrap().unwrap();
        let live = db.get_track("live").unwrap().unwrap();
        assert!(unique_metadata_match(
            vec![studio.clone(), live.clone()],
            "Artist",
            None,
            "Song",
            "retired"
        )
        .is_none());
        assert_eq!(
            unique_metadata_match(
                vec![studio, live],
                "Artist",
                Some("Live"),
                "Song",
                "retired"
            ),
            Some(("live".to_string(), Some("live-mbid".to_string())))
        );
    }

    fn held_op(id: i64, operation: SyncOperation, last_attempt_at: Option<i64>) -> QueuedOperation {
        QueuedOperation {
            id,
            operation,
            payload: "{}".to_string(),
            created_at: 1_000,
            attempts: 0,
            last_attempt_at,
            synced_at: None,
            held_no_mbid: true,
        }
    }

    fn normal_op(id: i64, operation: SyncOperation, attempts: u32) -> QueuedOperation {
        QueuedOperation {
            id,
            operation,
            payload: "{}".to_string(),
            created_at: 1_000,
            attempts,
            last_attempt_at: if attempts == 0 { None } else { Some(1_000) },
            synced_at: None,
            held_no_mbid: false,
        }
    }

    /// A brand-new held SKIP (no `last_attempt_at` yet) must always be ready,
    /// exactly like a brand-new held scrobble — the immediate resolve
    /// attempt in the same drain that discovered it, not just scrobbles.
    #[test]
    fn held_skip_with_no_last_attempt_is_always_ready() {
        let ops = vec![held_op(1, SyncOperation::Skip, None)];
        let ready = filter_ready_operations(ops, 1_000_000);
        assert_eq!(ready.len(), 1, "a brand-new held skip must be ready");
    }

    /// An already-held skip within the cooldown window must NOT be
    /// re-surfaced (avoids hammering Jellyfin every drain); one past the
    /// cooldown must be — identical behaviour to a held scrobble.
    #[test]
    fn held_skip_respects_the_resolve_cooldown_not_backoff() {
        let now = 1_000_000_i64;
        let within_cooldown = held_op(1, SyncOperation::Skip, Some(now - 60));
        let past_cooldown = held_op(
            2,
            SyncOperation::Skip,
            Some(now - HELD_RESOLVE_COOLDOWN_SECONDS - 1),
        );

        let ready = filter_ready_operations(vec![within_cooldown, past_cooldown], now);

        assert_eq!(
            ready.len(),
            1,
            "only the skip past its cooldown must be ready"
        );
        assert_eq!(ready[0].id, 2);
    }

    /// A held skip is exempt from `MAX_RETRY_ATTEMPTS`-based eviction even
    /// though `attempts` stays 0 forever on held rows — the exemption itself
    /// is what's under test, mirroring the scrobble guarantee.
    #[test]
    fn held_skip_never_evicted_by_backoff_gate() {
        let mut op = held_op(1, SyncOperation::Skip, None);
        op.attempts = MAX_RETRY_ATTEMPTS; // must never happen in practice, but prove the gate doesn't care
        let ready = filter_ready_operations(vec![op], 1_000_000);
        assert_eq!(
            ready.len(),
            1,
            "held rows must bypass the attempts gate entirely"
        );
    }

    /// A normal (non-held) skip still goes through ordinary backoff, same as
    /// a normal scrobble — the held exemption must not leak onto real ops.
    #[test]
    fn normal_skip_still_honours_backoff() {
        let maxed_out = normal_op(1, SyncOperation::Skip, MAX_RETRY_ATTEMPTS);
        let ready = filter_ready_operations(vec![maxed_out], 1_000_000);
        assert!(
            ready.is_empty(),
            "a normal skip at MAX_RETRY_ATTEMPTS must still be dropped"
        );
    }

    /// The hold-then-send path: with the Jellyfin lookup unanswered (or no
    /// server) a play with no id is held; once the lookup answers, or the
    /// server has no ids to give, it is sent by name.
    #[test]
    fn a_play_without_an_id_is_held_until_the_lookup_settles() {
        use crate::mediaserver::{subsonic::SubsonicClient, MediaClient};

        assert!(!lookup_settled(None), "no server connected settles nothing");
        let subsonic =
            MediaClient::Subsonic(SubsonicClient::new("http://127.0.0.1:1", "u", "p").unwrap());
        assert!(
            lookup_settled(Some(&subsonic)),
            "Subsonic has no ids to give"
        );

        assert!(hold_for_lookup(false, false), "unanswered lookup: held");
        assert!(
            !hold_for_lookup(false, true),
            "answered lookup: sent by name"
        );
        assert!(
            !hold_for_lookup(true, false),
            "settled server: sent by name"
        );
    }
}
