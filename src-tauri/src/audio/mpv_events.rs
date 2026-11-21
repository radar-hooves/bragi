//! mpv event handling.
//!
//! Processes events received from the mpv IPC connection and updates engine state.

use std::time::Instant;

use serde_json::Value;

use super::mpv_engine::{
    apply_superseded_intent, cancel_fade, emit_queue_changed, emit_state_changed, EngineState,
};
use super::mpv_ipc::{MpvEvent, MpvIpc};
use super::mpv_playback::{play_track, queue_next_tracks_in_mpv};
use super::types::{AudioEvent, RepeatMode};

/// Handle events from mpv.
pub(crate) fn handle_mpv_event<F>(
    state: &mut EngineState,
    event: MpvEvent,
    event_callback: &F,
    ipc: &MpvIpc,
) where
    F: Fn(AudioEvent),
{
    match event {
        MpvEvent::PropertyChange { name, data } => {
            match name.as_str() {
                "time-pos" => {
                    // Frequent updates - only log at trace level
                    if let Some(pos) = data.as_f64() {
                        let pos_ms = (pos * 1000.0) as u64;
                        state.track_start_position_ms = pos_ms;
                        state.track_start_time = Some(Instant::now());
                    }
                }
                "pause" => {
                    if let Some(paused) = data.as_bool() {
                        // Ignore pause events when no track is loaded (mpv delivers
                        // the initial pause=false on observer registration at startup).
                        if state.current_track.is_none() {
                            return;
                        }
                        // Ignore pause noise while a failure recovery is
                        // pending: a stale pause=false from the failed load
                        // would flag playback as active and veto the recovery.
                        if state.recovery_due.is_some() {
                            tracing::debug!("Ignoring pause event during failure recovery window");
                            return;
                        }

                        let was_playing = state.is_playing;
                        state.is_playing = !paused;

                        // Skip the pause/resume bookkeeping while a fade
                        // owns the transition — the fade command already set it
                        // up front. Belt-and-suspenders: with the early
                        // is_playing flip `was_playing` already makes both
                        // branches no-ops, but the guard keeps intent explicit.
                        if state.active_fade.is_none() {
                            if paused && was_playing {
                                // Transitioning to paused
                                state.paused_position_ms = state.current_position_ms();
                                state.track_start_time = None;
                            } else if !paused && !was_playing {
                                // Transitioning to playing
                                state.track_start_time = Some(Instant::now());
                                state.track_start_position_ms = state.paused_position_ms;
                            }
                        }

                        emit_state_changed(state, event_callback);
                    }
                }
                "playlist-pos" => {
                    // Track changed in mpv's playlist (gapless transition).
                    // Consult the LIVE value rather than trusting the event:
                    // when a loadfile/stop rebuilds the playlist (error
                    // recovery, manual play), an already-queued playlist-pos
                    // event describes the OLD playlist and would pop rebuilt
                    // entries as "finished" — a spurious full-length scrobble
                    // for a track that never played.
                    if let Some(event_pos) = data.as_i64() {
                        if event_pos >= 0 {
                            let live_pos = ipc
                                .get_property_typed::<i64>("playlist-pos")
                                .unwrap_or(event_pos);
                            if live_pos >= 0 {
                                handle_playlist_position_change(
                                    state,
                                    live_pos as usize,
                                    event_callback,
                                    ipc,
                                );
                            } else {
                                tracing::debug!(
                                    "Ignoring stale playlist-pos={} (mpv is idle now)",
                                    event_pos
                                );
                            }
                        }
                    }
                }
                "duration" => {
                    // Duration available - could update track metadata if needed
                }
                "idle-active" if data.as_bool() == Some(true) && state.current_track.is_some() => {
                    // mpv is idle but we had a track — playback ended. Two
                    // guards protect against STALE idle-active events (the
                    // initial observer value delivered after an mpv restart
                    // or device change races the resume and used to advance
                    // the queue spuriously — the "random skip"):
                    // 1. a loadfile is already in flight (is_loading), or
                    // 2. a live read says mpv is not actually idle any more.
                    if state.is_loading {
                        tracing::debug!("Ignoring idle-active while a load is in flight");
                    } else if state.recovery_due.is_some() {
                        // The idleness is the aftermath of our own stop during
                        // failure handling, not a natural end-of-queue.
                        tracing::debug!("Ignoring idle-active during failure recovery window");
                    } else if !ipc
                        .get_property_typed::<bool>("idle-active")
                        .unwrap_or(true)
                    {
                        tracing::debug!("Ignoring stale idle-active (mpv is live again)");
                    } else {
                        handle_playback_ended(state, event_callback, ipc);
                    }
                }
                _ => {}
            }
        }
        MpvEvent::EndFile {
            reason,
            playlist_entry_id,
            file_error,
        } => {
            tracing::warn!(target: super::TARGET,
                "mpv end-file: reason={}, playlist_entry_id={:?}, error={:?}",
                reason,
                playlist_entry_id,
                file_error
            );
            if reason == "eof" {
                // A track finished NATURALLY. This event is the single source
                // of listen credits: it carries the playlist entry id, so the
                // right track is credited no matter how property-change and
                // structural events interleave (an errored entry never gets a
                // credit because its reason is "error", not "eof").
                credit_finished_entry(state, playlist_entry_id, event_callback);
            } else if reason == "error" {
                let error_msg = file_error.unwrap_or_else(|| "Unknown playback error".to_string());
                // Only the CURRENT front entry's failure drives recovery.
                // Errors for other entries are stale (a playlist rebuild or an
                // mpv auto-advance already superseded them); acting on them
                // burnt the retry budget on ghosts and aborted playback that
                // mpv had already recovered by itself.
                let front_id = state.mpv_playlist_tracks.front().and_then(|e| e.entry_id);
                let is_current_failure = match (playlist_entry_id, front_id) {
                    (Some(ev), Some(front)) => ev == front,
                    // Without ids (older mpv) there is nothing to compare;
                    // treat as current.
                    _ => true,
                };
                if is_current_failure {
                    tracing::error!(target: super::TARGET, "mpv playback error: {}", error_msg);
                    handle_stream_failure(state, &error_msg, event_callback, ipc);
                } else {
                    tracing::warn!(target: super::TARGET,
                        "Ignoring error for superseded playlist entry {:?} (current front {:?}): {}",
                        playlist_entry_id,
                        front_id,
                        error_msg
                    );
                }
            }
        }
        MpvEvent::StartFile { playlist_entry_id } => {
            tracing::info!("mpv start-file: playlist_entry_id={:?}", playlist_entry_id);
            state.track_start_time = Some(Instant::now());
            state.track_start_position_ms = 0;
            state.paused_position_ms = 0;
            // New track: start listened-time accounting from 0. Note a
            // track resumed at an offset (play_track_at_position with a non-zero
            // start) is re-zeroed here, so such a resume credits from 0 — a
            // fail-safe over-credit, not an under-credit.
            state.reset_listened(0);
        }
        MpvEvent::PlaybackRestart => {
            // Seek completed or playback started - safe to emit progress now
            tracing::info!("mpv playback-restart (seek completed or playback started)");
            state.is_loading = false;
            state.track_start_time = Some(Instant::now());
            // A track is actually playing again: the stream-failure recovery
            // budget resets.
            state.consecutive_stream_failures = 0;
            state.retried_track_id = None;

            // Apply a seek that arrived while the file was still loading.
            if let Some(target_ms) = state.pending_seek_ms.take() {
                tracing::info!("Applying deferred seek to {}ms", target_ms);
                super::mpv_commands::handle_seek(state, target_ms, event_callback, ipc);
            }
        }
        MpvEvent::Idle => {
            tracing::info!("mpv idle event");
        }
        MpvEvent::Unknown(name) => {
            tracing::debug!("Unknown mpv event: {}", name);
        }
    }
}

/// Handle playlist position change (gapless track transition).
fn handle_playlist_position_change<F>(
    state: &mut EngineState,
    mpv_pos: usize,
    event_callback: &F,
    ipc: &MpvIpc,
) where
    F: Fn(AudioEvent),
{
    // A gapless transition supersedes any in-progress fade: if a PauseOut /
    // StopOut ramp is active when the fading track ends naturally, clear it so
    // the next advance_fade tick can't fade out / stop the WRONG (new) track.
    // Restores volume to base before the handler re-sets it below. Capture
    // the intent first so a user Pause/Stop caught mid-fade is still honoured on
    // the incoming track rather than dropped (the next track playing while the
    // UI shows paused).
    let superseded_fade = state.active_fade.as_ref().map(|f| f.kind);
    cancel_fade(state, ipc);

    // mpv_pos is the ABSOLUTE 0-indexed position in mpv's playlist.
    // mpv_playlist_base_pos tracks the absolute mpv position of mpv_playlist_tracks[0],
    // which shifts as gapless transitions pop the front of our tracking queue.
    // tracks_to_pop is relative: how many entries to remove from the front.

    if state.mpv_playlist_tracks.is_empty() {
        return;
    }

    let tracks_to_pop = mpv_pos.saturating_sub(state.mpv_playlist_base_pos);

    if tracks_to_pop == 0 {
        // No change, still on same track
        return;
    }

    state.mpv_playlist_base_pos += tracks_to_pop;

    // Pop passed entries — bookkeeping only. Listen credits are emitted by
    // the end-file(eof) handler (keyed by entry id), never here: an entry can
    // leave the front of mpv's playlist because it ERRORED, and crediting on
    // position change alone gave phantom full-length scrobbles for tracks
    // that never played. Popped entries are parked so an eof event that
    // arrives after its position change can still find its track.
    for i in 0..tracks_to_pop {
        if let Some(finished) = state.mpv_playlist_tracks.pop_front() {
            if let Some(id) = finished.entry_id {
                // Only the first-popped entry was the accounted current track;
                // capture its seek-aware listened time BEFORE reset_listened
                // below wipes the accumulator, so a pos-change-before-eof
                // ordering still credits actual listened time, not full
                // duration.
                let credited = if i == 0 {
                    Some(state.listened_ms_to_end(finished.track.duration_ms))
                } else {
                    None
                };
                state
                    .recently_popped
                    .push_back((id, finished.track, credited));
                while state.recently_popped.len() > 8 {
                    state.recently_popped.pop_front();
                }
            }
        }
    }

    // Update current track from the front of the queue
    if let Some(entry) = state.mpv_playlist_tracks.front() {
        state.current_track = Some(entry.track.clone());
        state.queue_position = Some(entry.queue_position);
        state.track_start_time = Some(Instant::now());
        state.track_start_position_ms = 0;
        state.paused_position_ms = 0;
        // Gapless transition to the next track: reset listened-time accounting
        // for the incoming track. The outgoing track's seek-aware credit
        // was already parked in recently_popped above.
        state.reset_listened(0);

        // Re-apply volume with ReplayGain adjustment for the new track
        let mpv_volume = (state.adjusted_volume() * 100.0) as i64;
        if let Err(e) = ipc.set_property("volume", Value::Number(mpv_volume.into())) {
            tracing::warn!("Failed to set volume on track change: {}", e);
        }

        emit_state_changed(state, event_callback);
        emit_queue_changed(state, event_callback);

        // Pre-buffer next upcoming tracks for continued gapless playback
        queue_next_tracks_in_mpv(state, ipc);

        // Honour a Pause/Stop that was mid-fade when this transition fired, so
        // the incoming track is paused/stopped rather than left playing.
        apply_superseded_intent(state, ipc, event_callback, superseded_fade);
    }
}

/// Credit a listen for a playlist entry that reached its natural end.
///
/// Resolution order: the live tracking queue (eof arrived before the position
/// change), then the recently-popped park (eof arrived after), then — for an
/// mpv that did not report an entry id — the front tracking entry. The 2 s
/// dedup guard in `emit_track_finished` absorbs any overlap with the manual
/// interrupted-track credit paths.
fn credit_finished_entry<F>(
    state: &mut EngineState,
    playlist_entry_id: Option<i64>,
    event_callback: &F,
) where
    F: Fn(AudioEvent),
{
    // Resolve to (track, parked listened_ms). The parked value is only present
    // when the entry was popped by a gapless position change before its eof
    // arrived; using it preserves the seek-aware credit that reset_listened
    // would otherwise have wiped.
    let resolved = match playlist_entry_id {
        Some(id) => state
            .mpv_playlist_tracks
            .iter()
            .find(|e| e.entry_id == Some(id))
            .map(|e| (e.track.clone(), None))
            .or_else(|| {
                state
                    .recently_popped
                    .iter()
                    .find(|(popped_id, _, _)| *popped_id == id)
                    .map(|(_, t, credit)| (t.clone(), *credit))
            }),
        // No entry id (old mpv): credit the front entry — unless a load is
        // pending, when the front is a freshly rebuilt playlist and a late
        // eof from the replaced track would credit the wrong one.
        None if !state.is_loading => state
            .mpv_playlist_tracks
            .front()
            .map(|e| (e.track.clone(), None)),
        None => None,
    };

    let Some((track, parked)) = resolved else {
        tracing::debug!(
            "end-file(eof) for unknown playlist entry {:?}; no credit",
            playlist_entry_id
        );
        return;
    };

    // Credit actual listened time, not full duration. When the entry is
    // still live (eof before the position change), compute from the current
    // accounting; when it was already popped by a gapless transition, use the
    // seek-aware value parked at pop time. End-to-end play credits full duration
    // (span from 0); a seek-to-tail credits only the spans actually heard.
    let played_ms = parked.unwrap_or_else(|| state.listened_ms_to_end(track.duration_ms));
    let finished_at = chrono::Utc::now().timestamp();
    state.emit_track_finished(&track, played_ms, finished_at, event_callback);
}

/// Recover from a missed end-of-playback: the engine thinks it is playing but
/// mpv has nothing loaded, so the UI clock would extrapolate over silence
/// forever. `idle-active` is edge-triggered and every guard on it can swallow
/// the one event; this level check, run from the progress poll, needs two
/// consecutive idle readings (a transition between entries is momentary) and
/// then ends playback the same way the event would have.
pub(crate) fn reconcile_idle_playback<F>(state: &mut EngineState, event_callback: &F, ipc: &MpvIpc)
where
    F: Fn(AudioEvent),
{
    let expecting_playback = state.is_playing
        && !state.is_loading
        && state.current_track.is_some()
        && state.recovery_due.is_none()
        && !state.mpv_respawn_due;
    let idle = expecting_playback
        && ipc
            .get_property_typed::<bool>("idle-active")
            .unwrap_or(false);
    if !idle {
        state.idle_polls = 0;
        return;
    }
    state.idle_polls += 1;
    if state.idle_polls < 2 {
        return;
    }
    state.idle_polls = 0;
    tracing::warn!(target: super::TARGET, "mpv is idle while playback is expected; advancing");
    handle_playback_ended(state, event_callback, ipc);
}

/// Handle playback ended (queue exhausted or stopped).
fn handle_playback_ended<F>(state: &mut EngineState, event_callback: &F, ipc: &MpvIpc)
where
    F: Fn(AudioEvent),
{
    // A natural end supersedes any in-progress fade so a deferred pause/stop
    // can't strand active_fade on the track we're about to advance to.
    // Capture the intent first so a user Pause/Stop caught mid-fade is honoured
    // on the track we advance to, rather than dropped.
    let superseded_fade = state.active_fade.as_ref().map(|f| f.kind);
    cancel_fade(state, ipc);

    // Check if we should advance to next track
    if let Some(next_pos) = resolve_next_queue_position(state) {
        if let Some(queued) = state.queue.get(next_pos).cloned() {
            state.queue_position = Some(next_pos);
            play_track(
                state,
                &queued.track,
                &queued.stream_url,
                event_callback,
                ipc,
            );
            // Honour a Pause/Stop caught mid-fade: pause/stop the advanced-to
            // track instead of letting it play under a paused/stopped UI.
            apply_superseded_intent(state, ipc, event_callback, superseded_fade);
            return;
        }
    }

    // Emit TrackFinished for the last track before stopping. Credit listened
    // time; both natural-end paths must agree because emit_track_finished
    // dedups within 2s and whichever fires first wins.
    if let Some(track) = state.current_track.clone() {
        let dur = state.duration_ms();
        let played_ms = state.listened_ms_to_end(dur);
        let finished_at = chrono::Utc::now().timestamp();
        state.emit_track_finished(&track, played_ms, finished_at, event_callback);
    }

    // No more tracks, stop
    state.is_playing = false;
    state.current_track = None;
    state.mpv_playlist_tracks.clear();
    emit_state_changed(state, event_callback);
}

/// Recover from a stream-open/playback failure: retry the failed track once,
/// then advance through the queue, and stop with an error after three
/// consecutive failures (a systemic fault — wrong device, dead network —
/// should not silently walk the whole queue).
fn handle_stream_failure<F>(
    state: &mut EngineState,
    error_msg: &str,
    event_callback: &F,
    ipc: &MpvIpc,
) where
    F: Fn(AudioEvent),
{
    telemetry::report_error_with_cause(
        "playback error",
        &crate::error::MessageError(error_msg.to_string()),
    );

    // The output, not the stream, failed: no retry on this mpv can succeed.
    if error_msg == AO_INIT_FAILED {
        state.mpv_respawn_due = true;
    }

    // A stream failure supersedes any in-progress fade; restore volume to base
    // so a fault mid-fade can't leave playback silent after recovery.
    cancel_fade(state, ipc);

    // Credit a partial listen when the failure interrupted real playback
    // (e.g. the network died mid-track). A track still in its loading phase
    // never played — the extrapolated position is meaningless for it.
    if state.is_playing && !state.is_loading && state.current_position_ms() > 1000 {
        if let Some(current) = state.current_track.clone() {
            let played_ms = state.current_position_ms();
            let finished_at = chrono::Utc::now().timestamp();
            state.emit_track_finished(&current, played_ms, finished_at, event_callback);
        }
    }

    // Take control back from mpv before it error-cascades through the
    // pre-buffered entries (each would emit its own end-file/error).
    let _ = ipc.command(vec![Value::String("stop".to_string())]);
    state.mpv_playlist_tracks.clear();
    state.mpv_playlist_base_pos = 0;
    state.is_playing = false;
    state.is_loading = false;
    state.track_start_time = None;

    state.consecutive_stream_failures += 1;
    if state.consecutive_stream_failures >= 3 {
        tracing::error!(target: super::TARGET,
            "Stopping playback after {} consecutive stream failures",
            state.consecutive_stream_failures
        );
        state.is_playing = false;
        state.current_track = None;
        state.consecutive_stream_failures = 0;
        state.retried_track_id = None;
        state.recovery_due = None;
        event_callback(AudioEvent::Error {
            message: format!("Playback stopped after repeated failures: {}", error_msg),
        });
        emit_state_changed(state, event_callback);
        return;
    }

    event_callback(AudioEvent::Error {
        message: format!("Playback error: {}", error_msg),
    });
    emit_state_changed(state, event_callback);

    // Defer the retry/advance so a transient fault has time to clear (an
    // immediate retry of a source that fails in milliseconds would burn the
    // whole budget instantly). The engine loop fires it.
    state.recovery_due = Some(Instant::now() + STREAM_RECOVERY_DELAY);
    tracing::warn!(target: super::TARGET,
        "Stream failure {}; recovery scheduled in {:?}",
        state.consecutive_stream_failures,
        STREAM_RECOVERY_DELAY
    );
}

/// mpv's end-file error for `MPV_ERROR_AO_INIT_FAILED`.
const AO_INIT_FAILED: &str = "audio output initialization failed";

/// Delay between a stream failure and the recovery attempt.
const STREAM_RECOVERY_DELAY: std::time::Duration = std::time::Duration::from_millis(1200);

/// Execute a scheduled stream-failure recovery: retry the failed track once,
/// then advance through the queue. Called from the engine loop when
/// `recovery_due` elapses; aborts silently if the user started something else
/// in the meantime.
pub(crate) fn attempt_stream_recovery<F>(state: &mut EngineState, event_callback: &F, ipc: &MpvIpc)
where
    F: Fn(AudioEvent),
{
    // A recovery restarts playback from scratch; drop any lingering fade so it
    // can't stay stuck on the failed track.
    cancel_fade(state, ipc);

    // New user intent supersedes recovery.
    if state.is_playing || state.is_loading {
        tracing::warn!(target: super::TARGET,
            "Skipping stream recovery; playback active again (is_playing={}, is_loading={})",
            state.is_playing,
            state.is_loading
        );
        return;
    }
    let Some(current_id) = state.current_track.as_ref().map(|t| t.jellyfin_id.clone()) else {
        tracing::warn!(target: super::TARGET, "Skipping stream recovery; no current track");
        return;
    };

    // Retry the same track once before giving up on it.
    let already_retried = state.retried_track_id.as_deref() == Some(current_id.as_str());
    if !already_retried {
        if let Some(queued) = state
            .queue_position
            .and_then(|pos| state.queue.get(pos))
            .cloned()
        {
            tracing::warn!("Retrying track after stream failure: {}", queued.track.name);
            state.retried_track_id = Some(current_id);
            play_track(
                state,
                &queued.track,
                &queued.stream_url,
                event_callback,
                ipc,
            );
            return;
        }
    }

    // Advance to the next queue entry.
    if let Some(next_pos) = resolve_next_queue_position(state) {
        // Under repeat-one "next" is the same track again; it already had its
        // retry, so stop instead of looping on a dead stream.
        if Some(next_pos) != state.queue_position {
            if let Some(queued) = state.queue.get(next_pos).cloned() {
                tracing::warn!(target: super::TARGET, "Advancing past failed track to queue position {}", next_pos);
                state.retried_track_id = None;
                state.queue_position = Some(next_pos);
                play_track(
                    state,
                    &queued.track,
                    &queued.stream_url,
                    event_callback,
                    ipc,
                );
                return;
            }
        }
    }

    // Nothing left to try.
    state.is_playing = false;
    state.current_track = None;
    emit_state_changed(state, event_callback);
}

/// Resolve the next queue position considering repeat mode.
pub(crate) fn resolve_next_queue_position(state: &EngineState) -> Option<usize> {
    let pos = state.queue_position?;
    if state.repeat == RepeatMode::One {
        return Some(pos);
    }
    let next = pos + 1;
    if next < state.queue.len() {
        Some(next)
    } else if state.repeat == RepeatMode::All && !state.queue.is_empty() {
        Some(0)
    } else {
        None
    }
}

/// Resolve the next queue position for an EXPLICIT skip.
///
/// A deliberate next always advances — repeat-one applies to natural track
/// ends, not to the user pressing next (matching iTunes/Apple Music).
pub(crate) fn resolve_manual_next_position(state: &EngineState) -> Option<usize> {
    let pos = state.queue_position?;
    let next = pos + 1;
    if next < state.queue.len() {
        Some(next)
    } else if state.repeat == RepeatMode::All && !state.queue.is_empty() {
        Some(0)
    } else {
        None
    }
}
