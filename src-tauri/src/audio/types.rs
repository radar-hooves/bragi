//! Audio engine types.
//!
//! Commands, events, and settings for the audio playback system.

use serde::{Deserialize, Serialize};

use crate::models::{PlaybackState, QueueState, Track};

/// Commands sent to the audio engine thread.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum AudioCommand {
    /// Start playing a track, optionally clearing the queue first.
    Play {
        track: Track,
        stream_url: String,
        clear_queue: bool,
    },
    /// Replace the queue with `tracks` (in order) and start playing the entry
    /// at `start_index`, atomically — the one mechanism every "play this row,
    /// queuing the rest of the list" entry point goes through.
    ///
    /// Unlike `Play` + a follow-up `AddToQueue`, this is a single engine
    /// operation: no window where the queue holds only the just-started track,
    /// and tracks BEFORE `start_index` are kept in the queue too (so Previous
    /// can navigate back into the context the row was played from, matching
    /// Apple Music/Spotify — the two-call dance only ever sent the tail).
    ///
    /// Any not-yet-played `UserQueued` entry (Play Next / Add to Queue) ahead
    /// of the current position survives into the new queue, right after the
    /// new current track — Spotify's "Next in queue" plays before a newly
    /// started context continues, rather than vanishing under it.
    ///
    /// `shuffle` is applied to the new context (start track pinned first)
    /// before the first `queue-changed`/`playback-state-changed` broadcast,
    /// so a caller with shuffle already on sees one shuffled queue rather
    /// than an unshuffled one that visibly reorders itself a moment later.
    PlayContext {
        tracks: Vec<(Track, String)>,
        start_index: usize,
        shuffle: bool,
    },
    /// Pause playback.
    Pause,
    /// Resume playback.
    Resume,
    /// Toggle between pause and resume.
    TogglePlayPause,
    /// Stop playback and clear position.
    // The handler and its StopOut fade are built and tested, but no UI
    // sends it: the `stop` Tauri command it served was never invoked.
    #[allow(dead_code)]
    Stop,
    /// Seek to a position in milliseconds.
    Seek(u64),
    /// Set volume (0.0 to 1.0).
    SetVolume(f32),
    /// Add tracks to the queue.
    AddToQueue {
        tracks: Vec<(Track, String)>,
        position: QueueInsertPosition,
    },
    /// Clear the queue.
    ClearQueue,
    /// Remove a track from the queue by index.
    RemoveFromQueue(usize),
    /// Reorder a track in the queue.
    ReorderQueue { from: usize, to: usize },
    /// Jump directly to a queue entry by index — the Up Next panel's
    /// double-click/Return activation. Shares `play_track` with
    /// `SkipNext`/`SkipPrevious` rather than opening a second play path.
    JumpToQueueIndex(usize),
    /// Skip to the next track.
    SkipNext,
    /// Skip to the previous track.
    SkipPrevious,
    /// Set shuffle mode.
    SetShuffle(bool),
    /// Set repeat mode.
    SetRepeat(RepeatMode),
    /// Set the output device by name.
    SetOutputDevice(Option<String>),
    /// Read a synchronous snapshot of playback and queue state.
    ///
    /// The engine replies on the provided channel. The
    /// caller gets the state directly rather than via an event — this is what
    /// makes `get_playback_state` / `get_queue` return real values, and it is
    /// what a test asserts against. When mpv is playing, the
    /// position is refreshed from mpv itself before the snapshot is taken, so
    /// a stalled mpv cannot masquerade as advancing playback.
    QuerySnapshot(std::sync::mpsc::Sender<(PlaybackState, QueueState)>),
    /// Set ReplayGain settings.
    // Engine handler is built, but no command sends it: the saved
    // replay_gain_* settings are never pushed to the engine.
    #[allow(dead_code)]
    SetReplayGain(ReplayGainSettings),
    /// Enable or disable the equalizer.
    SetEqEnabled(bool),
    /// Set the equalizer preset by name (see `audio::eq::EqPreset`).
    SetEqPreset(String),
    /// Set custom equalizer band gains in dB (10 or 18 bands).
    SetEqBands(Vec<i32>),
    /// Enable/disable the pause/stop volume fade and set its ramp duration.
    SetFade { enabled: bool, duration_ms: u64 },
    /// Shutdown the audio engine.
    Shutdown,
}

/// Position for inserting tracks into the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueueInsertPosition {
    /// Insert after the current track.
    Next,
    /// Append to the end of the queue.
    End,
}

/// Where a queue entry came from — the serialisable counterpart of
/// `mpv_engine::QueueSource`, carried on `QueueState`/`QueueChanged` so the Up
/// Next panel can group "Playing Next" above the rest of the context using
/// the engine's own queue order, rather than a second, locally-held queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QueueEntrySource {
    /// Part of a played context (`Play`/`PlayContext` — a Songs table, an
    /// album, a playlist, ...).
    Context,
    /// Explicitly queued by the user (`AddToQueue` — Play Next / Add to
    /// Queue), independent of whatever context is currently playing.
    UserQueued,
}

/// Repeat mode for playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    /// No repeat.
    #[default]
    Off,
    /// Repeat the entire queue.
    All,
    /// Repeat the current track.
    One,
}

/// Events emitted by the audio engine.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AudioEvent {
    /// Playback state changed (track changed, play/pause toggled).
    #[serde(rename_all = "camelCase")]
    StateChanged {
        current_track: Option<Track>,
        is_playing: bool,
        current_time_ms: u64,
        duration_ms: u64,
        volume: f32,
        shuffle: bool,
        repeat: RepeatMode,
        output_device: Option<String>,
    },
    /// Playback position update (emitted every second during playback).
    #[serde(rename_all = "camelCase")]
    Progress {
        current_time_ms: u64,
        duration_ms: u64,
    },
    /// Queue contents changed.
    #[serde(rename_all = "camelCase")]
    QueueChanged {
        tracks: Vec<Track>,
        current_index: i32,
        /// Parallel to `tracks` — where each entry came from, so the Up Next
        /// panel can group "Playing Next" (user-queued) ahead of the rest of
        /// the context without a second, locally-held queue.
        sources: Vec<QueueEntrySource>,
    },
    /// An error occurred during playback.
    #[serde(rename_all = "camelCase")]
    Error { message: String },
    /// Track finished playing (for scrobbling).
    ///
    /// `finished_at` is the wall-clock Unix timestamp (seconds) when the track
    /// finished. For gapless batches where multiple tracks complete at once,
    /// timestamps are reconstructed from track durations so each listen gets
    /// a distinct, accurate time.
    #[serde(rename_all = "camelCase")]
    TrackFinished {
        track: Track,
        played_ms: u64,
        finished_at: i64,
    },
    /// Audio output devices changed (connected/disconnected).
    ///
    /// Only ever constructed on macOS (`DeviceWatcher`, `#[cfg(target_os =
    /// "macos")]` in lib.rs) — Linux has no device-watcher backend yet — but the
    /// variant itself stays unconditional so this stays one enum the frontend's
    /// generated TS type can match on every platform.
    #[serde(rename_all = "camelCase")]
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    DevicesChanged { devices: Vec<AudioDevice> },
}

/// ReplayGain mode preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReplayGainMode {
    /// Normalise each track individually.
    #[default]
    Track,
    /// Preserve album dynamics.
    Album,
    /// ReplayGain disabled.
    Off,
}

/// ReplayGain settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayGainSettings {
    /// Whether ReplayGain is enabled.
    pub enabled: bool,
    /// Track or album mode.
    pub mode: ReplayGainMode,
    /// Additional gain adjustment in dB.
    pub pre_amp_db: f32,
    /// Whether to prevent clipping.
    pub prevent_clipping: bool,
}

impl Default for ReplayGainSettings {
    fn default() -> Self {
        Self {
            // Off by default: Jellyfin's NormalizationGain values produced
            // near-silent playback on some tracks.
            enabled: false,
            mode: ReplayGainMode::Track,
            pre_amp_db: 0.0,
            prevent_clipping: true,
        }
    }
}

/// Audio output device information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    /// Device identifier (mpv's `audio-device-list` name, e.g. `coreaudio/BuiltInSpeakerDevice`).
    /// Passed to `--audio-device=` when spawning mpv.
    pub name: String,
    /// Human-readable description (e.g. "MacBook Pro Speakers").
    /// Displayed in the UI device selector.
    pub description: String,
    /// Whether this is the default device.
    pub is_default: bool,
}
