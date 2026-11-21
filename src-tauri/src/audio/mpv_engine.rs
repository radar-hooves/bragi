//! mpv-based audio playback engine.
//!
//! Replaces the rodio-based engine with mpv subprocess for universal codec support.
//! mpv handles all audio decoding natively, including Opus, FLAC, MP3, AAC, etc.
//!
//! Architecture:
//! - mpv subprocess runs with `--input-ipc-server` for JSON IPC control
//! - Engine thread communicates via Unix socket
//! - Property observation provides playback state updates
//! - mpv's playlist is used for queue management and gapless playback
//!
//! Sub-modules:
//! - `mpv_commands` - Command dispatch from Tauri frontend
//! - `mpv_events` - Event handling from mpv IPC
//! - `mpv_playback` - Track loading, seeking, gapless queue management

use std::collections::VecDeque;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::mpv_commands::{apply_eq, handle_command, restart_mpv};
use super::mpv_events::handle_mpv_event;
use super::mpv_ipc::MpvIpc;
use super::mpv_process::{connect_to_mpv, get_socket_path, setup_property_observers, spawn_mpv};
use super::types::{AudioCommand, AudioEvent, QueueEntrySource, RepeatMode, ReplayGainSettings};
use crate::models::Track;

/// Interval for progress updates during playback.
const PROGRESS_UPDATE_INTERVAL: Duration = Duration::from_secs(1);

/// Which volume ramp an [`ActiveFade`] is performing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FadeKind {
    /// Fade the volume down, then pause mpv (a pause request).
    PauseOut,
    /// Fade the volume up from silence after resuming mpv (a resume request).
    ResumeIn,
    /// Fade the volume down, then stop mpv and clear state (a stop request).
    StopOut,
}

/// An in-progress volume ramp, driven one step per engine-loop iteration.
///
/// Fields are `pub(crate)` so the engine tests (a `super::` submodule) can
/// construct an `ActiveFade` and back-date `start` to simulate elapsed time
/// without sleeping.
pub(crate) struct ActiveFade {
    pub(crate) kind: FadeKind,
    pub(crate) start: Instant,
    pub(crate) duration_ms: u64,
}

/// Audio engine handle for sending commands from Tauri commands.
///
/// This is Clone and Send, so it can be stored in AppState and used
/// from multiple async tasks.
#[derive(Clone)]
pub struct AudioEngineHandle {
    command_tx: Sender<AudioCommand>,
    is_running: Arc<AtomicBool>,
}

impl AudioEngineHandle {
    /// Send a command to the audio engine.
    pub fn send(&self, command: AudioCommand) -> Result<(), String> {
        if !self.is_running.load(Ordering::SeqCst) {
            return Err("Audio engine is not running".to_string());
        }

        self.command_tx
            .send(command)
            .map_err(|e| format!("Failed to send command to audio engine: {}", e))
    }

    /// Shutdown the audio engine.
    pub fn shutdown(&self) {
        let _ = self.command_tx.send(AudioCommand::Shutdown);
    }
}

/// Internal state of the audio engine.
pub(crate) struct EngineState {
    /// Current track info.
    pub(crate) current_track: Option<Track>,

    /// Queue of tracks with their stream URLs.
    pub(crate) queue: Vec<QueuedTrack>,
    /// Current position in the queue.
    pub(crate) queue_position: Option<usize>,
    /// Original queue order (for shuffle mode restoration).
    pub(crate) original_queue_order: Vec<usize>,

    /// Whether playback is active.
    pub(crate) is_playing: bool,
    /// Volume level (0.0 to 1.0).
    pub(crate) volume: f32,
    /// Shuffle mode enabled.
    pub(crate) shuffle: bool,
    /// Repeat mode.
    pub(crate) repeat: RepeatMode,

    /// ReplayGain settings.
    pub(crate) replay_gain: ReplayGainSettings,

    /// Whether the equalizer is enabled.
    pub(crate) eq_enabled: bool,
    /// Current equalizer band gains in dB (18-band canonical form). Kept in
    /// engine state so it can be re-applied to every fresh mpv connection —
    /// the `af` filter property does not survive an mpv respawn.
    pub(crate) eq_bands: Vec<i32>,

    /// Track start time (for position calculation during network latency).
    pub(crate) track_start_time: Option<Instant>,
    /// Track start position from mpv.
    pub(crate) track_start_position_ms: u64,
    /// Last known position when paused.
    pub(crate) paused_position_ms: u64,

    /// Selected output device name.
    pub(crate) output_device_name: Option<String>,

    /// Tracks queued in mpv's playlist for gapless playback.
    /// Front of queue is current track, rest are upcoming.
    pub(crate) mpv_playlist_tracks: VecDeque<PlaylistEntry>,

    /// Whether mpv is currently loading a track (suppresses progress updates).
    /// Set true on loadfile, cleared on playback-restart event.
    pub(crate) is_loading: bool,

    /// Last emitted TrackFinished jellyfin_id for deduplication.
    pub(crate) last_finished_track_id: Option<String>,
    /// Timestamp of the last TrackFinished emission for deduplication.
    pub(crate) last_finished_at: Option<Instant>,

    /// Absolute mpv playlist position of mpv_playlist_tracks[0].
    /// Increments as gapless transitions pop the front of the tracking queue.
    /// Reset to 0 on every fresh play_track call.
    pub(crate) mpv_playlist_base_pos: usize,

    /// Consecutive stream-open failures since the last successful playback
    /// start. Bounds the retry/advance recovery so a systemic fault (wrong
    /// audio device, dead network) stops with an error instead of walking
    /// the whole queue.
    pub(crate) consecutive_stream_failures: u32,
    /// Track already retried once after a stream failure; the next failure
    /// for the same track advances instead of retrying again.
    pub(crate) retried_track_id: Option<String>,

    /// Entries recently popped from `mpv_playlist_tracks` by the position
    /// handler, kept so a late-arriving end-file(eof) can still find its
    /// track for the listen credit (event ordering on the socket is not
    /// guaranteed). Bounded to the last few entries.
    ///
    /// The third field is the seek-aware listened time captured at pop time for
    /// the entry that was the accounted current track, so a pos-change-before-eof
    /// ordering still credits actual listened time rather than full duration.
    /// `None` for entries whose accounting was not live at the pop.
    pub(crate) recently_popped: VecDeque<(i64, Track, Option<u64>)>,

    /// When set, a stream-failure recovery (retry or advance) fires once this
    /// deadline passes. Deferred rather than immediate so a transient fault
    /// has time to clear, and so an instantly-failing source cannot burn the
    /// whole retry budget in milliseconds. Cancelled by any new user intent
    /// (play, stop).
    pub(crate) recovery_due: Option<Instant>,

    /// Set when mpv failed to open its audio output. A long-lived mpv can keep
    /// a stale CoreAudio device table after a Bluetooth output drops and
    /// returns, failing every later open of a device that is back, so the
    /// engine loop replaces the process (resuming the current track) once no
    /// recovery is pending.
    pub(crate) mpv_respawn_due: bool,

    /// Consecutive progress polls that found mpv idle while the engine
    /// believed it was playing; see `reconcile_idle_playback`.
    pub(crate) idle_polls: u8,

    /// A seek requested while the current track was still loading. mpv
    /// rejects seeks before the file is ready, so the target is parked here
    /// and applied on the playback-restart event.
    pub(crate) pending_seek_ms: Option<u64>,

    /// Sum of CLOSED play spans for the current track, in milliseconds.
    ///
    /// Seek-aware listened time: a span is opened at `span_start_ms` and closed
    /// (folded into this total) whenever a seek jumps the position or the track
    /// ends. Reset to 0 on every track change so credits reflect actual audio
    /// heard, not full duration.
    pub(crate) listened_ms: u64,

    /// Media position where the currently-open play span began.
    pub(crate) span_start_ms: u64,

    /// Whether the pause/stop/resume volume fade is enabled.
    pub(crate) fade_enabled: bool,
    /// Fade ramp duration in milliseconds.
    pub(crate) fade_duration_ms: u64,
    /// An in-progress fade ramp, advanced by the engine loop.
    pub(crate) active_fade: Option<ActiveFade>,
}

/// A track in our queue with its stream URL.
#[derive(Clone)]
pub(crate) struct QueuedTrack {
    pub(crate) track: Track,
    pub(crate) stream_url: String,
    /// Where this entry came from — see [`QueueSource`]. `PlayContext`
    /// (starting a new list) keeps any not-yet-played `UserQueued` entries
    /// ahead of the new context rather than discarding them, matching
    /// Spotify's "Next in queue" surviving a context change.
    pub(crate) source: QueueSource,
}

/// Where a [`QueuedTrack`] entry came from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueueSource {
    /// Part of a played context (`Play`/`PlayContext` — a Songs table, an
    /// album, a playlist, ...).
    Context,
    /// Explicitly queued by the user (`AddToQueue` — Play Next / Add to
    /// Queue), independent of whatever context is currently playing.
    UserQueued,
}

impl From<QueueSource> for QueueEntrySource {
    fn from(source: QueueSource) -> Self {
        match source {
            QueueSource::Context => QueueEntrySource::Context,
            QueueSource::UserQueued => QueueEntrySource::UserQueued,
        }
    }
}

/// An entry in mpv's playlist (for tracking gapless transitions).
#[derive(Clone)]
pub(crate) struct PlaylistEntry {
    pub(crate) track: Track,
    pub(crate) queue_position: usize,
    /// mpv's stable playlist entry id (from the loadfile response). Listen
    /// credits key off this: end-file(eof) events carry the id, so a track is
    /// credited exactly when ITS entry ended naturally, regardless of how
    /// property-change and structural events interleave on the socket.
    pub(crate) entry_id: Option<i64>,
}

impl Default for EngineState {
    fn default() -> Self {
        Self {
            current_track: None,
            queue: Vec::new(),
            queue_position: None,
            original_queue_order: Vec::new(),
            is_playing: false,
            volume: 1.0,
            shuffle: false,
            repeat: RepeatMode::Off,
            replay_gain: ReplayGainSettings::default(),
            eq_enabled: false,
            eq_bands: vec![0; 18],
            track_start_time: None,
            track_start_position_ms: 0,
            paused_position_ms: 0,
            output_device_name: None,
            mpv_playlist_tracks: VecDeque::new(),
            is_loading: false,
            last_finished_track_id: None,
            last_finished_at: None,
            mpv_playlist_base_pos: 0,
            consecutive_stream_failures: 0,
            retried_track_id: None,
            recently_popped: VecDeque::new(),
            recovery_due: None,
            mpv_respawn_due: false,
            idle_polls: 0,
            pending_seek_ms: None,
            listened_ms: 0,
            span_start_ms: 0,
            fade_enabled: true,
            fade_duration_ms: 400,
            active_fade: None,
        }
    }
}

impl EngineState {
    /// Emit TrackFinished only if not a duplicate of the last emission.
    /// Returns true if the event was emitted, false if suppressed as duplicate.
    ///
    /// `finished_at` is the wall-clock Unix timestamp (seconds) when the track
    /// finished playing. For single-track finishes this is `Utc::now()`. For
    /// gapless batches the caller reconstructs per-track timestamps from
    /// cumulative durations so each listen gets a distinct time.
    ///
    /// Design contract: an interrupted current track may be credited from
    /// `handle_playback_ended` (natural stop) or `play_track_at_position`
    /// (user starts something new). Those paths are mutually exclusive in
    /// practice; this dedup window is the backstop if they ever race.
    pub(crate) fn emit_track_finished<F>(
        &mut self,
        track: &Track,
        played_ms: u64,
        finished_at: i64,
        event_callback: &F,
    ) -> bool
    where
        F: Fn(AudioEvent),
    {
        // Suppress if same track emitted within 2 seconds
        if let (Some(ref last_id), Some(last_at)) =
            (&self.last_finished_track_id, self.last_finished_at)
        {
            if last_id == &track.jellyfin_id && last_at.elapsed().as_secs() < 2 {
                tracing::debug!("Suppressed duplicate TrackFinished for '{}'", track.name);
                return false;
            }
        }
        self.last_finished_track_id = Some(track.jellyfin_id.clone());
        self.last_finished_at = Some(Instant::now());
        event_callback(AudioEvent::TrackFinished {
            track: track.clone(),
            played_ms,
            finished_at,
        });
        true
    }

    pub(crate) fn current_position_ms(&self) -> u64 {
        if !self.is_playing {
            return self.paused_position_ms;
        }

        self.track_start_time
            .map(|start| {
                let elapsed = start.elapsed().as_millis() as u64;
                self.track_start_position_ms + elapsed
            })
            .unwrap_or(self.paused_position_ms)
    }

    pub(crate) fn duration_ms(&self) -> u64 {
        self.current_track
            .as_ref()
            .map(|t| t.duration_ms)
            .unwrap_or(0)
    }

    /// Fold the currently-open play span into `listened_ms` at the live
    /// position, then reopen a new span from there.
    ///
    /// Call immediately BEFORE a seek moves the position, so the audio actually
    /// heard up to the seek point is credited and the skipped-over (or
    /// re-heard) region is not.
    pub(crate) fn close_play_span(&mut self) {
        let pos = self.current_position_ms();
        self.listened_ms = self
            .listened_ms
            .saturating_add(pos.saturating_sub(self.span_start_ms));
        self.span_start_ms = pos;
    }

    /// Reset listened-time accounting for a new track, opening the first span at
    /// `start_ms` (0 for a normal play, or the resume offset).
    pub(crate) fn reset_listened(&mut self, start_ms: u64) {
        self.listened_ms = 0;
        self.span_start_ms = start_ms;
    }

    /// Total listened time when a track ends naturally at `end_ms`.
    ///
    /// Adds the final open span (`span_start_ms..end_ms`) to the accumulated
    /// closed spans. Capped at `end_ms` so a backward-seek re-listen can never
    /// credit more than 100% in a single scrobble — which also keeps an
    /// uninterrupted end-to-end play crediting exactly the full duration.
    pub(crate) fn listened_ms_to_end(&self, end_ms: u64) -> u64 {
        let final_span = end_ms.saturating_sub(self.span_start_ms);
        self.listened_ms.saturating_add(final_span).min(end_ms)
    }

    /// Calculate effective volume with ReplayGain adjustment.
    ///
    /// Uses industry-standard limiting:
    /// - Maximum gain boost capped at +6dB (matches Spotify, Apple Music, YouTube)
    /// - Soft limiting above 0dB to prevent harsh clipping
    /// - Tracks with extreme values (>+12dB) logged as outliers
    pub(crate) fn adjusted_volume(&self) -> f32 {
        // Industry standard: max boost of +6dB (matches Spotify, Apple Music, YouTube)
        const MAX_GAIN_BOOST_DB: f32 = 6.0;
        // Outlier threshold for logging (likely bad metadata)
        const OUTLIER_THRESHOLD_DB: f32 = 12.0;

        let track_name = self
            .current_track
            .as_ref()
            .map(|t| t.name.as_str())
            .unwrap_or("none");

        if !self.replay_gain.enabled {
            tracing::debug!(
                "ReplayGain disabled, using raw volume {:.0}% for '{}'",
                self.volume * 100.0,
                track_name
            );
            return self.volume;
        }

        let track_gain_db = self
            .current_track
            .as_ref()
            .and_then(|t| t.normalization_gain_db);

        let Some(gain_db) = track_gain_db else {
            tracing::debug!(
                "No normalization data for '{}', using raw volume {:.0}%",
                track_name,
                self.volume * 100.0
            );
            return self.volume;
        };

        if !gain_db.is_finite() {
            tracing::warn!(
                "Invalid normalization value for '{}', using raw volume",
                track_name
            );
            return self.volume;
        }

        // Log outliers (likely bad metadata from analyser)
        if gain_db > OUTLIER_THRESHOLD_DB {
            tracing::warn!(
                "Outlier normalization value {:.1}dB for '{}' (capping at +{}dB)",
                gain_db,
                track_name,
                MAX_GAIN_BOOST_DB
            );
        }

        // Calculate total gain with pre-amp, then clamp boost to industry standard
        let total_gain_db = gain_db + self.replay_gain.pre_amp_db;
        let clamped_gain_db = total_gain_db.min(MAX_GAIN_BOOST_DB);

        // Convert to linear gain
        let linear_gain = 10_f32.powf(clamped_gain_db / 20.0);
        let adjusted = self.volume * linear_gain;

        // Apply soft limiting if prevent_clipping is enabled
        // Uses a simple soft knee at 0dB (volume = 1.0)
        let final_volume = if self.replay_gain.prevent_clipping && adjusted > 1.0 {
            // Soft limiting: compress values above 1.0 using sqrt curve
            // This provides gentler limiting than hard clipping
            // At 2.0 input -> ~1.41 output, at 4.0 input -> ~2.0 output
            1.0 + (adjusted - 1.0).sqrt() * 0.5
        } else {
            adjusted
        }
        .min(2.0); // Hard ceiling at 200% to prevent extreme values

        tracing::debug!(
            "ReplayGain for '{}': gain={:.1}dB (clamped={:.1}dB), preamp={:.1}dB, volume {:.0}% -> {:.0}%",
            track_name,
            gain_db,
            clamped_gain_db,
            self.replay_gain.pre_amp_db,
            self.volume * 100.0,
            final_volume * 100.0
        );

        final_volume
    }
}

/// Abort any in-progress fade and restore mpv's volume to the current base
/// (ReplayGain-adjusted) level.
///
/// Called by any user intent (play, skip, seek, resume) and by the event paths
/// that supersede a fade (gapless transition, natural end, stream failure), so
/// an interrupted ramp can never strand playback muted or leave a deferred
/// pause/stop pointing at the wrong track.
pub(crate) fn cancel_fade(state: &mut EngineState, ipc: &MpvIpc) {
    if state.active_fade.take().is_some() {
        let base = (state.adjusted_volume() * 100.0) as i64;
        let _ = ipc.set_property("volume", Value::Number(base.into()));
    }
}

/// Advance the active fade by one step, driven once per engine-loop iteration.
/// Mirrors the loop-driven, directly-callable shape of
/// `attempt_stream_recovery`.
///
/// The base volume is recomputed from `adjusted_volume()` each tick so a
/// concurrent `SetVolume`/`SetReplayGain` self-corrects within the ramp. A
/// `duration_ms` of 0 completes instantly. On completion the fade finalises per
/// kind (PauseOut pauses mpv, ResumeIn just restores volume, StopOut stops mpv
/// and clears playback state) and always restores the base volume so the next
/// track starts at full level.
pub(crate) fn advance_fade<F>(state: &mut EngineState, ipc: &MpvIpc, event_callback: &F)
where
    F: Fn(AudioEvent),
{
    let Some(fade) = state.active_fade.as_ref() else {
        return;
    };
    let kind = fade.kind;
    let progress = if fade.duration_ms == 0 {
        1.0
    } else {
        (fade.start.elapsed().as_millis() as f64 / fade.duration_ms as f64).clamp(0.0, 1.0)
    };

    let base = state.adjusted_volume() as f64 * 100.0;

    if progress < 1.0 {
        let factor = match kind {
            FadeKind::PauseOut | FadeKind::StopOut => 1.0 - progress,
            FadeKind::ResumeIn => progress,
        };
        let vol = (base * factor) as i64;
        let _ = ipc.set_property("volume", Value::Number(vol.into()));
        return;
    }

    // Ramp complete — finalise and restore the base volume.
    let base_i = base as i64;
    match kind {
        FadeKind::PauseOut => {
            let _ = ipc.set_property("pause", Value::Bool(true));
            let _ = ipc.set_property("volume", Value::Number(base_i.into()));
        }
        FadeKind::ResumeIn => {
            let _ = ipc.set_property("volume", Value::Number(base_i.into()));
        }
        FadeKind::StopOut => {
            let _ = ipc.command(vec![Value::String("stop".to_string())]);
            state.is_playing = false;
            state.current_track = None;
            state.track_start_time = None;
            state.paused_position_ms = 0;
            state.mpv_playlist_tracks.clear();
            state.recovery_due = None;
            let _ = ipc.set_property("volume", Value::Number(base_i.into()));
            emit_state_changed(state, event_callback);
        }
    }
    state.active_fade = None;
}

/// Honour a user Pause/Stop that was still fading out when a gapless transition
/// or natural end superseded (and cancelled) the fade.
///
/// The fade defers the actual mpv pause/stop until the ramp finishes, so if the
/// track ends inside that window the transition would otherwise drop the intent
/// and leave the *next* track audibly playing while the UI shows paused/stopped.
/// This re-applies the intent to the post-transition state. Volume was already
/// restored to base by `cancel_fade`; `kind` is the fade captured before it.
pub(crate) fn apply_superseded_intent<F>(
    state: &mut EngineState,
    ipc: &MpvIpc,
    event_callback: &F,
    kind: Option<FadeKind>,
) where
    F: Fn(AudioEvent),
{
    match kind {
        Some(FadeKind::PauseOut) if state.current_track.is_some() => {
            // Read position before flipping the playing flags (ordering).
            state.paused_position_ms = state.current_position_ms();
            state.track_start_time = None;
            state.is_playing = false;
            let _ = ipc.set_property("pause", Value::Bool(true));
            emit_state_changed(state, event_callback);
        }
        Some(FadeKind::StopOut) if state.current_track.is_some() => {
            let _ = ipc.command(vec![Value::String("stop".to_string())]);
            state.is_playing = false;
            state.current_track = None;
            state.track_start_time = None;
            state.paused_position_ms = 0;
            state.mpv_playlist_tracks.clear();
            state.recovery_due = None;
            emit_state_changed(state, event_callback);
        }
        _ => {}
    }
}

/// Start the audio engine with mpv backend.
///
/// Returns a handle for sending commands; the engine thread runs detached.
pub fn start_audio_engine<F>(event_callback: F) -> Result<AudioEngineHandle, std::io::Error>
where
    F: Fn(AudioEvent) + Send + 'static,
{
    let (command_tx, command_rx) = mpsc::channel();
    let is_running = Arc::new(AtomicBool::new(true));
    let is_running_clone = Arc::clone(&is_running);

    thread::Builder::new()
        .name("audio-engine".to_string())
        .spawn(move || {
            run_audio_engine(command_rx, event_callback, is_running_clone);
        })?;

    let command_handle = AudioEngineHandle {
        command_tx,
        is_running,
    };

    Ok(command_handle)
}

/// Main audio engine loop.
fn run_audio_engine<F>(
    command_rx: Receiver<AudioCommand>,
    event_callback: F,
    is_running: Arc<AtomicBool>,
) where
    F: Fn(AudioEvent) + Send + 'static,
{
    tracing::info!("Audio engine starting (mpv backend)");

    // Reap any mpv orphaned by a previous bragi that died abruptly (before
    // PDEATHSIG could act, or on a platform without it) so they cannot pile up.
    super::mpv_process::reap_orphaned_mpv();

    let socket_path = get_socket_path();
    let mut mpv_process: Option<Child> = None;
    let mut mpv: Option<MpvIpc> = None;
    let mut state = EngineState::default();
    let mut last_progress_update = Instant::now();

    // Try to start mpv
    match spawn_mpv(&socket_path, None) {
        Ok(child) => {
            mpv_process = Some(child);
            match connect_to_mpv(&socket_path) {
                Ok(ipc) => {
                    // Set up property observers
                    if let Err(e) = setup_property_observers(&ipc) {
                        tracing::error!("Failed to setup mpv observers: {}", e);
                    }
                    mpv = Some(ipc);
                    tracing::info!("mpv connected successfully");
                    // Re-apply EQ on the fresh connection (af is per-process).
                    if let Some(ref ipc) = mpv {
                        apply_eq(&state, ipc);
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to connect to mpv: {}", e);
                    // Kill orphaned mpv process
                    if let Some(mut child) = mpv_process.take() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    event_callback(AudioEvent::Error {
                        message: format!("Failed to connect to mpv: {}", e),
                    });
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to start mpv: {}", e);
            event_callback(AudioEvent::Error {
                message: format!("Failed to start mpv: {}", e),
            });
        }
    }

    loop {
        // Process mpv events
        if let Some(ref ipc) = mpv {
            while let Some(event) = ipc.try_recv_event() {
                handle_mpv_event(&mut state, event, &event_callback, ipc);
            }
        }

        // Fire a scheduled stream-failure recovery once its delay elapses. An
        // audio-output failure is recovered by a fresh mpv instead, whose
        // resume of the current track is the retry; with no recovery pending
        // (three strikes, or a new play cancelled it) it is replaced at once,
        // so the next play does not meet the same broken process.
        let recovery_fired = state.recovery_due.is_some_and(|due| Instant::now() >= due);
        if recovery_fired {
            state.recovery_due = None;
        }
        if state.mpv_respawn_due && state.recovery_due.is_none() {
            state.mpv_respawn_due = false;
            tracing::warn!(target: super::TARGET, "Replacing mpv after an audio output failure");
            restart_mpv(
                &mut mpv,
                &mut mpv_process,
                &socket_path,
                &mut state,
                &event_callback,
            );
        } else if recovery_fired {
            if let Some(ref ipc) = mpv {
                super::mpv_events::attempt_stream_recovery(&mut state, &event_callback, ipc);
            }
        }

        // Advance an in-progress pause/stop/resume volume fade.
        if state.active_fade.is_some() {
            if let Some(ref ipc) = mpv {
                advance_fade(&mut state, ipc, &event_callback);
            }
        }

        // Send progress updates during playback (but not while loading)
        if state.is_playing
            && !state.is_loading
            && last_progress_update.elapsed() >= PROGRESS_UPDATE_INTERVAL
        {
            // Get current position from mpv. Guarded on is_connected(): a
            // known-dead connection must not block this once-a-second poll
            // for a full response_timeout on every single loop iteration —
            // ensure_mpv_running (run from the next command) is what
            // actually respawns it.
            if let Some(ref ipc) = mpv {
                if ipc.is_connected() {
                    super::mpv_events::reconcile_idle_playback(&mut state, &event_callback, ipc);
                }
            }
            if let Some(ref ipc) = mpv {
                if ipc.is_connected() {
                    if let Ok(pos) = ipc.get_property_typed::<f64>("time-pos") {
                        let pos_ms = (pos * 1000.0) as u64;
                        state.track_start_position_ms = pos_ms;
                        state.track_start_time = Some(Instant::now());
                    }
                }
            }

            event_callback(AudioEvent::Progress {
                current_time_ms: state.current_position_ms(),
                duration_ms: state.duration_ms(),
            });
            last_progress_update = Instant::now();
        }

        // Process commands (with timeout for event processing). Poll faster
        // while a fade is ramping so the volume steps stay smooth.
        let timeout = Duration::from_millis(if state.active_fade.is_some() { 20 } else { 50 });
        match command_rx.recv_timeout(timeout) {
            Ok(command) => {
                tracing::info!(
                    "Audio engine received command: {:?}",
                    std::mem::discriminant(&command)
                );
                handle_command(
                    command,
                    &mut state,
                    &mut mpv,
                    &mut mpv_process,
                    &socket_path,
                    &event_callback,
                    &is_running,
                );

                if !is_running.load(Ordering::SeqCst) {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Continue loop for event processing
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                tracing::info!("Audio engine command channel disconnected, shutting down");
                is_running.store(false, Ordering::SeqCst);
                break;
            }
        }
    }

    // Cleanup
    if let Some(ref ipc) = mpv {
        let _ = ipc.command(vec![Value::String("quit".to_string())]);
    }
    if let Some(mut child) = mpv_process {
        let _ = child.wait();
    }
    let _ = std::fs::remove_file(&socket_path);

    tracing::info!("Audio engine stopped");
}

/// Build a snapshot of the current playback state.
pub(crate) fn snapshot_playback_state(state: &EngineState) -> crate::models::PlaybackState {
    crate::models::PlaybackState {
        current_track: state.current_track.clone(),
        is_playing: state.is_playing,
        current_time_ms: state.current_position_ms(),
        duration_ms: state.duration_ms(),
        volume: state.volume,
        shuffle: state.shuffle,
        repeat: state.repeat,
    }
}

/// Build a snapshot of the current queue state.
pub(crate) fn snapshot_queue_state(state: &EngineState) -> crate::models::QueueState {
    crate::models::QueueState {
        tracks: state.queue.iter().map(|q| q.track.clone()).collect(),
        current_index: state.queue_position.map_or(-1, |pos| pos as i32),
        sources: state.queue.iter().map(|q| q.source.into()).collect(),
    }
}

/// Emit a state changed event.
pub(crate) fn emit_state_changed<F>(state: &EngineState, event_callback: &F)
where
    F: Fn(AudioEvent),
{
    event_callback(AudioEvent::StateChanged {
        current_track: state.current_track.clone(),
        is_playing: state.is_playing,
        current_time_ms: state.current_position_ms(),
        duration_ms: state.duration_ms(),
        volume: state.volume,
        shuffle: state.shuffle,
        repeat: state.repeat,
        output_device: state.output_device_name.clone(),
    });
}

/// Emit a queue changed event.
pub(crate) fn emit_queue_changed<F>(state: &EngineState, event_callback: &F)
where
    F: Fn(AudioEvent),
{
    event_callback(AudioEvent::QueueChanged {
        tracks: state.queue.iter().map(|q| q.track.clone()).collect(),
        current_index: state.queue_position.map_or(-1, |pos| pos as i32),
        sources: state.queue.iter().map(|q| q.source.into()).collect(),
    });
}
