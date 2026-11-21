//! Command handling for the mpv audio engine.
//!
//! Processes `AudioCommand` variants received from the Tauri frontend.

use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;

use super::mpv_engine::{
    cancel_fade, emit_queue_changed, emit_state_changed, snapshot_playback_state,
    snapshot_queue_state, ActiveFade, EngineState, FadeKind, QueueSource, QueuedTrack,
};
use super::mpv_events::resolve_manual_next_position;
use super::mpv_ipc::MpvIpc;
use super::mpv_playback::{
    handle_set_output_device, handle_set_shuffle, invalidate_and_requeue_mpv, play_track,
    play_track_at_position, shuffle_queue_tail,
};
use super::mpv_process::{connect_to_mpv, setup_property_observers, spawn_mpv};
use super::types::{AudioCommand, AudioEvent, QueueInsertPosition, RepeatMode};

/// Handle a command from the Tauri frontend.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_command<F>(
    command: AudioCommand,
    state: &mut EngineState,
    mpv: &mut Option<MpvIpc>,
    mpv_process: &mut Option<Child>,
    socket_path: &std::path::PathBuf,
    event_callback: &F,
    is_running: &Arc<AtomicBool>,
) where
    F: Fn(AudioEvent),
{
    // Handle QuerySnapshot before ensure_mpv_running: a state read must work
    // even when mpv is not running (e.g. before first playback). When mpv is
    // playing, refresh the position from mpv itself first so the snapshot
    // reflects real playback progress, not the Instant-based extrapolation.
    if let AudioCommand::QuerySnapshot(tx) = command {
        if state.is_playing && !state.is_loading {
            // is_connected() guard: a snapshot read must never block on a
            // known-dead connection's response_timeout — it answers from the
            // Instant-based extrapolation instead, same as before mpv ever
            // connected.
            if let Some(ref ipc) = mpv {
                if ipc.is_connected() {
                    if let Ok(pos) = ipc.get_property_typed::<f64>("time-pos") {
                        state.track_start_position_ms = (pos * 1000.0) as u64;
                        state.track_start_time = Some(Instant::now());
                    }
                }
            }
        }
        let _ = tx.send((snapshot_playback_state(state), snapshot_queue_state(state)));
        return;
    }

    // Handle SetOutputDevice separately because it needs mutable access to mpv/mpv_process
    // while the main match borrows mpv immutably via ipc.
    if let AudioCommand::SetOutputDevice(device_name) = command {
        if !ensure_mpv_running(mpv, mpv_process, socket_path, state, event_callback) {
            return;
        }
        handle_set_output_device(
            state,
            device_name,
            mpv,
            mpv_process,
            socket_path,
            event_callback,
        );
        return;
    }

    // Ensure mpv is running
    if !ensure_mpv_running(mpv, mpv_process, socket_path, state, event_callback) {
        return;
    }
    let ipc = mpv
        .as_ref()
        .expect("mpv should be Some after ensure_mpv_running returned true");

    tracing::debug!("Handling command: {:?}", std::mem::discriminant(&command));

    match command {
        AudioCommand::Play {
            track,
            stream_url,
            clear_queue,
        } => {
            tracing::info!("AudioCommand::Play received for: {}", track.name);
            // A new play supersedes any in-progress fade — restore volume and
            // discard a deferred pause/stop before loading.
            cancel_fade(state, ipc);
            if clear_queue {
                state.queue.clear();
                state.original_queue_order.clear();
            }

            // Add track to queue
            let queued = QueuedTrack {
                track: track.clone(),
                stream_url: stream_url.clone(),
                source: QueueSource::Context,
            };

            if clear_queue || state.queue.is_empty() {
                state.queue.push(queued);
                state.original_queue_order.push(0);
                state.queue_position = Some(0);
            } else {
                // Play-now-keep-queue: insert after the current track and
                // move to it, so the playing track is always IN the queue and
                // gapless pre-buffering tags entries with real positions.
                let insert_idx = state.queue_position.map_or(0, |pos| pos + 1);
                state.queue.insert(insert_idx, queued);
                state.original_queue_order.insert(insert_idx, insert_idx);
                state.queue_position = Some(insert_idx);
            }

            play_track(state, &track, &stream_url, event_callback, ipc);
        }

        AudioCommand::PlayContext {
            tracks,
            start_index,
            shuffle,
        } => {
            cancel_fade(state, ipc);

            // Keep any not-yet-played UserQueued entry (Play Next / Add to
            // Queue) ahead of the current position — Spotify's "Next in
            // queue" plays before a newly started context continues, rather
            // than vanishing under it. Already-played entries (at or before
            // the old position) are history and are dropped along with the
            // rest of the old queue.
            let retained_user_queued: Vec<QueuedTrack> = state
                .queue_position
                .and_then(|pos| state.queue.get(pos + 1..))
                .map(|rest| {
                    rest.iter()
                        .filter(|q| q.source == QueueSource::UserQueued)
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();

            let context: Vec<QueuedTrack> = tracks
                .into_iter()
                .map(|(track, stream_url)| QueuedTrack {
                    track,
                    stream_url,
                    source: QueueSource::Context,
                })
                .collect();

            if context.is_empty() {
                state.queue.clear();
                state.original_queue_order.clear();
                state.queue_position = None;
                emit_state_changed(state, event_callback);
                emit_queue_changed(state, event_callback);
                return;
            }

            let start_index = start_index.min(context.len() - 1);
            let (before, at_and_after) = context.split_at(start_index);
            let (start_track, after) = at_and_after
                .split_first()
                .expect("at_and_after is non-empty: start_index < context.len()");

            let mut new_queue: Vec<QueuedTrack> =
                Vec::with_capacity(before.len() + 1 + retained_user_queued.len() + after.len());
            new_queue.extend_from_slice(before);
            new_queue.push(start_track.clone());
            new_queue.extend(retained_user_queued);
            new_queue.extend_from_slice(after);

            let queue_position = before.len();
            state.original_queue_order = (0..new_queue.len()).collect();
            state.queue = new_queue;
            state.queue_position = Some(queue_position);
            state.shuffle = shuffle;

            // Apply shuffle (start track pinned first, same mechanism as the
            // SetShuffle command) BEFORE the first broadcast below, so a
            // caller with shuffle already on sees one shuffled queue instead
            // of an unshuffled one that visibly reorders a moment later.
            if shuffle {
                shuffle_queue_tail(
                    &mut state.queue,
                    &mut state.original_queue_order,
                    queue_position + 1,
                );
            }

            let queued = state.queue[queue_position].clone();
            play_track(
                state,
                &queued.track,
                &queued.stream_url,
                event_callback,
                ipc,
            );
        }

        AudioCommand::Pause => {
            do_pause(state, ipc, event_callback);
        }

        AudioCommand::Resume => {
            do_resume(state, ipc, event_callback);
        }

        AudioCommand::TogglePlayPause => {
            if state.is_playing {
                do_pause(state, ipc, event_callback);
            } else {
                do_resume(state, ipc, event_callback);
            }
        }

        AudioCommand::Stop => {
            if state.fade_enabled && state.is_playing {
                // Fade out, then stop for real at the ramp end (advance_fade
                // does the mpv stop + state clear). Flip is_playing now for the
                // UI.
                state.is_playing = false;
                emit_state_changed(state, event_callback);
                state.active_fade = Some(ActiveFade {
                    kind: FadeKind::StopOut,
                    start: Instant::now(),
                    duration_ms: state.fade_duration_ms,
                });
            } else {
                // Instant stop: discard any in-progress fade too.
                state.active_fade = None;
                let _ = ipc.command(vec![Value::String("stop".to_string())]);
                state.is_playing = false;
                state.current_track = None;
                state.track_start_time = None;
                state.paused_position_ms = 0;
                state.mpv_playlist_tracks.clear();
                state.recovery_due = None;
                emit_state_changed(state, event_callback);
            }
        }

        AudioCommand::Seek(position_ms) => {
            // Seeks stay instant — a fade in flight would otherwise leave the
            // volume stuck low mid-scrub.
            cancel_fade(state, ipc);
            handle_seek(state, position_ms, event_callback, ipc);
        }

        AudioCommand::SetVolume(volume) => {
            let volume = volume.clamp(0.0, 1.0);
            state.volume = volume;
            // mpv volume is 0-100
            let mpv_volume = (state.adjusted_volume() * 100.0) as i64;
            let _ = ipc.set_property("volume", Value::Number(mpv_volume.into()));
            emit_state_changed(state, event_callback);
        }

        AudioCommand::AddToQueue { tracks, position } => {
            let start_idx = match position {
                QueueInsertPosition::Next => state.queue_position.map_or(0, |pos| pos + 1),
                QueueInsertPosition::End => state.queue.len(),
            };

            for (i, (track, url)) in tracks.into_iter().enumerate() {
                let queued = QueuedTrack {
                    track,
                    stream_url: url,
                    source: QueueSource::UserQueued,
                };
                let insert_idx = start_idx + i;
                state.queue.insert(insert_idx, queued);
                state.original_queue_order.insert(insert_idx, insert_idx);
            }

            // Clear and re-queue tracks in mpv for gapless playback
            // This ensures "Play Next" works correctly
            invalidate_and_requeue_mpv(state, ipc);
            emit_queue_changed(state, event_callback);
        }

        AudioCommand::ClearQueue => {
            handle_clear_queue(state, ipc, event_callback);
        }

        AudioCommand::RemoveFromQueue(index) => {
            if index < state.queue.len() && state.queue_position != Some(index) {
                state.queue.remove(index);
                state.original_queue_order.remove(index);

                if let Some(pos) = state.queue_position {
                    if index < pos {
                        state.queue_position = Some(pos - 1);
                    }
                }

                // Re-sync mpv playlist to match our queue
                invalidate_and_requeue_mpv(state, ipc);
                emit_queue_changed(state, event_callback);
            }
        }

        AudioCommand::ReorderQueue { from, to } => {
            handle_reorder_queue(state, from, to, ipc, event_callback);
        }

        AudioCommand::JumpToQueueIndex(index) => {
            // Instant, like a skip: drop any in-progress fade first.
            cancel_fade(state, ipc);
            handle_jump_to_queue_index(state, index, event_callback, ipc);
        }

        AudioCommand::SkipNext => {
            // Skips are instant; drop any in-progress fade first.
            cancel_fade(state, ipc);
            handle_skip_next(state, event_callback, ipc);
        }

        AudioCommand::SkipPrevious => {
            cancel_fade(state, ipc);
            handle_skip_previous(state, event_callback, ipc);
        }

        AudioCommand::SetShuffle(shuffle) => {
            // Rebuild mpv's gapless prebuffer from the reordered queue.
            // handle_set_shuffle reorders state.queue, but the 1-2 tracks already
            // loaded into mpv's internal playlist keep their pre-shuffle order
            // until it drains — so the immediately-next gapless track would play
            // stale without this, exactly as AddToQueue/RemoveFromQueue do.
            if handle_set_shuffle(state, shuffle, event_callback) {
                invalidate_and_requeue_mpv(state, ipc);
            }
        }

        AudioCommand::SetRepeat(repeat) => {
            state.repeat = repeat;

            // Repeat is handled entirely by the engine's queue layer
            // (resolve_next_queue_position + gapless pre-buffering). mpv's own
            // loop modes must stay OFF: loop-file swallows end-of-track events
            // (no scrobbles on repeat-one) and loop-playlist wraps mpv's
            // internal playlist behind the tracker's back, desyncing the
            // queue position.
            let _ = ipc.set_property("loop-playlist", Value::String("no".to_string()));
            let _ = ipc.set_property("loop-file", Value::String("no".to_string()));

            emit_state_changed(state, event_callback);
        }

        AudioCommand::SetOutputDevice(_) => {
            unreachable!("SetOutputDevice is handled before the main match");
        }

        AudioCommand::QuerySnapshot(_) => {
            unreachable!("QuerySnapshot is handled before the main match");
        }

        AudioCommand::SetReplayGain(settings) => {
            state.replay_gain = settings;
            // Reapply volume with new ReplayGain settings
            let mpv_volume = (state.adjusted_volume() * 100.0) as i64;
            let _ = ipc.set_property("volume", Value::Number(mpv_volume.into()));
            tracing::debug!(
                "ReplayGain settings updated: enabled={}, mode={:?}, pre_amp={}dB",
                state.replay_gain.enabled,
                state.replay_gain.mode,
                state.replay_gain.pre_amp_db
            );
        }

        AudioCommand::SetEqEnabled(enabled) => {
            state.eq_enabled = enabled;
            apply_eq(state, ipc);
            tracing::debug!("Equalizer enabled set to {}", enabled);
        }

        AudioCommand::SetEqPreset(preset_name) => {
            match super::eq::EqPreset::from_str(&preset_name) {
                Some(preset) => {
                    state.eq_bands = preset.bands_18().to_vec();
                    apply_eq(state, ipc);
                    tracing::debug!("Equalizer preset set to '{}'", preset_name);
                }
                None => {
                    tracing::warn!("Ignoring unknown EQ preset '{}'", preset_name);
                }
            }
        }

        AudioCommand::SetEqBands(bands) => {
            // Commands normally arrive as 18 bands, but accept 10 and map up so
            // the engine is robust to either shape.
            state.eq_bands = match <[i32; 10]>::try_from(bands.as_slice()) {
                Ok(bands_10) => super::eq::map_10_to_18_bands(&bands_10).to_vec(),
                Err(_) => bands,
            };
            apply_eq(state, ipc);
            tracing::debug!("Equalizer custom bands applied");
        }

        AudioCommand::SetFade {
            enabled,
            duration_ms,
        } => {
            state.fade_enabled = enabled;
            state.fade_duration_ms = duration_ms.clamp(0, 2000);
            tracing::debug!(
                "Fade set: enabled={}, duration_ms={}",
                state.fade_enabled,
                state.fade_duration_ms
            );
        }

        AudioCommand::Shutdown => {
            tracing::info!("Audio engine shutting down");
            is_running.store(false, Ordering::SeqCst);
            let _ = ipc.command(vec![Value::String("quit".to_string())]);
        }
    }
}

/// Apply the current EQ state to mpv's `af` (audio-filter) property.
///
/// When enabled, renders the stored bands to a `superequalizer` filter string;
/// when disabled, clears `af`. Called on every EQ change AND after any fresh
/// mpv connection, because the `af` property does not survive an mpv respawn
/// (device switch, crash recovery, initial start).
pub(crate) fn apply_eq(state: &EngineState, ipc: &MpvIpc) {
    if state.eq_enabled {
        let filter = super::eq::filter_string(&state.eq_bands);
        match ipc.set_property("af", Value::String(filter.clone())) {
            Ok(()) => tracing::debug!("Applied EQ filter: {}", filter),
            Err(e) => tracing::warn!("Failed to apply EQ filter '{}': {}", filter, e),
        }
    } else if let Err(e) = ipc.set_property("af", Value::String(String::new())) {
        tracing::warn!("Failed to clear EQ filter: {}", e);
    }
}

/// Ensure mpv is running. If disconnected or not started, restarts it.
///
/// Returns `true` if mpv is ready, `false` if it could not be started.
/// After this returns `true`, `mpv.as_ref().unwrap()` is safe.
fn ensure_mpv_running<F>(
    mpv: &mut Option<MpvIpc>,
    mpv_process: &mut Option<Child>,
    socket_path: &std::path::PathBuf,
    state: &mut EngineState,
    event_callback: &F,
) -> bool
where
    F: Fn(AudioEvent),
{
    match mpv {
        Some(ref ipc) if ipc.is_connected() => return true,
        Some(_) => {
            tracing::info!("mpv connection lost, restarting...");
            *mpv = None;
        }
        None => {
            tracing::info!("Starting mpv for the first time in command handler...");
        }
    }
    restart_mpv(mpv, mpv_process, socket_path, state, event_callback)
}

/// Restart mpv process and reconnect.
///
/// If a track was mid-play when mpv died, playback resumes from the last
/// known position (a hiccup, not a stop — and never a skip: the fresh
/// connection's initial `idle-active=true` observer value used to race the
/// old state and advance the queue).
///
/// Returns `true` if mpv was successfully started and connected.
pub(crate) fn restart_mpv<F>(
    mpv: &mut Option<MpvIpc>,
    mpv_process: &mut Option<Child>,
    socket_path: &std::path::PathBuf,
    state: &mut EngineState,
    event_callback: &F,
) -> bool
where
    F: Fn(AudioEvent),
{
    if let Some(mut child) = mpv_process.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = std::fs::remove_file(socket_path);

    match spawn_mpv(socket_path, state.output_device_name.as_deref()) {
        Ok(child) => {
            *mpv_process = Some(child);
            match connect_to_mpv(socket_path) {
                Ok(new_ipc) => {
                    let _ = setup_property_observers(&new_ipc);
                    tracing::info!("mpv connected successfully");
                    *mpv = Some(new_ipc);

                    // A fresh mpv has no `af` filter; re-apply the EQ before
                    // (possibly) resuming so the sound is unchanged by the hiccup.
                    if let Some(ipc) = mpv.as_ref() {
                        apply_eq(state, ipc);
                    }

                    // The old mpv took any loaded track with it; our state
                    // still says one is current. Resume it where it was.
                    if state.current_track.is_some() {
                        let resume_pos = state.current_position_ms();
                        if let (Some(queued), Some(ipc)) = (
                            state
                                .queue_position
                                .and_then(|pos| state.queue.get(pos))
                                .cloned(),
                            mpv.as_ref(),
                        ) {
                            tracing::info!(
                                "Resuming '{}' at {}ms after mpv restart",
                                queued.track.name,
                                resume_pos
                            );
                            play_track_at_position(
                                state,
                                &queued.track,
                                &queued.stream_url,
                                resume_pos,
                                false,
                                event_callback,
                                ipc,
                            );
                        }
                    }
                    true
                }
                Err(e) => {
                    tracing::error!("Failed to connect to mpv: {}", e);
                    event_callback(AudioEvent::Error {
                        message: format!("Failed to connect to mpv: {}", e),
                    });
                    false
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to start mpv: {}", e);
            event_callback(AudioEvent::Error {
                message: format!("Failed to start mpv: {}", e),
            });
            false
        }
    }
}

/// Begin a pause: fade the volume out and pause at the ramp end when
/// fade is enabled, otherwise pause mpv instantly.
///
/// The fade path flips `is_playing`/`track_start_time` up front so the UI
/// reflects the pause immediately and mpv keeps decoding during the ramp.
/// CRITICAL: `paused_position_ms` is read from `current_position_ms()` BEFORE
/// those flips, because that reader depends on both — reversing the order would
/// freeze the resumed position. The early flip also means the async mpv `pause`
/// event's `paused && was_playing` branch is a no-op, so the pause
/// bookkeeping is performed here.
fn do_pause<F>(state: &mut EngineState, ipc: &MpvIpc, event_callback: &F)
where
    F: Fn(AudioEvent),
{
    if !state.is_playing {
        return;
    }
    if state.fade_enabled {
        state.paused_position_ms = state.current_position_ms();
        state.track_start_time = None;
        state.is_playing = false;
        emit_state_changed(state, event_callback);
        state.active_fade = Some(ActiveFade {
            kind: FadeKind::PauseOut,
            start: Instant::now(),
            duration_ms: state.fade_duration_ms,
        });
    } else {
        let _ = ipc.set_property("pause", Value::Bool(true));
    }
}

/// Begin a resume: cancel any in-progress fade, unpause, restore the
/// listened-time resume bookkeeping, then fade the volume in from silence when fade is
/// enabled.
///
/// The bookkeeping runs unconditionally (both fade and non-fade paths): with
/// `is_playing` already true the subsequent async mpv `pause=false` event's
/// `!paused && !was_playing` branch is a no-op, so this keeps listened time
/// consistent while also covering the resume-during-a-deferred-pause-fade case where
/// mpv was never actually paused.
fn do_resume<F>(state: &mut EngineState, ipc: &MpvIpc, event_callback: &F)
where
    F: Fn(AudioEvent),
{
    if state.is_playing || state.current_track.is_none() {
        return;
    }
    cancel_fade(state, ipc);
    let _ = ipc.set_property("pause", Value::Bool(false));
    state.track_start_time = Some(Instant::now());
    state.track_start_position_ms = state.paused_position_ms;
    state.is_playing = true;
    emit_state_changed(state, event_callback);
    if state.fade_enabled {
        let _ = ipc.set_property("volume", Value::Number(0.into()));
        state.active_fade = Some(ActiveFade {
            kind: FadeKind::ResumeIn,
            start: Instant::now(),
            duration_ms: state.fade_duration_ms,
        });
    }
}

/// Handle seek command.
pub(crate) fn handle_seek<F>(
    state: &mut EngineState,
    position_ms: u64,
    event_callback: &F,
    ipc: &MpvIpc,
) where
    F: Fn(AudioEvent),
{
    if state.current_track.is_some() {
        let duration = state.duration_ms();
        let target_ms = if duration > 0 {
            position_ms.min(duration)
        } else {
            position_ms
        };

        // Seek-aware listened time: close the span that played up to the
        // pre-seek position and open a fresh one at the target, so skipped-over
        // or re-heard audio is never credited. Done before both the deferred and
        // live branches; close_play_span reads the live pre-seek position.
        state.close_play_span();
        state.span_start_ms = target_ms;

        // mpv rejects seeks while the file is still loading; park the target
        // and apply it on playback-restart instead of surfacing an error for
        // a perfectly reasonable "click track, drag scrubber" gesture.
        if state.is_loading {
            tracing::debug!("Deferring seek to {}ms until playback starts", target_ms);
            state.pending_seek_ms = Some(target_ms);
            state.track_start_position_ms = target_ms;
            state.paused_position_ms = target_ms;
            emit_state_changed(state, event_callback);
            return;
        }

        let target_secs = target_ms as f64 / 1000.0;

        let target_num = serde_json::Number::from_f64(target_secs)
            .unwrap_or_else(|| serde_json::Number::from(0));
        if let Err(e) = ipc.command(vec![
            Value::String("seek".to_string()),
            Value::Number(target_num),
            Value::String("absolute".to_string()),
        ]) {
            tracing::error!("Seek failed: {}", e);
            event_callback(AudioEvent::Error {
                message: format!("Seek failed: {}", e),
            });
            return;
        }

        state.track_start_position_ms = target_ms;
        state.track_start_time = if state.is_playing {
            Some(Instant::now())
        } else {
            None
        };
        state.paused_position_ms = target_ms;
        emit_state_changed(state, event_callback);
    }
}

/// Handle clear queue command.
fn handle_clear_queue<F>(state: &mut EngineState, ipc: &MpvIpc, event_callback: &F)
where
    F: Fn(AudioEvent),
{
    // Keep current track, clear the rest
    if let Some(pos) = state.queue_position {
        let current = state.queue.remove(pos);
        state.queue.clear();
        state.queue.push(current);
        state.queue_position = Some(0);
        state.original_queue_order.clear();
        state.original_queue_order.push(0);
    } else {
        state.queue.clear();
        state.original_queue_order.clear();
    }

    // Clear queued tracks from mpv (remove all except index 0)
    // playlist-remove removes by index; we remove from back to front
    let playlist_len = state.mpv_playlist_tracks.len();
    for i in (1..playlist_len).rev() {
        let _ = ipc.command(vec![
            Value::String("playlist-remove".to_string()),
            Value::Number(i.into()),
        ]);
    }
    // Keep only current track in our tracking
    while state.mpv_playlist_tracks.len() > 1 {
        state.mpv_playlist_tracks.pop_back();
    }

    emit_queue_changed(state, event_callback);
}

/// Handle reorder queue command.
fn handle_reorder_queue<F>(
    state: &mut EngineState,
    from: usize,
    to: usize,
    ipc: &MpvIpc,
    event_callback: &F,
) where
    F: Fn(AudioEvent),
{
    if from < state.queue.len() && to < state.queue.len() && from != to {
        let item = state.queue.remove(from);
        let order = state.original_queue_order.remove(from);
        state.queue.insert(to, item);
        state.original_queue_order.insert(to, order);

        if let Some(pos) = state.queue_position {
            if pos == from {
                state.queue_position = Some(to);
            } else if from < to && pos > from && pos <= to {
                state.queue_position = Some(pos - 1);
            } else if from > to && pos >= to && pos < from {
                state.queue_position = Some(pos + 1);
            }
        }

        // Re-sync mpv playlist to match our queue
        invalidate_and_requeue_mpv(state, ipc);
        emit_queue_changed(state, event_callback);
    }
}

/// Handle jump-to-index: the Up Next panel's double-click/Return activation.
///
/// The one queue-navigation path alongside `SkipNext`/`SkipPrevious` — all
/// three set `queue_position` and hand off to the shared `play_track`, never
/// a second play mechanism. A no-op out of bounds or on the already-current
/// entry (the panel never lists the current track as a jump target, so this
/// is a defensive guard rather than a real UI path).
fn handle_jump_to_queue_index<F>(
    state: &mut EngineState,
    index: usize,
    event_callback: &F,
    ipc: &MpvIpc,
) where
    F: Fn(AudioEvent),
{
    if state.queue_position == Some(index) {
        return;
    }
    if let Some(queued) = state.queue.get(index).cloned() {
        state.queue_position = Some(index);
        play_track(
            state,
            &queued.track,
            &queued.stream_url,
            event_callback,
            ipc,
        );
    }
}

/// Handle skip next command.
///
/// Uses the MANUAL resolver: an explicit next always advances, even under
/// repeat-one (repeat-one applies to natural track ends only).
fn handle_skip_next<F>(state: &mut EngineState, event_callback: &F, ipc: &MpvIpc)
where
    F: Fn(AudioEvent),
{
    if let Some(next_pos) = resolve_manual_next_position(state) {
        if let Some(queued) = state.queue.get(next_pos).cloned() {
            state.queue_position = Some(next_pos);
            play_track(
                state,
                &queued.track,
                &queued.stream_url,
                event_callback,
                ipc,
            );
        }
    }
}

/// Handle skip previous command.
fn handle_skip_previous<F>(state: &mut EngineState, event_callback: &F, ipc: &MpvIpc)
where
    F: Fn(AudioEvent),
{
    if let Some(pos) = state.queue_position {
        if state.current_position_ms() > 3000 {
            // Restart current track
            let _ = ipc.command(vec![
                Value::String("seek".to_string()),
                Value::Number(0.into()),
                Value::String("absolute".to_string()),
            ]);
            state.track_start_time = Some(Instant::now());
            state.track_start_position_ms = 0;
            state.paused_position_ms = 0;
            emit_state_changed(state, event_callback);
        } else if pos > 0 {
            state.queue_position = Some(pos - 1);
            if let Some(queued) = state.queue.get(pos - 1).cloned() {
                play_track(
                    state,
                    &queued.track,
                    &queued.stream_url,
                    event_callback,
                    ipc,
                );
            }
        } else if state.repeat == RepeatMode::All && !state.queue.is_empty() {
            let last = state.queue.len() - 1;
            state.queue_position = Some(last);
            if let Some(queued) = state.queue.get(last).cloned() {
                play_track(
                    state,
                    &queued.track,
                    &queued.stream_url,
                    event_callback,
                    ipc,
                );
            }
        }
    }
}
