//! Download manager implementation.
//!
//! Manages the download queue and coordinates background downloads.
//!
//! Sub-modules:
//! - `cache` - LRU eviction and full cache clearing
//! - `download_process` - Single download processing with retry logic

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};

use crate::db::{keys, Database};
use crate::error::AppError;
use crate::mediaserver::MediaClient;
use crate::proxy::ProxyHandle;

use super::cache::clear_all_cache;
use super::download_process::process_download;
use super::queue_handlers::{handle_queue_album, handle_queue_track};
use super::types::{
    CancellationToken, DownloadCommand, DownloadEvent, DownloadRequest, DownloadState,
    DownloadStatus, QueuedDownload,
};
use super::validation::sanitise_for_path;

/// Default cache size limit: 10GB
const DEFAULT_CACHE_SIZE_BYTES: u64 = 10 * 1024 * 1024 * 1024;

/// Handle for sending commands to the download manager.
#[derive(Clone)]
pub struct DownloadManagerHandle {
    command_tx: mpsc::Sender<DownloadCommand>,
}

impl DownloadManagerHandle {
    /// Queue a single track for download.
    pub async fn queue_track(&self, request: DownloadRequest) -> Result<(), AppError> {
        self.command_tx
            .send(DownloadCommand::QueueTrack(request))
            .await
            .map_err(|_| AppError::InvalidOperation("Download manager is not running".to_string()))
    }

    /// Queue all tracks from an album for download.
    pub async fn queue_album(
        &self,
        album_id: String,
        tracks: Vec<DownloadRequest>,
    ) -> Result<(), AppError> {
        self.command_tx
            .send(DownloadCommand::QueueAlbum { album_id, tracks })
            .await
            .map_err(|_| AppError::InvalidOperation("Download manager is not running".to_string()))
    }

    /// Cancel a download.
    pub async fn cancel(&self, track_id: String) -> Result<(), AppError> {
        self.command_tx
            .send(DownloadCommand::Cancel(track_id))
            .await
            .map_err(|_| AppError::InvalidOperation("Download manager is not running".to_string()))
    }

    /// Pause all downloads.
    pub async fn pause(&self) -> Result<(), AppError> {
        self.command_tx
            .send(DownloadCommand::Pause)
            .await
            .map_err(|_| AppError::InvalidOperation("Download manager is not running".to_string()))
    }

    /// Resume downloads.
    pub async fn resume(&self) -> Result<(), AppError> {
        self.command_tx
            .send(DownloadCommand::Resume)
            .await
            .map_err(|_| AppError::InvalidOperation("Download manager is not running".to_string()))
    }

    /// Clear completed and failed downloads from queue.
    pub async fn clear_completed(&self) -> Result<(), AppError> {
        self.command_tx
            .send(DownloadCommand::ClearCompleted)
            .await
            .map_err(|_| AppError::InvalidOperation("Download manager is not running".to_string()))
    }

    /// Get the current download queue.
    pub async fn get_queue(&self) -> Result<Vec<QueuedDownload>, AppError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.command_tx
            .send(DownloadCommand::GetQueue(tx))
            .await
            .map_err(|_| {
                AppError::InvalidOperation("Download manager is not running".to_string())
            })?;

        rx.await
            .map_err(|_| AppError::InvalidOperation("Failed to get download queue".to_string()))
    }

    /// Get the current download state.
    pub async fn get_state(&self) -> Result<DownloadState, AppError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.command_tx
            .send(DownloadCommand::GetState(tx))
            .await
            .map_err(|_| {
                AppError::InvalidOperation("Download manager is not running".to_string())
            })?;

        rx.await
            .map_err(|_| AppError::InvalidOperation("Failed to get download state".to_string()))
    }

    /// Set the cache size limit.
    pub async fn set_cache_limit(&self, limit_bytes: u64) -> Result<(), AppError> {
        self.command_tx
            .send(DownloadCommand::SetCacheLimit(limit_bytes))
            .await
            .map_err(|_| AppError::InvalidOperation("Download manager is not running".to_string()))
    }

    /// Clear all cached files.
    pub async fn clear_cache(&self) -> Result<(), AppError> {
        self.command_tx
            .send(DownloadCommand::ClearCache)
            .await
            .map_err(|_| AppError::InvalidOperation("Download manager is not running".to_string()))
    }
}

/// Internal state for the download manager.
pub(super) struct DownloadManagerState {
    /// Current download state.
    pub(super) state: DownloadState,
    /// Download queue.
    pub(super) queue: VecDeque<QueuedDownload>,
    /// Cache size limit in bytes.
    pub(super) cache_limit_bytes: u64,
    /// Cache directory path.
    cache_dir: PathBuf,
    /// Track ID currently being downloaded (if any).
    pub(super) active_download: Option<String>,
    /// Per-download cancellation tokens, keyed by track_id.
    cancellation_tokens: HashMap<String, CancellationToken>,
}

impl DownloadManagerState {
    pub(super) fn new(cache_dir: PathBuf, cache_limit_bytes: u64) -> Self {
        Self {
            state: DownloadState::Idle,
            queue: VecDeque::new(),
            cache_limit_bytes,
            cache_dir,
            active_download: None,
            cancellation_tokens: HashMap::new(),
        }
    }

    /// Get the file path for a cached track.
    ///
    /// Sanitises the track_id to prevent path traversal attacks.
    pub(super) fn cache_path(&self, track_id: &str, extension: &str) -> PathBuf {
        let safe_id = sanitise_for_path(track_id);
        self.cache_dir
            .join("audio")
            .join(format!("{}.{}", safe_id, extension))
    }

    /// Check if a track is already in the queue (pending or downloading).
    pub(super) fn is_track_queued(&self, track_id: &str) -> bool {
        self.queue.iter().any(|d| {
            d.track_id == track_id
                && (d.status == DownloadStatus::Pending || d.status == DownloadStatus::Downloading)
        })
    }

    /// Create a cancellation token for a download.
    pub(super) fn create_cancellation_token(&mut self, track_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.cancellation_tokens
            .insert(track_id.to_string(), token.clone());
        token
    }

    /// Get the cancellation token for a download.
    fn get_cancellation_token(&self, track_id: &str) -> Option<CancellationToken> {
        self.cancellation_tokens.get(track_id).cloned()
    }

    /// Remove the cancellation token for a download.
    pub(super) fn remove_cancellation_token(&mut self, track_id: &str) {
        self.cancellation_tokens.remove(track_id);
    }
}

/// Start the download manager background task.
///
/// Returns a handle for sending commands and a join handle for the background task.
pub fn start_download_manager<F>(
    cache_dir: PathBuf,
    db: Arc<Database>,
    jellyfin: Arc<RwLock<Option<MediaClient>>>,
    proxy: Arc<RwLock<Option<ProxyHandle>>>,
    audio: super::cache::AudioSlot,
    event_callback: F,
) -> (DownloadManagerHandle, tokio::task::JoinHandle<()>)
where
    F: Fn(DownloadEvent) + Send + Sync + 'static,
{
    let (command_tx, command_rx) = mpsc::channel::<DownloadCommand>(100);

    // Load cache limit from database or use default
    let cache_limit = db
        .get_setting(keys::CACHE_SIZE_LIMIT)
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_CACHE_SIZE_BYTES);

    let state = Arc::new(RwLock::new(DownloadManagerState::new(
        cache_dir,
        cache_limit,
    )));

    let event_callback = Arc::new(event_callback);

    let task = tokio::spawn(run_download_manager(
        command_rx,
        state,
        db,
        jellyfin,
        proxy,
        audio,
        event_callback,
    ));

    let handle = DownloadManagerHandle { command_tx };

    (handle, task)
}

/// Main loop for the download manager.
async fn run_download_manager<F>(
    mut command_rx: mpsc::Receiver<DownloadCommand>,
    state: Arc<RwLock<DownloadManagerState>>,
    db: Arc<Database>,
    jellyfin: Arc<RwLock<Option<MediaClient>>>,
    proxy: Arc<RwLock<Option<ProxyHandle>>>,
    audio: super::cache::AudioSlot,
    event_callback: Arc<F>,
) where
    F: Fn(DownloadEvent) + Send + Sync + 'static,
{
    tracing::info!("Download manager started");

    loop {
        // Check if we should start a new download
        let should_download = {
            let state_guard = state.read().await;
            state_guard.state == DownloadState::Downloading
                && state_guard.active_download.is_none()
                && state_guard
                    .queue
                    .iter()
                    .any(|d| d.status == DownloadStatus::Pending)
        };

        if should_download {
            // Get the next pending download
            let next_download = {
                let mut state_guard = state.write().await;
                let idx = state_guard
                    .queue
                    .iter()
                    .position(|d| d.status == DownloadStatus::Pending);

                if let Some(idx) = idx {
                    let track_id = state_guard.queue[idx].track_id.clone();
                    state_guard.queue[idx].status = DownloadStatus::Downloading;
                    state_guard.active_download = Some(track_id.clone());
                    let cancellation_token = state_guard.create_cancellation_token(&track_id);
                    Some((state_guard.queue[idx].clone(), cancellation_token))
                } else {
                    None
                }
            };

            if let Some((download, cancellation_token)) = next_download {
                // Spawn the download task
                let state_clone = state.clone();
                let db_clone = db.clone();
                let jellyfin_clone = jellyfin.clone();
                let proxy_clone = proxy.clone();
                let event_callback_clone = event_callback.clone();

                tokio::spawn(async move {
                    process_download(
                        download,
                        state_clone,
                        db_clone,
                        jellyfin_clone,
                        proxy_clone,
                        event_callback_clone,
                        cancellation_token,
                    )
                    .await;
                });
            }
        }

        // Process commands with a timeout to allow checking for new downloads
        let command =
            tokio::time::timeout(std::time::Duration::from_millis(100), command_rx.recv()).await;

        match command {
            Ok(Some(cmd)) => {
                handle_command(cmd, &state, &db, &audio, &event_callback).await;
            }
            Ok(None) => {
                // Channel closed, shutdown
                tracing::info!("Download manager command channel closed");
                break;
            }
            Err(_) => {
                // Timeout, continue loop to check for new downloads
            }
        }

        // Check if we should transition to idle. Paused counts too: a paused
        // manager whose queue empties would otherwise stay "Downloading"
        // forever after a resume with nothing left to do.
        {
            let mut state_guard = state.write().await;
            if (state_guard.state == DownloadState::Downloading
                || state_guard.state == DownloadState::Paused)
                && state_guard.active_download.is_none()
                && !state_guard
                    .queue
                    .iter()
                    .any(|d| d.status == DownloadStatus::Pending)
            {
                state_guard.state = DownloadState::Idle;
                event_callback(DownloadEvent::StateChanged {
                    state: DownloadState::Idle,
                });
            }
        }
    }

    tracing::info!("Download manager stopped");
}

/// Handle a download manager command.
async fn handle_command<F>(
    command: DownloadCommand,
    state: &Arc<RwLock<DownloadManagerState>>,
    db: &Arc<Database>,
    audio: &super::cache::AudioSlot,
    event_callback: &Arc<F>,
) where
    F: Fn(DownloadEvent) + Send + Sync + 'static,
{
    match command {
        DownloadCommand::QueueTrack(request) => {
            handle_queue_track(request, state, db, event_callback).await;
        }

        DownloadCommand::QueueAlbum { album_id, tracks } => {
            handle_queue_album(album_id, tracks, state, db, event_callback).await;
        }

        DownloadCommand::Cancel(track_id) => {
            let mut state_guard = state.write().await;

            // If this is the active download, cancel via token
            if state_guard.active_download.as_ref() == Some(&track_id) {
                if let Some(token) = state_guard.get_cancellation_token(&track_id) {
                    token.cancel();
                    tracing::debug!("Cancellation requested for active download: {}", track_id);
                }
            }

            // Update status in queue
            if let Some(download) = state_guard
                .queue
                .iter_mut()
                .find(|d| d.track_id == track_id)
            {
                if download.status == DownloadStatus::Pending
                    || download.status == DownloadStatus::Downloading
                {
                    download.status = DownloadStatus::Cancelled;
                    event_callback(DownloadEvent::Cancelled {
                        track_id: track_id.clone(),
                    });
                }
            }
        }

        DownloadCommand::Pause => {
            let mut state_guard = state.write().await;
            if state_guard.state == DownloadState::Downloading {
                state_guard.state = DownloadState::Paused;
                event_callback(DownloadEvent::StateChanged {
                    state: DownloadState::Paused,
                });
            }
        }

        DownloadCommand::Resume => {
            let mut state_guard = state.write().await;
            if state_guard.state == DownloadState::Paused {
                state_guard.state = DownloadState::Downloading;
                event_callback(DownloadEvent::StateChanged {
                    state: DownloadState::Downloading,
                });
            }
        }

        DownloadCommand::ClearCompleted => {
            let mut state_guard = state.write().await;
            state_guard.queue.retain(|d| {
                d.status != DownloadStatus::Completed
                    && d.status != DownloadStatus::Failed
                    && d.status != DownloadStatus::Cancelled
            });
            event_callback(DownloadEvent::QueueChanged {
                queue_length: state_guard.queue.len(),
            });
        }

        DownloadCommand::GetQueue(reply) => {
            let state_guard = state.read().await;
            let queue: Vec<QueuedDownload> = state_guard.queue.iter().cloned().collect();
            let _ = reply.send(queue);
        }

        DownloadCommand::GetState(reply) => {
            let state_guard = state.read().await;
            let _ = reply.send(state_guard.state);
        }

        DownloadCommand::SetCacheLimit(limit_bytes) => {
            let mut state_guard = state.write().await;
            state_guard.cache_limit_bytes = limit_bytes;

            // Store in database
            if let Err(e) = db.set_setting(keys::CACHE_SIZE_LIMIT, &limit_bytes.to_string()) {
                tracing::error!("Failed to save cache limit setting: {}", e);
            }
        }

        DownloadCommand::ClearCache => {
            let protected = super::cache::protected_track_ids(audio).await;
            if let Err(e) = clear_all_cache(db, &protected, event_callback).await {
                tracing::error!("Cache clear failed: {}", e);
            }
        }
    }
}
