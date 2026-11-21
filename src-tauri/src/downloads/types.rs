//! Types for the download manager.

use serde::{Deserialize, Serialize};

/// Current state of the download manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DownloadState {
    /// Download manager is idle with no active downloads.
    #[default]
    Idle,
    /// Actively downloading tracks.
    Downloading,
    /// Download paused by user.
    Paused,
}

/// Status of an individual download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadStatus {
    /// Waiting in queue.
    Pending,
    /// Currently downloading.
    Downloading,
    /// Download completed successfully.
    Completed,
    /// Download failed.
    Failed,
    /// Download cancelled by user.
    Cancelled,
}

/// Preferred download/transcode format for offline copies.
///
/// `Original` (the default) keeps the exact source bytes from Jellyfin; the
/// lossy/lossless variants request a server-side transcode. The on-disk file
/// extension is the container returned by [`DownloadFormat::transcode_params`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DownloadFormat {
    /// Keep the exact source file (no transcode).
    #[default]
    Original,
    /// Transcode to FLAC (lossless).
    Flac,
    /// Transcode to MP3 (lossy).
    Mp3,
    /// Transcode to AAC in an m4a container (lossy).
    Aac,
    /// Transcode to Opus (lossy).
    Opus,
}

impl DownloadFormat {
    /// Parse a stored setting value into a `DownloadFormat`.
    ///
    /// Named `parse` (not `from_str`) to avoid clashing with the `FromStr`
    /// trait convention (`clippy::should_implement_trait`).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "original" => Some(Self::Original),
            "flac" => Some(Self::Flac),
            "mp3" => Some(Self::Mp3),
            "aac" => Some(Self::Aac),
            "opus" => Some(Self::Opus),
            _ => None,
        }
    }

    /// Stable string key used for persistence and IPC.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Flac => "flac",
            Self::Mp3 => "mp3",
            Self::Aac => "aac",
            Self::Opus => "opus",
        }
    }

    /// Whether this format is lossy (and therefore honours the bitrate setting).
    pub fn is_lossy(&self) -> bool {
        matches!(self, Self::Mp3 | Self::Aac | Self::Opus)
    }

    /// Jellyfin transcode parameters as `(container, audio_codec)`.
    ///
    /// Returns `None` for [`DownloadFormat::Original`] (no transcode). The
    /// container doubles as the on-disk file extension and is always within
    /// the allow-list of [`super::worker::get_extension_from_url`].
    pub fn transcode_params(&self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Original => None,
            Self::Flac => Some(("flac", "flac")),
            Self::Mp3 => Some(("mp3", "mp3")),
            Self::Aac => Some(("m4a", "aac")),
            Self::Opus => Some(("opus", "opus")),
        }
    }

    /// The on-disk container extension requested for a transcode.
    ///
    /// Returns `None` for [`DownloadFormat::Original`] (the extension is
    /// resolved from the response `Content-Type` / URL instead). This is the
    /// container half of [`DownloadFormat::transcode_params`].
    pub fn container_extension(&self) -> Option<&'static str> {
        self.transcode_params().map(|(container, _codec)| container)
    }
}

/// A download request from the frontend.
#[derive(Debug, Clone)]
pub struct DownloadRequest {
    /// Jellyfin track ID.
    pub track_id: String,
    /// Track name for display.
    pub track_name: String,
    /// Artist name for display.
    pub artist_name: String,
    /// Optional album ID (for batch downloads).
    pub album_id: Option<String>,
}

/// A queued download with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedDownload {
    /// Jellyfin track ID.
    pub track_id: String,
    /// Track name for display.
    pub track_name: String,
    /// Artist name for display.
    pub artist_name: String,
    /// Album ID if part of album download.
    pub album_id: Option<String>,
    /// Current download status.
    pub status: DownloadStatus,
    /// Progress percentage (0-100).
    pub progress_percent: u8,
    /// Bytes downloaded so far.
    pub bytes_downloaded: u64,
    /// Total bytes (if known).
    pub total_bytes: Option<u64>,
    /// Error message if failed.
    pub error: Option<String>,
    /// When this download was queued (Unix timestamp).
    pub queued_at: i64,
}

impl QueuedDownload {
    /// Create a new queued download from a request.
    pub fn from_request(request: DownloadRequest, queued_at: i64) -> Self {
        Self {
            track_id: request.track_id,
            track_name: request.track_name,
            artist_name: request.artist_name,
            album_id: request.album_id,
            status: DownloadStatus::Pending,
            progress_percent: 0,
            bytes_downloaded: 0,
            total_bytes: None,
            error: None,
            queued_at,
        }
    }
}

/// Cache settings for download management.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheSettings {
    /// Maximum cache size in bytes (default: 10GB).
    pub max_size_bytes: u64,
    /// Current cache size in bytes.
    pub current_size_bytes: u64,
    /// Number of cached tracks.
    pub cached_track_count: u32,
}

impl Default for CacheSettings {
    fn default() -> Self {
        Self {
            // Default 10GB cache
            max_size_bytes: 10 * 1024 * 1024 * 1024,
            current_size_bytes: 0,
            cached_track_count: 0,
        }
    }
}

/// Events emitted by the download manager.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DownloadEvent {
    /// Download progress update.
    #[serde(rename_all = "camelCase")]
    Progress {
        track_id: String,
        track_name: String,
        artist_name: String,
        bytes_downloaded: u64,
        total_bytes: Option<u64>,
        progress_percent: u8,
    },
    /// Download completed.
    #[serde(rename_all = "camelCase")]
    Completed {
        track_id: String,
        track_name: String,
        file_path: String,
        file_size_bytes: u64,
    },
    /// Download failed.
    #[serde(rename_all = "camelCase")]
    Failed {
        track_id: String,
        track_name: String,
        error: String,
    },
    /// Download cancelled.
    #[serde(rename_all = "camelCase")]
    Cancelled { track_id: String },
    /// Queue changed (item added/removed).
    #[serde(rename_all = "camelCase")]
    QueueChanged { queue_length: usize },
    /// Download state changed (idle/downloading/paused).
    #[serde(rename_all = "camelCase")]
    StateChanged { state: DownloadState },
    /// A "Clear all cache" fired.
    ///
    /// `evicted_track_ids` names exactly the tracks that lost their cached
    /// file, so the frontend can clear only those tracks' downloaded badges
    /// (`markTrackRemoved`) instead of wiping every badge in the library — a
    /// currently-playing (protected) track survives the clear, so its badge
    /// must survive too.
    #[serde(rename_all = "camelCase")]
    CacheEvicted {
        evicted_count: u32,
        freed_bytes: u64,
        evicted_track_ids: Vec<String>,
    },
}

/// Commands sent to the download manager.
#[derive(Debug)]
pub enum DownloadCommand {
    /// Add a track to the download queue.
    QueueTrack(DownloadRequest),
    /// Add multiple tracks (album) to the queue.
    QueueAlbum {
        album_id: String,
        tracks: Vec<DownloadRequest>,
    },
    /// Cancel a pending or active download.
    Cancel(String),
    /// Pause all downloads.
    Pause,
    /// Resume downloads.
    Resume,
    /// Clear completed and failed downloads from queue.
    ClearCompleted,
    /// Get current queue state.
    GetQueue(tokio::sync::oneshot::Sender<Vec<QueuedDownload>>),
    /// Get current download state.
    GetState(tokio::sync::oneshot::Sender<DownloadState>),
    /// Set cache size limit.
    SetCacheLimit(u64),
    /// Clear all cached files.
    ClearCache,
}

/// A per-download cancellation token.
///
/// Passed into the download worker and checked periodically during streaming.
#[derive(Clone)]
pub struct CancellationToken {
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl std::fmt::Debug for CancellationToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CancellationToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancellationToken {
    /// Create a new cancellation token.
    pub fn new() -> Self {
        Self {
            cancelled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Check if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Request cancellation.
    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::super::worker::get_extension_from_url;
    use super::*;

    #[test]
    fn download_format_default_is_original() {
        assert_eq!(DownloadFormat::default(), DownloadFormat::Original);
    }

    #[test]
    fn download_format_parse_round_trips_as_str() {
        for fmt in [
            DownloadFormat::Original,
            DownloadFormat::Flac,
            DownloadFormat::Mp3,
            DownloadFormat::Aac,
            DownloadFormat::Opus,
        ] {
            assert_eq!(DownloadFormat::parse(fmt.as_str()), Some(fmt));
        }
        assert_eq!(DownloadFormat::parse("bogus"), None);
    }

    #[test]
    fn download_format_transcode_params() {
        assert_eq!(DownloadFormat::Original.transcode_params(), None);
        assert_eq!(
            DownloadFormat::Flac.transcode_params(),
            Some(("flac", "flac"))
        );
        assert_eq!(DownloadFormat::Mp3.transcode_params(), Some(("mp3", "mp3")));
        assert_eq!(DownloadFormat::Aac.transcode_params(), Some(("m4a", "aac")));
        assert_eq!(
            DownloadFormat::Opus.transcode_params(),
            Some(("opus", "opus"))
        );
    }

    #[test]
    fn download_format_container_extension_matches_transcode_params() {
        for fmt in [
            DownloadFormat::Original,
            DownloadFormat::Flac,
            DownloadFormat::Mp3,
            DownloadFormat::Aac,
            DownloadFormat::Opus,
        ] {
            assert_eq!(
                fmt.container_extension(),
                fmt.transcode_params().map(|(container, _)| container),
            );
        }
    }

    #[test]
    fn download_format_is_lossy() {
        assert!(!DownloadFormat::Original.is_lossy());
        assert!(!DownloadFormat::Flac.is_lossy());
        assert!(DownloadFormat::Mp3.is_lossy());
        assert!(DownloadFormat::Aac.is_lossy());
        assert!(DownloadFormat::Opus.is_lossy());
    }

    #[test]
    fn transcode_container_is_in_extension_allow_list() {
        // Guards against transcode_params and get_extension_from_url drifting
        // apart: every transcode container must be recognised as an extension.
        for fmt in [
            DownloadFormat::Flac,
            DownloadFormat::Mp3,
            DownloadFormat::Aac,
            DownloadFormat::Opus,
        ] {
            let (container, _codec) = fmt.transcode_params().unwrap();
            let url = format!("https://host/Audio/ID/stream.{}", container);
            assert_eq!(
                get_extension_from_url(&url),
                container,
                "container {container} must be recognised by get_extension_from_url"
            );
        }
    }
}
