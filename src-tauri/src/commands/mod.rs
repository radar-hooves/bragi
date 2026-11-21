//! Tauri command handlers.
//!
//! All commands are organised by functionality:
//! - `cache`: Local SQLite cache management
//! - `developer`: Developer tools and diagnostics
//! - `downloads`: Offline download management
//! - `jellyfin`: Jellyfin server connection management
//! - `jellyfin_playlist`: Jellyfin playlist management
//! - `library`: Library browsing and search
//! - `playback`: Audio playback control
//! - `scrobble`: Scrobbling and sync with music service
//! - `settings`: Application settings
//! - `stats`: Track stats from music service
//!
//! Telemetry's get/set/probe commands are `tauri-plugin-telemetry`'s own,
//! invoked from the frontend as `plugin:telemetry|telemetry_get` and siblings
//! — no local module here any more.
mod ai_playlist;
mod cache;
mod developer;
mod downloads;
mod equalizer;
mod fade;
mod jellyfin;
mod jellyfin_playlist;
mod library;
mod mini_player;
mod playback;
mod scrobble;
mod settings;
mod shared_playlist;
mod stats;
mod subsonic;

pub use ai_playlist::*;
pub use cache::*;
pub use developer::*;
pub use downloads::*;
pub use equalizer::*;
pub use fade::*;
pub use jellyfin::*;
pub use jellyfin_playlist::*;
pub use library::*;
pub use mini_player::*;
pub use playback::*;
pub use scrobble::*;
pub use settings::*;
pub use shared_playlist::*;
pub use stats::*;
pub use subsonic::*;
