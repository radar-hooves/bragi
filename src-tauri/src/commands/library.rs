//! Library-related Tauri commands.
//!
//! Handles library browsing, searching, and metadata retrieval.
//! Fetches from Jellyfin and caches to local SQLite database.

use crate::error::{CommandError, CommandResult};
use crate::models::{Album, Artist, SearchResults, Track};
use crate::state::AppState;
use serde::Serialize;

/// Metrics collected during a library sync operation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncMetrics {
    /// Total sync duration in milliseconds.
    pub duration_ms: u64,
    /// Time spent fetching from Jellyfin API (ms).
    pub fetch_time_ms: u64,
    /// Time spent writing to SQLite cache (ms).
    pub db_time_ms: u64,
    /// Number of artists synced.
    pub artist_count: u32,
    /// Number of albums synced.
    pub album_count: u32,
    /// Number of tracks synced.
    pub track_count: u32,
    /// Number of Jellyfin API requests made.
    pub api_requests: u32,
    /// Number of stale artists removed.
    pub stale_artists: u32,
    /// Number of stale albums removed.
    pub stale_albums: u32,
    /// Number of stale tracks removed.
    pub stale_tracks: u32,
    /// Number of album track fetches that failed.
    pub failed_fetches: u32,
    /// True when this trigger was coalesced by the single-flight guard because a
    /// sync was already in progress — a benign no-op, not an error.
    #[serde(default)]
    pub skipped: bool,
}

impl SyncMetrics {
    /// A zeroed result for a sync that was coalesced away by the single-flight
    /// guard (another sync was already running). The UI treats this as a no-op.
    pub fn skipped() -> Self {
        Self {
            duration_ms: 0,
            fetch_time_ms: 0,
            db_time_ms: 0,
            artist_count: 0,
            album_count: 0,
            track_count: 0,
            api_requests: 0,
            stale_artists: 0,
            stale_albums: 0,
            stale_tracks: 0,
            failed_fetches: 0,
            skipped: true,
        }
    }
}

/// Reject an item ID that is not a safe opaque media id.
fn validate_jellyfin_id(id: &str) -> Result<(), String> {
    if crate::downloads::validation::is_valid_item_id(id) {
        Ok(())
    } else {
        Err("Invalid item ID".to_string())
    }
}

/// Enrich albums with download counts from the local SQLite cache.
///
/// Jellyfin has no knowledge of locally cached files, so we overlay
/// the download counts from our database onto the Jellyfin response.
fn enrich_albums_with_download_counts(state: &AppState, albums: &mut [Album]) {
    if let Ok(counts) = state.db.get_album_download_counts() {
        for album in albums.iter_mut() {
            if let Some(&count) = counts.get(&album.jellyfin_id) {
                album.downloaded_track_count = count;
            }
        }
    }
}

/// Get all albums in the library.
#[tauri::command]
pub async fn get_library_albums(state: tauri::State<'_, AppState>) -> CommandResult<Vec<Album>> {
    get_library_albums_inner(&state).await
}

async fn get_library_albums_inner(state: &AppState) -> CommandResult<Vec<Album>> {
    tracing::debug!("Fetching all albums from Jellyfin");

    // Clone the client and drop the lock before the network await
    // (release-before-await): a slow Jellyfin response must never hold up a
    // reconnect or backend switch that needs the write lock.
    let client = state.get_media().await?;

    let mut albums = client.get_albums().await?;
    enrich_albums_with_download_counts(state, &mut albums);
    // Rewrite the OpenSubsonic cover sentinel to a live /cover proxy URL so the
    // frontend never receives a credential-free-but-unfetchable reference; a
    // Jellyfin URL passes through unchanged.
    state.resolve_albums_artwork(&mut albums).await;
    Ok(albums)
}

/// Get all artists in the library.
#[tauri::command]
pub async fn get_library_artists(state: tauri::State<'_, AppState>) -> CommandResult<Vec<Artist>> {
    tracing::debug!("Fetching all artists from Jellyfin");

    let client = state.get_media().await?;

    let mut artists = client.get_artists().await?;
    state.resolve_artists_artwork(&mut artists).await;
    Ok(artists)
}

/// Get tracks for a specific album.
#[tauri::command]
pub async fn get_album_tracks(
    state: tauri::State<'_, AppState>,
    album_id: String,
) -> CommandResult<Vec<Track>> {
    tauri_plugin_telemetry::traced_with_span(
        tracing::info_span!(
            target: tauri_plugin_telemetry::COMMAND_SPAN_TARGET,
            "ipc_command",
            command = "get_album_tracks",
            outcome = tracing::field::Empty,
            %album_id,
        ),
        async move {
            validate_jellyfin_id(&album_id)?;
            tracing::debug!("Fetching tracks for album: {}", album_id);

            let client = state.get_media().await?;

            let mut tracks = client.get_album_tracks(&album_id).await?;
            state.resolve_tracks_artwork(&mut tracks).await;
            Ok(tracks)
        },
    )
    .await
}

/// Get built-in lyrics for a track.
///
/// Returns `Ok(None)` when the track has no lyrics (or the Jellyfin server
/// predates the lyrics endpoint) rather than an error, so the UI can show a
/// clean "no lyrics" state instead of an error toast.
#[tauri::command]
pub async fn get_track_lyrics(
    state: tauri::State<'_, AppState>,
    track_id: String,
) -> CommandResult<Option<crate::models::TrackLyrics>> {
    validate_jellyfin_id(&track_id)?;
    tracing::debug!("Fetching lyrics for track: {}", track_id);

    // Clone the client and drop the lock BEFORE the network call. Lyrics are a
    // cosmetic, best-effort fetch — they must never hold the Jellyfin guard
    // across an await (release-before-await rule) and must never sit anywhere
    // near the playback path.
    let client = state.get_media().await?;

    client
        .get_lyrics(&track_id)
        .await
        .map_err(CommandError::msg)
}

/// Max size of the audio source fetched purely to compute a waveform.
///
/// ~65 min at 128 kbps. A track larger than this (a long DJ mix or audiobook)
/// simply gets no waveform rather than pulling hundreds of MB into memory for a
/// cosmetic feature.
const MAX_WAVEFORM_SOURCE_BYTES: usize = 60 * 1024 * 1024;

/// Get precomputed waveform peaks for a track.
///
/// Returns a normalised (0.0–1.0) peak array for drawing a seekable waveform in
/// the now-playing view. Distinguishes two "no peaks" outcomes:
/// - `Ok(None)` — decoding actually ran and produced nothing usable (an
///   undecodable stream, or a source over the size cap). The UI treats this as
///   a permanent "no waveform" and won't refetch.
/// - `Err(..)` — a *transient* failure (offline / not connected, a network or
///   auth error). The frontend leaves it uncached so it retries once a server
///   is reachable again, instead of remembering "no waveform" for the session.
///
/// Cosmetic and fully decoupled from playback: peaks are cached to disk on
/// first compute, decoding runs on a blocking thread, and the Jellyfin guard is
/// released before any network/CPU work (release-before-await).
#[tauri::command]
pub async fn get_track_waveform(
    state: tauri::State<'_, AppState>,
    track_id: String,
) -> CommandResult<Option<Vec<f32>>> {
    validate_jellyfin_id(&track_id)?;
    let cache_dir = state.waveform_cache_dir();

    // Cache hit — return without touching the network or a decoder.
    if let Some(peaks) = crate::waveform::read_cache(&cache_dir, &track_id).await {
        return Ok(Some(peaks));
    }

    let mut peaks: Option<Vec<f32>> = None;

    // Prefer a locally downloaded file: no network, works offline. Decoded via a
    // streaming `File` source so a large lossless file isn't buffered into RAM.
    // (Skipped for codecs symphonia can't read, e.g. Opus — those fall through
    // to a transcoded MP3 below.)
    let local_path = state
        .db
        .get_track(&track_id)?
        .and_then(|t| t.cached_file_path);
    if let Some(path) = local_path {
        let ext = std::path::Path::new(&path)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_owned);
        let path_for_task = path.clone();
        match tokio::task::spawn_blocking(move || {
            let file =
                std::fs::File::open(&path_for_task).map_err(|e| format!("open failed: {e}"))?;
            crate::waveform::compute_peaks(
                Box::new(file),
                ext.as_deref(),
                crate::waveform::NUM_BUCKETS,
            )
        })
        .await
        {
            Ok(Ok(p)) => peaks = Some(p),
            Ok(Err(e)) => tracing::debug!("Local waveform decode failed for {path}: {e}"),
            Err(e) => tracing::debug!("Waveform decode task failed: {e}"),
        }
    }

    // Fall back to a small server-transcoded MP3 (uniform codec, tiny download).
    if peaks.is_none() {
        // Clone the client and drop the guard before the network + decode work.
        let client = {
            let guard = state.media.read().await;
            match guard.as_ref() {
                Some(c) => c.clone(),
                // Offline / not connected is transient: surface as Err so the
                // frontend retries later rather than caching "no waveform".
                None => {
                    return Err("Not connected — waveform unavailable offline"
                        .to_string()
                        .into());
                }
            }
        };
        let url = client.get_transcoded_stream_url(&track_id, "mp3", "mp3", Some(128));
        // A network/auth failure is transient → Err (uncached, retried). A body
        // over the cap yields Ok(None) → treated as "no waveform" below.
        match client
            .download_bytes_capped(&url, MAX_WAVEFORM_SOURCE_BYTES)
            .await?
        {
            Some(bytes) => {
                match tokio::task::spawn_blocking(move || {
                    crate::waveform::compute_peaks(
                        Box::new(std::io::Cursor::new(bytes)),
                        Some("mp3"),
                        crate::waveform::NUM_BUCKETS,
                    )
                })
                .await
                {
                    Ok(Ok(p)) => peaks = Some(p),
                    Ok(Err(e)) => tracing::debug!("Transcoded waveform decode failed: {e}"),
                    Err(e) => tracing::debug!("Waveform decode task failed: {e}"),
                }
            }
            None => tracing::debug!("Waveform source over size cap for {track_id}; skipping"),
        }
    }

    if let Some(ref p) = peaks {
        crate::waveform::write_cache(&cache_dir, &track_id, p).await;
    }
    Ok(peaks)
}

/// Get albums for a specific artist.
#[tauri::command]
pub async fn get_artist_albums(
    state: tauri::State<'_, AppState>,
    artist_id: String,
) -> CommandResult<Vec<Album>> {
    tauri_plugin_telemetry::traced_with_span(
        tracing::info_span!(
            target: tauri_plugin_telemetry::COMMAND_SPAN_TARGET,
            "ipc_command",
            command = "get_artist_albums",
            outcome = tracing::field::Empty,
            %artist_id,
        ),
        async move {
            validate_jellyfin_id(&artist_id)?;
            tracing::debug!("Fetching albums for artist: {}", artist_id);

            let client = state.get_media().await?;

            let mut albums = client.get_artist_albums(&artist_id).await?;
            enrich_albums_with_download_counts(&state, &mut albums);
            state.resolve_albums_artwork(&mut albums).await;
            Ok(albums)
        },
    )
    .await
}

/// Search the library for tracks, albums, and artists.
#[tauri::command]
pub async fn search_library(
    state: tauri::State<'_, AppState>,
    query: String,
) -> CommandResult<SearchResults> {
    tauri_plugin_telemetry::traced_with_span(
        tracing::info_span!(
            target: tauri_plugin_telemetry::COMMAND_SPAN_TARGET,
            "ipc_command",
            command = "search_library",
            outcome = tracing::field::Empty,
            %query,
        ),
        async move {
            tracing::debug!("Searching library for: {}", query);

            let client = state.get_media().await?;

            let mut results = client.search(&query).await?;
            enrich_albums_with_download_counts(&state, &mut results.albums);
            state.resolve_albums_artwork(&mut results.albums).await;
            state.resolve_artists_artwork(&mut results.artists).await;
            state.resolve_tracks_artwork(&mut results.tracks).await;
            Ok(results)
        },
    )
    .await
}

/// Sync library from Jellyfin server to local SQLite cache.
///
/// Fetches all albums, artists, and tracks from Jellyfin and
/// stores them in the local database for offline access.
///
/// Returns metrics about the sync operation including timing and counts.
///
/// # Implementation Notes
///
/// - Uses a single sync timestamp for all upserts to ensure consistent
///   stale entry detection (fixes race condition)
/// - Processes albums in batches to reduce memory usage for large libraries
/// - Deletes tracks before albums to respect foreign key constraints
#[tauri::command]
pub async fn sync_library(state: tauri::State<'_, AppState>) -> CommandResult<SyncMetrics> {
    tauri_plugin_telemetry::traced("sync_library", async move {
    tracing::info!("Starting library sync from Jellyfin...");

    let metrics = crate::services::perform_sync_with_metrics(&state).await?;

    tracing::info!(
        "Library sync complete in {}ms (fetch: {}ms, db: {}ms): {} artists, {} albums, {} tracks, {} API requests",
        metrics.duration_ms,
        metrics.fetch_time_ms,
        metrics.db_time_ms,
        metrics.artist_count,
        metrics.album_count,
        metrics.track_count,
        metrics.api_requests,
    );

    Ok(metrics)
}).await
}

/// Get the stream URL for a track via the local proxy.
#[tauri::command]
pub async fn get_stream_url(
    state: tauri::State<'_, AppState>,
    track_id: String,
) -> CommandResult<String> {
    validate_jellyfin_id(&track_id)?;

    let (proxy_port, proxy_token) = state.proxy_endpoint().await?;
    let client = state.get_media().await?;

    Ok(client.get_stream_url(&track_id, proxy_port, &proxy_token))
}

/// Get the artwork URL for an item.
#[tauri::command]
pub async fn get_artwork_url(
    state: tauri::State<'_, AppState>,
    item_id: String,
    size: Option<u32>,
) -> CommandResult<String> {
    validate_jellyfin_id(&item_id)?;

    // OpenSubsonic returns a credential-free `/cover` proxy URL (the proxy injects
    // the salted token server-side); Jellyfin returns its unauthenticated
    // Primary-image URL. Fetch the port before taking the media guard.
    let proxy_port = state.proxy_port().await?;
    let client = state.get_media().await?;

    Ok(client.cover_proxy_url(&item_id, size.unwrap_or(300), proxy_port))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    use super::*;

    #[tokio::test]
    async fn slow_album_request_releases_media_lock() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (request_started, request_started_rx) = oneshot::channel();
        let (reply, reply_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            request_started.send(()).unwrap();
            reply_rx.await.unwrap();
            let body = r#"{"Items":[],"TotalRecordCount":0}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        let state = AppState::new_without_audio(
            crate::db::Database::open_in_memory().unwrap(),
            std::env::temp_dir(),
        );
        let client = crate::jellyfin::JellyfinClient::new_with_device_id(
            &format!("http://{address}"),
            "test-key",
            "test-user",
            "test-device",
        )
        .unwrap();
        *state.media.write().await = Some(crate::mediaserver::MediaClient::Jellyfin(client));

        let (_, ()) = tokio::join!(get_library_albums_inner(&state), async {
            request_started_rx.await.unwrap();
            let writer = tokio::time::timeout(Duration::from_millis(100), state.media.write())
                .await
                .expect("a slow request must not hold the media lock");
            drop(writer);
            reply.send(()).unwrap();
        });
        server.await.unwrap();
    }
}
