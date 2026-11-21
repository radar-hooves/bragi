//! Download manager for offline audio caching.
//!
//! Handles downloading tracks from Jellyfin for offline playback with:
//! - Background download queue with progress reporting
//! - A configurable cache size limit that refuses a new download once full,
//!   never one that silently deletes an existing download to make room
//! - Pause, resume, and cancel operations
//!
//! Downloads are stored in the app cache directory and tracked in SQLite.

mod cache;
mod download_process;
mod manager;
mod queue_handlers;
mod types;
pub mod validation;
mod worker;

pub use manager::{start_download_manager, DownloadManagerHandle};
pub use types::{
    CacheSettings, DownloadEvent, DownloadFormat, DownloadRequest, DownloadState, QueuedDownload,
};
