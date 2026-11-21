//! Queue command handlers for the download manager.
//!
//! Handles validation and queuing of single tracks and albums for download.

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::db::Database;

use super::manager::DownloadManagerState;
use super::types::{DownloadEvent, DownloadRequest, DownloadState, QueuedDownload};
use super::validation::validate_jellyfin_id;

/// Handle queuing a single track for download.
pub(super) async fn handle_queue_track<F>(
    request: DownloadRequest,
    state: &Arc<RwLock<DownloadManagerState>>,
    db: &Arc<Database>,
    event_callback: &Arc<F>,
) where
    F: Fn(DownloadEvent) + Send + Sync + 'static,
{
    // Validate the track ID
    if let Err(e) = validate_jellyfin_id(&request.track_id, "track") {
        tracing::warn!("Invalid track ID rejected: {}", e);
        return;
    }

    let mut state_guard = state.write().await;

    // Check for duplicates - skip if already queued or downloading
    if state_guard.is_track_queued(&request.track_id) {
        tracing::debug!("Track {} already in queue, skipping", request.track_id);
        return;
    }

    // Check if already cached in database
    if let Ok(Some(track)) = db.get_track(&request.track_id) {
        if track.cached_file_path.is_some() {
            tracing::debug!("Track {} already cached, skipping", request.track_id);
            return;
        }
    }

    let queued = QueuedDownload::from_request(request, chrono::Utc::now().timestamp());
    state_guard.queue.push_back(queued);

    // Start downloading if idle
    if state_guard.state == DownloadState::Idle {
        state_guard.state = DownloadState::Downloading;
        event_callback(DownloadEvent::StateChanged {
            state: DownloadState::Downloading,
        });
    }

    event_callback(DownloadEvent::QueueChanged {
        queue_length: state_guard.queue.len(),
    });
}

/// Handle queuing all tracks from an album.
pub(super) async fn handle_queue_album<F>(
    album_id: String,
    tracks: Vec<DownloadRequest>,
    state: &Arc<RwLock<DownloadManagerState>>,
    db: &Arc<Database>,
    event_callback: &Arc<F>,
) where
    F: Fn(DownloadEvent) + Send + Sync + 'static,
{
    // Validate the album ID
    if let Err(e) = validate_jellyfin_id(&album_id, "album") {
        tracing::warn!("Invalid album ID rejected: {}", e);
        return;
    }

    let mut state_guard = state.write().await;
    let timestamp = chrono::Utc::now().timestamp();
    let mut added_count = 0;

    for mut request in tracks {
        // Validate each track ID
        if let Err(e) = validate_jellyfin_id(&request.track_id, "track") {
            tracing::warn!("Invalid track ID in album rejected: {}", e);
            continue;
        }

        // Check for duplicates
        if state_guard.is_track_queued(&request.track_id) {
            tracing::debug!("Track {} already in queue, skipping", request.track_id);
            continue;
        }

        // Check if already cached
        if let Ok(Some(track)) = db.get_track(&request.track_id) {
            if track.cached_file_path.is_some() {
                tracing::debug!("Track {} already cached, skipping", request.track_id);
                continue;
            }
        }

        request.album_id = Some(album_id.clone());
        let queued = QueuedDownload::from_request(request, timestamp);
        state_guard.queue.push_back(queued);
        added_count += 1;
    }

    if added_count == 0 {
        tracing::debug!("No new tracks to add for album {}", album_id);
        return;
    }

    // Start downloading if idle
    if state_guard.state == DownloadState::Idle {
        state_guard.state = DownloadState::Downloading;
        event_callback(DownloadEvent::StateChanged {
            state: DownloadState::Downloading,
        });
    }

    event_callback(DownloadEvent::QueueChanged {
        queue_length: state_guard.queue.len(),
    });
}
