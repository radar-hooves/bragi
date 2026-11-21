//! Audio engine module.
//!
//! Handles audio playback using mpv with support for:
//! - Universal codec support (Opus, FLAC, MP3, AAC, etc.) via FFmpeg
//! - Native HTTP streaming
//! - Queue management with shuffle and repeat modes
//! - Gapless playback
//! - Output device selection
//! - Media key integration (macOS MPRemoteCommandCenter)
//! - Device change notifications (macOS CoreAudio)
//!
//! The audio engine runs on a dedicated thread and communicates via channels.
//! Commands are sent from Tauri commands, and events are broadcast to the frontend.
//!
//! Architecture:
//! - mpv subprocess runs with JSON IPC for control
//! - Engine thread manages state and translates commands/events

#[cfg(target_os = "macos")]
mod device_watcher;
mod devices;
pub mod eq;
mod mpv_commands;
mod mpv_engine;
mod mpv_events;
mod mpv_ipc;
mod mpv_playback;
pub(crate) mod mpv_process;
mod types;

/// The `tracing` target for engine events that may leave the device: stream
/// failures, end-file reasons and recovery steps. Content-free by rule (no
/// track, artist or URL), so a silent stop shows its cause in Loki.
pub(crate) const TARGET: &str = "bragi::audio";

#[cfg(test)]
mod engine_tests;

#[cfg(target_os = "macos")]
pub use device_watcher::DeviceWatcher;
pub use devices::query_mpv_devices;
pub use mpv_engine::{start_audio_engine, AudioEngineHandle};
pub use types::{
    AudioCommand, AudioDevice, AudioEvent, QueueEntrySource, QueueInsertPosition, RepeatMode,
    ReplayGainMode,
};
