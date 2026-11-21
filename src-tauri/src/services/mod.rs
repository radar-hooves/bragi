//! External service integrations.
//!
//! API clients for backend services that store user data.

mod client;
mod client_stats;
mod client_sync;
mod credentials;
mod models;
mod sync_scheduler;

pub use client::MusicServiceClient;
pub use credentials::{
    clear_music_service_credentials, load_music_service_credentials,
    store_music_service_credentials, MusicServiceCredentials,
};
pub use models::{ListenHistoryEntry, ScrobbleRequest, SkipRequest, SyncStatus};
pub use sync_scheduler::{perform_sync_with_metrics, start_sync_scheduler};
