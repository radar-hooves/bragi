//! Download-related Tauri commands.
//!
//! Commands for managing offline downloads and cache.

use crate::db::{CachedAlbum, CachedTrack};
use crate::downloads::validation::validate_jellyfin_id;
use crate::downloads::{CacheSettings, DownloadRequest, DownloadState, QueuedDownload};
use crate::error::{CommandError, CommandResult};
use crate::models::Track;
use crate::state::AppState;

/// Queue a single track for download.
#[tauri::command]
pub async fn download_track(
    state: tauri::State<'_, AppState>,
    track_id: String,
    track_name: String,
    artist_name: String,
) -> CommandResult<()> {
    tauri_plugin_telemetry::traced("download_track", async move {
        tracing::info!("download_track called: {} - {}", artist_name, track_name);

        // Validate track ID before processing
        validate_jellyfin_id(&track_id, "track").map_err(|e| {
            let msg = format!("Invalid track ID '{}': {}", track_id, e);
            tracing::error!("{}", msg);
            msg
        })?;

        tracing::info!(
            "Queueing track for download: {} - {}",
            artist_name,
            track_name
        );

        let manager = state.get_download_manager().await?;

        manager
            .queue_track(DownloadRequest {
                track_id,
                track_name,
                artist_name,
                album_id: None,
            })
            .await
            .map_err(|e| CommandError::from(format!("Failed to queue download: {}", e)))
    })
    .await
}

/// Queue all tracks from an album for download.
#[tauri::command]
pub async fn download_album(
    state: tauri::State<'_, AppState>,
    album_id: String,
) -> CommandResult<u32> {
    tauri_plugin_telemetry::traced("download_album", async move {
        tracing::info!("download_album called with album_id={}", album_id);

        // Validate album ID before processing
        validate_jellyfin_id(&album_id, "album").map_err(|e| {
            let msg = format!("Invalid album ID '{}': {}", album_id, e);
            tracing::error!("{}", msg);
            msg
        })?;

        tracing::info!("Queueing album for download: {}", album_id);

        // Get tracks for this album — try local cache first, fall back to Jellyfin API.
        let mut tracks = state.db.get_tracks_by_album(&album_id).unwrap_or_default();

        if tracks.is_empty() {
            tracing::info!(
                "No cached tracks for album {}, fetching from Jellyfin",
                album_id
            );
            tracks = fetch_and_cache_album_tracks(&state, &album_id).await?;
        }

        if tracks.is_empty() {
            return Err("No tracks found for album".into());
        }

        let track_count = tracks.len() as u32;

        // Filter out already downloaded tracks
        let tracks_to_download: Vec<DownloadRequest> = tracks
            .into_iter()
            .filter(|t| t.cached_file_path.is_none())
            .map(|t| DownloadRequest {
                track_id: t.jellyfin_id,
                track_name: t.name,
                artist_name: t.artist_name,
                album_id: Some(album_id.clone()),
            })
            .collect();

        if tracks_to_download.is_empty() {
            tracing::info!("All tracks already downloaded for album {}", album_id);
            return Ok(0);
        }

        let download_count = tracks_to_download.len() as u32;

        let download_manager = state.download_manager.read().await;
        let manager = download_manager.as_ref().ok_or_else(|| {
            tracing::error!("download_album: Download manager not initialised");
            "Download manager not initialised".to_string()
        })?;

        manager
            .queue_album(album_id.clone(), tracks_to_download)
            .await
            .map_err(|e| {
                let msg = format!("Failed to queue album download: {}", e);
                tracing::error!("download_album: {}", msg);
                msg
            })?;

        tracing::info!(
            "Queued {} tracks for download (total: {})",
            download_count,
            track_count
        );

        Ok(download_count)
    })
    .await
}

/// Fetch an album's tracks from Jellyfin (when not already cached locally) and
/// persist them to the DB so `set_track_cached_path` can find them once the
/// download completes.
///
/// Extracted from `download_album` so the release-before-await regression
/// test can exercise the fetch without needing a running download manager,
/// which the rest of `download_album` depends on.
async fn fetch_and_cache_album_tracks(
    state: &AppState,
    album_id: &str,
) -> CommandResult<Vec<Track>> {
    // Clone the client and drop the lock before the network await
    // (release-before-await): a slow Jellyfin response must never hold up
    // a reconnect or backend switch that needs the write lock.
    let client = state
        .get_media()
        .await
        .inspect_err(|e| tracing::error!("download_album: {e}"))?;
    // Stamp cached rows with the backend this download is for, not the
    // live active source — so a mid-download backend toggle can't mislabel them.
    let source = client.kind();
    let tracks = client.get_album_tracks(album_id).await.map_err(|e| {
        let msg = format!("Failed to fetch album tracks: {}", e);
        tracing::error!("download_album: {}", msg);
        msg
    })?;

    if tracks.is_empty() {
        return Ok(tracks);
    }

    // Ensure the album row exists (foreign key constraint).
    let album_name = tracks[0].album_name.clone();
    let artist_name_for_album = tracks[0].artist_name.clone();
    let _ = state.db.upsert_album(
        &CachedAlbum {
            jellyfin_id: album_id.to_string(),
            release_mbid: tracks[0].release_mbid.clone(),
            name: album_name,
            artist_name: Some(artist_name_for_album),
            artist_jellyfin_id: None,
            year: tracks[0].year,
            genre: tracks[0].genre.clone(),
            artwork_url: tracks[0].artwork_url.clone(),
            track_count: Some(tracks.len() as u32),
            duration_ms: Some(tracks.iter().map(|t| t.duration_ms).sum()),
            date_created: None,
        },
        source,
        None,
    );

    let cached: Vec<CachedTrack> = tracks
        .iter()
        .map(|t| CachedTrack::from_track(t, None))
        .collect();

    if let Err(e) = state.db.upsert_tracks(&cached, source, None) {
        tracing::warn!("Failed to cache tracks in DB: {}", e);
    }

    Ok(tracks)
}

/// Cancel a pending or active download.
#[tauri::command]
pub async fn cancel_download(
    state: tauri::State<'_, AppState>,
    track_id: String,
) -> CommandResult<()> {
    // Validate track ID before processing
    validate_jellyfin_id(&track_id, "track").map_err(|e| format!("Invalid track ID: {}", e))?;

    tracing::info!("Cancelling download: {}", track_id);

    let manager = state.get_download_manager().await?;

    manager
        .cancel(track_id)
        .await
        .map_err(|e| CommandError::from(format!("Failed to cancel download: {}", e)))
}

/// Pause all downloads.
#[tauri::command]
pub async fn pause_downloads(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    tracing::info!("Pausing downloads");

    let manager = state.get_download_manager().await?;

    manager
        .pause()
        .await
        .map_err(|e| CommandError::from(format!("Failed to pause downloads: {}", e)))
}

/// Resume paused downloads.
#[tauri::command]
pub async fn resume_downloads(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    tracing::info!("Resuming downloads");

    let manager = state.get_download_manager().await?;

    manager
        .resume()
        .await
        .map_err(|e| CommandError::from(format!("Failed to resume downloads: {}", e)))
}

/// Get the current download queue.
#[tauri::command]
pub async fn get_download_queue(
    state: tauri::State<'_, AppState>,
) -> CommandResult<Vec<QueuedDownload>> {
    let manager = state.get_download_manager().await?;

    manager
        .get_queue()
        .await
        .map_err(|e| CommandError::from(format!("Failed to get download queue: {}", e)))
}

/// Get the current download state.
#[tauri::command]
pub async fn get_download_state(state: tauri::State<'_, AppState>) -> CommandResult<DownloadState> {
    let manager = state.get_download_manager().await?;

    manager
        .get_state()
        .await
        .map_err(|e| CommandError::from(format!("Failed to get download state: {}", e)))
}

/// Clear completed and failed downloads from the queue.
#[tauri::command]
pub async fn clear_completed_downloads(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    tracing::info!("Clearing completed downloads");

    let manager = state.get_download_manager().await?;

    manager
        .clear_completed()
        .await
        .map_err(|e| CommandError::from(format!("Failed to clear completed downloads: {}", e)))
}

/// Get cache settings.
#[tauri::command]
pub async fn get_cache_settings(state: tauri::State<'_, AppState>) -> CommandResult<CacheSettings> {
    let current_size = state
        .db
        .get_cached_files_size()
        .map_err(|e| format!("Failed to get cache size: {}", e))?;

    let cached_tracks = state
        .db
        .get_cached_tracks()
        .map_err(|e| format!("Failed to get cached tracks: {}", e))?;

    // Get cache limit from settings
    let max_size = state
        .db
        .get_setting(crate::db::keys::CACHE_SIZE_LIMIT)
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(10 * 1024 * 1024 * 1024); // Default 10GB

    Ok(CacheSettings {
        max_size_bytes: max_size,
        current_size_bytes: current_size,
        cached_track_count: cached_tracks.len() as u32,
    })
}

/// Update cache settings.
#[tauri::command]
pub async fn set_cache_settings(
    state: tauri::State<'_, AppState>,
    max_size_bytes: u64,
) -> CommandResult<()> {
    tracing::info!("Setting cache size limit to {} bytes", max_size_bytes);

    // Save to database
    state
        .db
        .set_setting(
            crate::db::keys::CACHE_SIZE_LIMIT,
            &max_size_bytes.to_string(),
        )
        .map_err(|e| format!("Failed to save cache setting: {}", e))?;

    // Update download manager
    let download_manager = state.download_manager.read().await;
    if let Some(manager) = download_manager.as_ref() {
        manager
            .set_cache_limit(max_size_bytes)
            .await
            .map_err(|e| format!("Failed to update download manager: {}", e))?;
    }

    Ok(())
}

/// Clear all cached files.
#[tauri::command]
pub async fn clear_cache(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    tracing::info!("Clearing all cached files");

    let manager = state.get_download_manager().await?;

    manager
        .clear_cache()
        .await
        .map_err(|e| CommandError::from(format!("Failed to clear cache: {}", e)))
}

/// Get all downloaded tracks.
#[tauri::command]
pub async fn get_downloaded_tracks(state: tauri::State<'_, AppState>) -> CommandResult<Vec<Track>> {
    state
        .db
        .get_cached_tracks()
        .map_err(|e| CommandError::from(format!("Failed to get downloaded tracks: {}", e)))
}

/// Remove a downloaded track from cache.
#[tauri::command]
pub async fn remove_downloaded_track(
    state: tauri::State<'_, AppState>,
    track_id: String,
) -> CommandResult<()> {
    // Validate track ID before processing
    validate_jellyfin_id(&track_id, "track").map_err(|e| format!("Invalid track ID: {}", e))?;

    tracing::info!("Removing downloaded track: {}", track_id);

    // Get the cached file path
    let track = state
        .db
        .get_track(&track_id)
        .map_err(|e| format!("Failed to get track: {}", e))?
        .ok_or_else(|| "Track not found".to_string())?;

    if let Some(file_path) = track.cached_file_path {
        // Delete the file - handle NotFound gracefully (TOCTOU safe)
        let path = std::path::PathBuf::from(&file_path);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {
                tracing::debug!("Deleted cached file: {}", file_path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // File already gone, that's fine
                tracing::debug!("Cached file already deleted: {}", file_path);
            }
            Err(e) => {
                return Err(format!("Failed to delete cached file: {}", e).into());
            }
        }
    }

    // Clear from database
    state
        .db
        .clear_track_cached_path(&track_id)
        .map_err(|e| format!("Failed to clear track cache path: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    use super::*;

    /// A ninth instance of the release-before-await bug 5668f49 fixed, on the
    /// offline-download path (found in review): mirrors
    /// `commands::library::tests::slow_album_request_releases_media_lock`.
    #[tokio::test]
    async fn slow_download_album_fetch_releases_media_lock() {
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

        let (result, ()) = tokio::join!(fetch_and_cache_album_tracks(&state, "album-1"), async {
            request_started_rx.await.unwrap();
            let writer = tokio::time::timeout(Duration::from_millis(100), state.media.write())
                .await
                .expect("a slow request must not hold the media lock");
            drop(writer);
            reply.send(()).unwrap();
        });
        result.unwrap();
        server.await.unwrap();
    }
}
