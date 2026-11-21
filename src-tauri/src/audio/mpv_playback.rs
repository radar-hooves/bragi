//! Playback operations for the mpv audio engine.
//!
//! Track loading, seeking, gapless queue management, mpv playlist synchronisation,
//! shuffle/unshuffle, and output device switching.

use std::process::Child;
use std::time::Instant;

use serde_json::Value;

use super::mpv_engine::{
    emit_queue_changed, emit_state_changed, EngineState, PlaylistEntry, QueuedTrack,
};
use super::mpv_ipc::MpvIpc;
use super::mpv_process::{connect_to_mpv, setup_property_observers, spawn_mpv};
use super::types::{AudioEvent, RepeatMode};

/// Play a track via mpv.
pub(crate) fn play_track<F>(
    state: &mut EngineState,
    track: &crate::models::Track,
    stream_url: &str,
    event_callback: &F,
    ipc: &MpvIpc,
) where
    F: Fn(AudioEvent),
{
    play_track_at_position(state, track, stream_url, 0, true, event_callback, ipc);
}

/// Play a track via mpv at a specific position.
///
/// `credit_interrupted` controls whether the currently playing track gets a
/// TrackFinished emission before being replaced. A user-initiated change of
/// track wants it (that is how partial listens and skips get recorded); a
/// RESUME of the same track (device switch, mpv restart) must not, or one
/// real listen turns into two scrobbles.
pub(crate) fn play_track_at_position<F>(
    state: &mut EngineState,
    track: &crate::models::Track,
    stream_url: &str,
    start_position_ms: u64,
    credit_interrupted: bool,
    event_callback: &F,
    ipc: &MpvIpc,
) where
    F: Fn(AudioEvent),
{
    // Redact the query string: a proxy stream URL carries the per-session
    // `?tk=<token>` secret. Never log the tokenised URL.
    tracing::info!(
        "play_track_at_position: {} at {}ms, url={}",
        track.name,
        start_position_ms,
        crate::proxy::redact_stream_url(stream_url)
    );

    // Emit TrackFinished for the currently playing track before replacing it.
    // This ensures scrobbles fire when the user double-clicks a new track
    // mid-playback (the interrupted track won't get an end-file/eof event).
    // Guards: >1 second of playback (webview reloads must not scrobble) and
    // not still loading (a loading track never actually played).
    if credit_interrupted
        && state.is_playing
        && !state.is_loading
        && state.current_position_ms() > 1000
    {
        if let Some(current) = state.current_track.clone() {
            let played_ms = state.current_position_ms();
            let finished_at = chrono::Utc::now().timestamp();
            state.emit_track_finished(&current, played_ms, finished_at, event_callback);
        }
    }

    // Clear mpv playlist
    if let Err(e) = ipc.command(vec![Value::String("playlist-clear".to_string())]) {
        tracing::warn!("Failed to clear playlist: {}", e);
    }
    state.mpv_playlist_tracks.clear();
    state.mpv_playlist_base_pos = 0;

    // Set start position before loading (applies to next loaded file).
    // Using the `start` property avoids loadfile option format differences across mpv versions.
    if start_position_ms > 0 {
        let start_secs = start_position_ms as f64 / 1000.0;
        if let Err(e) = ipc.set_property("start", Value::String(format!("{start_secs:.3}"))) {
            tracing::warn!("Failed to set start position: {}", e);
        }
    }

    // Set volume before playing
    let mpv_volume = (state.adjusted_volume() * 100.0) as i64;
    if let Err(e) = ipc.set_property("volume", Value::Number(mpv_volume.into())) {
        tracing::warn!("Failed to set volume: {}", e);
    }

    let args = vec![
        Value::String("loadfile".to_string()),
        Value::String(stream_url.to_string()),
        Value::String("replace".to_string()),
    ];

    tracing::debug!("Sending loadfile command: {:?}", args);

    // Mark as loading to suppress progress updates until playback-restart
    state.is_loading = true;

    match ipc.command(args) {
        Ok(result) => {
            tracing::info!("loadfile succeeded: {:?}", result);

            // Update state
            state.current_track = Some(track.clone());
            state.is_playing = true;
            state.track_start_time = Some(Instant::now());
            state.track_start_position_ms = start_position_ms;
            state.paused_position_ms = start_position_ms;
            // Begin listened-time accounting from this play's start.
            // Normal plays pass start_position_ms == 0; StartFile also re-zeroes.
            state.reset_listened(start_position_ms);

            // A fresh playlist invalidates any credits pending against the
            // old one, and a deliberate play supersedes any scheduled
            // failure recovery or deferred seek.
            state.recently_popped.clear();
            state.recovery_due = None;
            state.pending_seek_ms = None;

            // Track in our playlist queue
            if let Some(pos) = state.queue_position {
                state.mpv_playlist_tracks.push_back(PlaylistEntry {
                    track: track.clone(),
                    queue_position: pos,
                    entry_id: result.get("playlist_entry_id").and_then(|v| v.as_i64()),
                });
            }

            // Reset start property so it doesn't affect subsequent tracks
            if start_position_ms > 0 {
                let _ = ipc.set_property("start", Value::String("none".to_string()));
            }

            // Explicitly unpause mpv (in case it started paused)
            if let Err(e) = ipc.set_property("pause", Value::Bool(false)) {
                tracing::warn!("Failed to unpause mpv: {}", e);
            }

            emit_state_changed(state, event_callback);
            emit_queue_changed(state, event_callback);

            // Queue next tracks for gapless playback
            queue_next_tracks_in_mpv(state, ipc);

            // Redact the query string so the per-session proxy token never
            // reaches this persisted INFO line.
            tracing::info!(
                "Playing: {} ({})",
                track.name,
                crate::proxy::redact_stream_url(stream_url)
            );
        }
        Err(e) => {
            state.is_loading = false;
            state.is_playing = false;
            state.current_track = None;
            state.track_start_time = None;
            tracing::error!("Failed to play track via loadfile: {}", e);
            event_callback(AudioEvent::Error {
                message: format!("Failed to play: {}", e),
            });
            emit_state_changed(state, event_callback);
        }
    }
}

/// Invalidate queued tracks in mpv and re-queue from current state.
///
/// Called when the queue is modified (add, remove, reorder) to ensure
/// mpv's playlist matches our queue state.
pub(crate) fn invalidate_and_requeue_mpv(state: &mut EngineState, ipc: &MpvIpc) {
    // Remove all tracks from mpv playlist except the currently playing one (index 0)
    let playlist_len = state.mpv_playlist_tracks.len();
    for i in (1..playlist_len).rev() {
        let _ = ipc.command(vec![
            Value::String("playlist-remove".to_string()),
            Value::Number(i.into()),
        ]);
    }

    // Clear our tracking queue except current
    while state.mpv_playlist_tracks.len() > 1 {
        state.mpv_playlist_tracks.pop_back();
    }

    // Re-queue upcoming tracks
    queue_next_tracks_in_mpv(state, ipc);
}

/// Queue upcoming tracks in mpv for gapless playback.
pub(crate) fn queue_next_tracks_in_mpv(state: &mut EngineState, ipc: &MpvIpc) {
    let Some(current_pos) = state.queue_position else {
        return;
    };

    // Queue up to 2 tracks ahead for gapless playback
    let max_lookahead = 2;
    let mut queued = state.mpv_playlist_tracks.len().saturating_sub(1); // Don't count current

    for i in 1..=max_lookahead {
        if queued >= max_lookahead {
            break;
        }

        let next_pos = if state.repeat == RepeatMode::One {
            current_pos // Repeat same track
        } else {
            let pos = current_pos + i;
            if pos >= state.queue.len() {
                if state.repeat == RepeatMode::All {
                    pos % state.queue.len()
                } else {
                    continue;
                }
            } else {
                pos
            }
        };

        // Check if already queued
        if state
            .mpv_playlist_tracks
            .iter()
            .any(|e| e.queue_position == next_pos)
        {
            continue;
        }

        if let Some(queued_track) = state.queue.get(next_pos) {
            // Append to mpv playlist
            match ipc.command(vec![
                Value::String("loadfile".to_string()),
                Value::String(queued_track.stream_url.clone()),
                Value::String("append".to_string()),
            ]) {
                Ok(result) => {
                    state.mpv_playlist_tracks.push_back(PlaylistEntry {
                        track: queued_track.track.clone(),
                        queue_position: next_pos,
                        entry_id: result.get("playlist_entry_id").and_then(|v| v.as_i64()),
                    });
                    queued += 1;
                    tracing::debug!(
                        "Queued for gapless: {} (pos {})",
                        queued_track.track.name,
                        next_pos
                    );
                }
                Err(e) => {
                    tracing::warn!("Failed to queue track for gapless: {}", e);
                }
            }
        }
    }
}

/// Fisher-Yates shuffle of `queue[shuffle_start..]` in place, keeping
/// `original_order` (the pre-shuffle index of each queue slot, used to
/// restore order when shuffle is later disabled) in sync via the same
/// parallel swaps. Shared by `handle_set_shuffle`'s enable path and
/// `PlayContext` (`mpv_commands.rs`), so a freshly built context shuffles
/// exactly the same way toggling shuffle on an existing queue does — one
/// mechanism, not a parallel reimplementation.
pub(crate) fn shuffle_queue_tail(
    queue: &mut [QueuedTrack],
    original_order: &mut [usize],
    shuffle_start: usize,
) {
    let len = queue.len();
    if shuffle_start >= len {
        return;
    }
    for i in (shuffle_start + 1..len).rev() {
        let j = shuffle_start + fastrand::usize(..=(i - shuffle_start));
        queue.swap(i, j);
        original_order.swap(i, j);
    }
}

/// Handle set shuffle command.
///
/// Returns `true` when the queue order (or the shuffle flag) actually changed,
/// so the caller can rebuild mpv's gapless prebuffer only when it needs to.
///
/// Enabling is deliberately NOT idempotent: it re-permutes the current queue
/// from its present order every time it is requested — toggling shuffle off
/// then on again on an unchanged queue is expected to produce a new order,
/// not a no-op. `PlayContext` applies this same permutation (via
/// `shuffle_queue_tail`) while it builds a fresh context queue instead of
/// routing through this command, so starting a new list with shuffle already
/// on shuffles exactly once, before the first broadcast.
/// Disabling when already off is the only true no-op.
pub(crate) fn handle_set_shuffle<F>(
    state: &mut EngineState,
    shuffle: bool,
    event_callback: &F,
) -> bool
where
    F: Fn(AudioEvent),
{
    if !shuffle && !state.shuffle {
        return false;
    }
    state.shuffle = shuffle;

    if shuffle {
        // Snapshot the current order as the restore baseline before shuffling.
        state.original_queue_order = (0..state.queue.len()).collect();

        // Shuffle everything after current position.
        let shuffle_start = state.queue_position.map_or(0, |pos| pos + 1);
        shuffle_queue_tail(
            &mut state.queue,
            &mut state.original_queue_order,
            shuffle_start,
        );
    } else {
        // Restore original order
        if !state.original_queue_order.is_empty() {
            let current_original_idx = state
                .queue_position
                .and_then(|pos| state.original_queue_order.get(pos).copied());

            let mut restore: Vec<(usize, QueuedTrack)> = state
                .original_queue_order
                .iter()
                .zip(state.queue.drain(..))
                .map(|(&orig_idx, track)| (orig_idx, track))
                .collect();
            restore.sort_by_key(|(idx, _)| *idx);

            state.queue = restore.into_iter().map(|(_, track)| track).collect();
            state.original_queue_order = (0..state.queue.len()).collect();

            if let Some(orig_idx) = current_original_idx {
                state.queue_position = Some(orig_idx);
            }
        }
    }

    emit_state_changed(state, event_callback);
    emit_queue_changed(state, event_callback);
    true
}

/// Handle set output device command.
///
/// Restarts mpv with the new audio device and resumes playback if it was active.
pub(crate) fn handle_set_output_device<F>(
    state: &mut EngineState,
    device_name: Option<String>,
    mpv: &mut Option<MpvIpc>,
    mpv_process: &mut Option<Child>,
    socket_path: &std::path::PathBuf,
    event_callback: &F,
) where
    F: Fn(AudioEvent),
{
    let was_playing = state.is_playing;
    let current_pos = state.current_position_ms();

    // Stop current playback
    if let Some(ref ipc) = mpv {
        let _ = ipc.command(vec![Value::String("stop".to_string())]);
    }

    // Restart mpv with new device
    if let Some(mut child) = mpv_process.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = std::fs::remove_file(socket_path);

    match spawn_mpv(socket_path, device_name.as_deref()) {
        Ok(child) => {
            *mpv_process = Some(child);
            match connect_to_mpv(socket_path) {
                Ok(new_ipc) => {
                    let _ = setup_property_observers(&new_ipc);
                    let _ = new_ipc.set_property(
                        "volume",
                        Value::Number(((state.adjusted_volume() * 100.0) as i64).into()),
                    );

                    state.output_device_name = device_name;
                    *mpv = Some(new_ipc);

                    // A device switch respawns mpv, which drops the `af`
                    // filter; re-apply the EQ on the new process before resuming.
                    if let Some(ipc) = mpv.as_ref() {
                        super::mpv_commands::apply_eq(state, ipc);
                    }

                    // Resume playback if we were playing
                    if was_playing {
                        if let (Some(queued), Some(ipc)) = (
                            state
                                .queue_position
                                .and_then(|pos| state.queue.get(pos))
                                .cloned(),
                            mpv.as_ref(),
                        ) {
                            // Resume, not a new play: no interrupted-track credit.
                            play_track_at_position(
                                state,
                                &queued.track,
                                &queued.stream_url,
                                current_pos,
                                false,
                                event_callback,
                                ipc,
                            );
                        }
                    }

                    emit_state_changed(state, event_callback);
                }
                Err(e) => {
                    event_callback(AudioEvent::Error {
                        message: format!("Failed to connect to mpv after device change: {}", e),
                    });
                }
            }
        }
        Err(e) => {
            event_callback(AudioEvent::Error {
                message: format!("Failed to restart mpv for device change: {}", e),
            });
        }
    }
}
