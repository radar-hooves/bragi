//! Regression tests for the mpv audio engine.
//!
//! Uses a fake mpv — a UnixListener that accepts one connection, reads JSON
//! commands line-by-line, and replies with canned responses — so the real
//! `MpvIpc` drives the same socket-level protocol the production engine uses.
//!
//! Events are injected by calling `handle_mpv_event` directly; the fake never
//! pushes unsolicited events. Deferred stream-failure recoveries are pumped by
//! calling `attempt_stream_recovery` directly (in production the engine loop
//! fires it when `recovery_due` elapses).
//!
//! These tests pin: stale idle-active guards, the
//! entry-id-keyed listen credits, deferred retry/advance stream recovery with
//! a three-strike stop, manual skip overriding repeat-one, and
//! play-without-clear queue insertion.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::mpv_commands::{handle_command, handle_seek};
use super::mpv_engine::{
    advance_fade, snapshot_queue_state, ActiveFade, EngineState, FadeKind, PlaylistEntry,
    QueueSource, QueuedTrack,
};
use super::mpv_events::{attempt_stream_recovery, handle_mpv_event, reconcile_idle_playback};
use super::mpv_ipc::{MpvEvent, MpvIpc};
use super::mpv_playback::handle_set_shuffle;
use super::types::{AudioCommand, AudioEvent, QueueEntrySource, RepeatMode};
use crate::models::Track;

// ============================================================================
// Fake mpv infrastructure
// ============================================================================

/// All commands sent to the fake by the engine under test.
type CommandLog = Arc<Mutex<Vec<Value>>>;

/// Monotonic counter for unique socket paths across parallel test threads.
static SOCKET_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Start a fake mpv on a unique temp socket.
///
/// Returns the socket path, the command log, and the IPC client. The
/// `idle_active` flag controls what `get_property idle-active` returns;
/// `get_property time-pos` returns `0.0`; every `loadfile` reply carries a
/// fresh `playlist_entry_id` starting at 100 (mirroring mpv >= 0.38); all
/// other commands return `null`.
fn start_fake_mpv(idle_active: Arc<AtomicBool>) -> (PathBuf, CommandLog, MpvIpc) {
    let id = SOCKET_COUNTER.fetch_add(1, Ordering::SeqCst);
    let socket_path = PathBuf::from(format!(
        "/tmp/bragi-test-{}-{}.sock",
        std::process::id(),
        id
    ));

    let listener = UnixListener::bind(&socket_path).expect("bind fake socket");
    let log: CommandLog = Arc::new(Mutex::new(Vec::new()));
    let log_clone = Arc::clone(&log);
    let idle_clone = Arc::clone(&idle_active);

    thread::spawn(move || {
        let (stream, _) = listener.accept().expect("fake mpv accept");
        let write_stream = stream.try_clone().expect("clone stream");
        let write_stream = Arc::new(Mutex::new(write_stream));
        let entry_counter = AtomicI64::new(100);

        let mut reader = BufReader::new(stream);
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Err(_) => break,
                Ok(_) => {}
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let cmd: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let request_id = cmd.get("request_id").cloned().unwrap_or(Value::Null);

            if let Ok(mut l) = log_clone.lock() {
                l.push(cmd.clone());
            }

            let args = cmd.get("command").and_then(|a| a.as_array()).cloned();
            let data: Value = match args.as_deref() {
                Some([Value::String(cmd_name), Value::String(prop_name), ..])
                    if cmd_name == "get_property" =>
                {
                    match prop_name.as_str() {
                        "idle-active" => Value::Bool(idle_clone.load(Ordering::SeqCst)),
                        "time-pos" => json!(0.0),
                        _ => Value::Null,
                    }
                }
                Some([Value::String(cmd_name), ..]) if cmd_name == "loadfile" => {
                    json!({ "playlist_entry_id": entry_counter.fetch_add(1, Ordering::SeqCst) })
                }
                _ => Value::Null,
            };

            let reply = serde_json::to_string(&json!({
                "request_id": request_id,
                "error": "success",
                "data": data,
            }))
            .expect("serialise reply");

            if let Ok(mut w) = write_stream.lock() {
                let _ = writeln!(w, "{}", reply);
                let _ = w.flush();
            }
        }
    });

    thread::sleep(Duration::from_millis(20));
    let ipc = MpvIpc::connect(&socket_path).expect("connect to fake mpv");

    (socket_path, log, ipc)
}

/// Collect loadfile URLs from the command log.
fn loadfile_urls(log: &CommandLog) -> Vec<String> {
    log.lock()
        .unwrap()
        .iter()
        .filter_map(|cmd| {
            let args = cmd.get("command")?.as_array()?;
            if args.first()?.as_str()? == "loadfile" {
                args.get(1)?.as_str().map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Collect every `set_property("volume", n)` value from the command log.
fn volume_writes(log: &CommandLog) -> Vec<i64> {
    log.lock()
        .unwrap()
        .iter()
        .filter_map(|cmd| {
            let args = cmd.get("command")?.as_array()?;
            if args.first()?.as_str()? == "set_property" && args.get(1)?.as_str()? == "volume" {
                args.get(2)?.as_i64()
            } else {
                None
            }
        })
        .collect()
}

/// Collect every `set_property("pause", bool)` value from the command log.
fn pause_writes(log: &CommandLog) -> Vec<bool> {
    log.lock()
        .unwrap()
        .iter()
        .filter_map(|cmd| {
            let args = cmd.get("command")?.as_array()?;
            if args.first()?.as_str()? == "set_property" && args.get(1)?.as_str()? == "pause" {
                args.get(2)?.as_bool()
            } else {
                None
            }
        })
        .collect()
}

/// Collect AudioEvent::TrackFinished jellyfin ids from an event log.
fn track_finished_ids(events: &Arc<Mutex<Vec<AudioEvent>>>) -> Vec<String> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| {
            if let AudioEvent::TrackFinished { track, .. } = e {
                Some(track.jellyfin_id.clone())
            } else {
                None
            }
        })
        .collect()
}

/// The entry id of the current front tracking entry.
fn front_entry_id(state: &EngineState) -> Option<i64> {
    state.mpv_playlist_tracks.front().and_then(|e| e.entry_id)
}

/// Inject an EndFile{error} for the CURRENT front entry, then pump the
/// deferred recovery the way the engine loop would once the delay elapses.
fn fail_current_and_recover(
    state: &mut EngineState,
    events: &Arc<Mutex<Vec<AudioEvent>>>,
    ipc: &MpvIpc,
) {
    let cb = make_callback(Arc::clone(events));
    let entry_id = front_entry_id(state);
    handle_mpv_event(
        state,
        MpvEvent::EndFile {
            reason: "error".to_string(),
            playlist_entry_id: entry_id,
            file_error: Some("Cannot open stream".to_string()),
        },
        &cb,
        ipc,
    );
    if state.current_track.is_none() {
        // Third strike: the ladder stopped with an error; nothing to pump.
        return;
    }
    // Every non-terminal strike must schedule deferred recovery; pumping it
    // here stands in for the engine loop's timer without the sleep.
    assert!(
        state.recovery_due.take().is_some(),
        "stream failure must schedule a deferred recovery attempt"
    );
    attempt_stream_recovery(state, &cb, ipc);
}

// ============================================================================
// Test helpers
// ============================================================================

/// Minimal Track with unique jellyfin_id and stream URL of the form
/// `file:///fake/<n>.flac`.
fn make_track(n: u32) -> (Track, String) {
    let track = Track {
        jellyfin_id: format!("track-{n}"),
        mbid: None,
        artist_mbid: None,
        release_mbid: None,
        name: format!("Track {n}"),
        artist_name: "Artist".to_string(),
        album_name: "Album".to_string(),
        album_jellyfin_id: "album-1".to_string(),
        track_number: n,
        disc_number: 1,
        // 3-minute tracks so duration_ms() > 1000 for most tests.
        duration_ms: 180_000,
        genre: None,
        year: None,
        artwork_url: None,
        cached_file_path: None,
        file_size_bytes: None,
        normalization_gain_db: None,
    };
    let url = format!("file:///fake/{n}.flac");
    (track, url)
}

/// Build an `EngineState` with `n` tracks queued, position 0 playing.
///
/// The mpv playlist mirror carries stable entry ids 1..=n so end-file events
/// can be injected with the id of a specific entry.
fn make_state_n(n: usize, start_pos_ms: u64) -> EngineState {
    let mut state = EngineState::default();

    for i in 0..n {
        let (track, url) = make_track(i as u32);
        state.queue.push(QueuedTrack {
            track,
            stream_url: url,
            source: QueueSource::Context,
        });
        state.original_queue_order.push(i);
    }

    state.queue_position = Some(0);
    state.current_track = state.queue.first().map(|q| q.track.clone());
    state.is_playing = true;
    state.is_loading = false;
    state.track_start_time = Some(Instant::now());
    state.track_start_position_ms = start_pos_ms;
    // Keep listened-time accounting consistent with the play position.
    state.span_start_ms = start_pos_ms;
    state.mpv_playlist_base_pos = 0;

    for (i, q) in state.queue.iter().enumerate() {
        state.mpv_playlist_tracks.push_back(PlaylistEntry {
            track: q.track.clone(),
            queue_position: i,
            entry_id: Some(i as i64 + 1),
        });
    }

    state
}

/// Convenience event callback builder.
fn make_callback(events: Arc<Mutex<Vec<AudioEvent>>>) -> impl Fn(AudioEvent) {
    move |e| {
        events.lock().unwrap().push(e);
    }
}

/// Collect (jellyfin_id, played_ms) from TrackFinished events (tests).
fn finished_played_ms(events: &Arc<Mutex<Vec<AudioEvent>>>) -> Vec<(String, u64)> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| {
            if let AudioEvent::TrackFinished {
                track, played_ms, ..
            } = e
            {
                Some((track.jellyfin_id.clone(), *played_ms))
            } else {
                None
            }
        })
        .collect()
}

/// Pin the media position deterministically at `pos_ms` while "playing".
///
/// Setting `track_start_time = None` makes `current_position_ms()` return
/// `paused_position_ms` exactly, so span math does not depend on `Instant` drift.
fn simulate_played_to(state: &mut EngineState, pos_ms: u64) {
    state.is_playing = true;
    state.track_start_time = None;
    state.track_start_position_ms = pos_ms;
    state.paused_position_ms = pos_ms;
}

// ============================================================================
// Stale idle-active
// ============================================================================

/// `idle-active=true` arriving while mpv is actually live (the fake's real
/// value is `false`) must be IGNORED: the engine live-confirms the property
/// before treating it as end-of-playback. This is the "random skip" fix — a
/// stale initial observer value after an mpv restart or device change used to
/// advance the queue spuriously.
#[test]
fn stale_idle_active_is_ignored_when_mpv_is_live() {
    let idle = Arc::new(AtomicBool::new(false));
    let (_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(3, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    handle_mpv_event(
        &mut state,
        MpvEvent::PropertyChange {
            name: "idle-active".to_string(),
            data: Value::Bool(true),
        },
        &cb,
        &ipc,
    );

    assert_eq!(
        state.queue_position,
        Some(0),
        "queue must not advance on a stale idle-active"
    );
    assert_eq!(
        state.current_track.as_ref().map(|t| t.jellyfin_id.as_str()),
        Some("track-0")
    );
    assert!(
        loadfile_urls(&log).is_empty(),
        "no loadfile on a stale idle-active"
    );
}

/// When `is_loading=true` a loadfile is in flight; idle-active in this window
/// must be suppressed without even consulting mpv.
#[test]
fn idle_active_ignored_while_loading() {
    let idle = Arc::new(AtomicBool::new(true));
    let (_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(3, 0);
    state.is_loading = true;
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    handle_mpv_event(
        &mut state,
        MpvEvent::PropertyChange {
            name: "idle-active".to_string(),
            data: Value::Bool(true),
        },
        &cb,
        &ipc,
    );

    assert_eq!(state.queue_position, Some(0));
    assert_eq!(
        state.current_track.as_ref().map(|t| t.jellyfin_id.as_str()),
        Some("track-0")
    );
    assert!(loadfile_urls(&log).is_empty(), "no loadfile while loading");
}

/// A GENUINE idle-active (live-confirmed by the fake) advances exactly one
/// position and loads queue[1].
#[test]
fn genuine_idle_active_advances_exactly_one() {
    let idle = Arc::new(AtomicBool::new(true));
    let (_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(3, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    handle_mpv_event(
        &mut state,
        MpvEvent::PropertyChange {
            name: "idle-active".to_string(),
            data: Value::Bool(true),
        },
        &cb,
        &ipc,
    );

    assert_eq!(state.queue_position, Some(1), "must advance to position 1");

    let urls = loadfile_urls(&log);
    assert!(!urls.is_empty(), "at least one loadfile must be sent");
    assert!(
        urls[0].contains("1.flac"),
        "first loadfile must target queue[1]; got {:?}",
        urls
    );
}

/// A 1-track queue reaching idle stops playback, clears the current track,
/// and credits the final track once.
#[test]
fn natural_end_at_queue_tail_stops_and_credits_final_track() {
    let idle = Arc::new(AtomicBool::new(true));
    let (_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(1, 5_000);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    handle_mpv_event(
        &mut state,
        MpvEvent::PropertyChange {
            name: "idle-active".to_string(),
            data: Value::Bool(true),
        },
        &cb,
        &ipc,
    );

    assert!(!state.is_playing, "must stop after tail");
    assert!(state.current_track.is_none(), "current_track cleared");

    let finished = track_finished_ids(&events);
    assert_eq!(finished, vec!["track-0"], "exactly one TrackFinished");
}

// ============================================================================
// Stream-failure recovery (deferred retry → advance → three-strike stop)
// ============================================================================

/// First failure of a track that never left loading: recovery is scheduled
/// (not immediate), the pump retries the SAME track, and no TrackFinished is
/// emitted for a track that never played.
#[test]
fn stream_error_retries_same_track_once() {
    let idle = Arc::new(AtomicBool::new(false));
    let (_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(2, 0);
    state.is_loading = true; // open failure — never left loading
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    let front = front_entry_id(&state);
    handle_mpv_event(
        &mut state,
        MpvEvent::EndFile {
            reason: "error".to_string(),
            playlist_entry_id: front,
            file_error: Some("Failed to open".to_string()),
        },
        &cb,
        &ipc,
    );

    // Deferred: nothing sent yet, but a recovery is scheduled.
    assert!(state.recovery_due.is_some(), "recovery must be scheduled");
    assert!(
        loadfile_urls(&log).is_empty(),
        "retry must not fire immediately"
    );

    state.recovery_due = None;
    attempt_stream_recovery(&mut state, &cb, &ipc);

    assert_eq!(state.queue_position, Some(0), "still on queue[0] for retry");
    let urls = loadfile_urls(&log);
    assert!(
        urls.iter().any(|u| u.contains("0.flac")),
        "retry loadfile must target queue[0]; got {:?}",
        urls
    );
    assert!(
        track_finished_ids(&events).is_empty(),
        "no TrackFinished for an open failure"
    );
}

/// A second failure of the same (retried) track advances to the next queue
/// entry.
#[test]
fn stream_error_advances_after_retry_exhausted() {
    let idle = Arc::new(AtomicBool::new(false));
    let (_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(2, 0);
    state.is_loading = true;
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));

    // Failure 1 → retry queue[0] (play_track leaves is_loading=true until a
    // playback-restart, which never comes for a failing source).
    fail_current_and_recover(&mut state, &events, &ipc);
    assert_eq!(state.queue_position, Some(0));

    // Failure 2 (the retry also failed) → advance to queue[1].
    fail_current_and_recover(&mut state, &events, &ipc);

    assert_eq!(
        state.queue_position,
        Some(1),
        "advances to queue[1] after retry exhausted"
    );
    let urls = loadfile_urls(&log);
    assert!(
        urls.iter().any(|u| u.contains("1.flac")),
        "a loadfile for queue[1] must have been sent; got {:?}",
        urls
    );
}

/// Three consecutive failures stop playback with a repeated-failures error
/// and zero TrackFinished emissions.
#[test]
fn three_consecutive_failures_stop_with_error() {
    let idle = Arc::new(AtomicBool::new(false));
    let (_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(5, 0);
    state.is_loading = true;
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));

    for _ in 0..3 {
        fail_current_and_recover(&mut state, &events, &ipc);
    }

    assert!(!state.is_playing, "must stop after three failures");
    assert!(state.current_track.is_none(), "current_track cleared");

    let error_msgs: Vec<String> = events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| {
            if let AudioEvent::Error { message } = e {
                Some(message.clone())
            } else {
                None
            }
        })
        .collect();
    assert!(
        error_msgs
            .iter()
            .any(|m| m.to_lowercase().contains("repeated failures")),
        "expected a repeated-failures error; got: {:?}",
        error_msgs
    );
    assert!(
        track_finished_ids(&events).is_empty(),
        "no TrackFinished for tracks that never played"
    );
}

/// A failure while the track was ACTUALLY playing credits the partial listen
/// before retrying.
#[test]
fn mid_play_failure_credits_partial_listen() {
    let idle = Arc::new(AtomicBool::new(false));
    let (_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(2, 90_000);
    state.is_loading = false; // the track was genuinely playing
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));

    fail_current_and_recover(&mut state, &events, &ipc);

    let finished: Vec<(String, u64)> = events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| {
            if let AudioEvent::TrackFinished {
                track, played_ms, ..
            } = e
            {
                Some((track.jellyfin_id.clone(), *played_ms))
            } else {
                None
            }
        })
        .collect();

    assert!(
        !finished.is_empty(),
        "TrackFinished must be emitted for a mid-play failure"
    );
    assert_eq!(finished[0].0, "track-0");
    assert!(
        finished[0].1 >= 90_000,
        "played_ms must be >= 90000; got {}",
        finished[0].1
    );

    let urls = loadfile_urls(&log);
    assert!(
        urls.iter().any(|u| u.contains("0.flac")),
        "retry loadfile for queue[0] must be sent; got {:?}",
        urls
    );
}

/// An audio-output failure marks mpv for replacement, through the third strike
/// (02/11/2026: three plays failed on an mpv that could no longer resolve the
/// AirPods, though a fresh one opened them); a stream failure does not.
#[test]
fn audio_output_failure_marks_mpv_for_respawn() {
    let idle = Arc::new(AtomicBool::new(false));
    let (_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));
    let fail = |state: &mut EngineState, msg: &str| {
        let entry_id = front_entry_id(state);
        handle_mpv_event(
            state,
            MpvEvent::EndFile {
                reason: "error".to_string(),
                playlist_entry_id: entry_id,
                file_error: Some(msg.to_string()),
            },
            &cb,
            &ipc,
        );
    };

    let mut state = make_state_n(2, 0);
    fail(&mut state, "Cannot open stream");
    assert!(!state.mpv_respawn_due, "a stream failure keeps this mpv");

    let mut state = make_state_n(2, 0);
    state.consecutive_stream_failures = 2;
    fail(&mut state, "audio output initialization failed");
    assert!(state.current_track.is_none(), "third strike stops playback");
    assert!(state.recovery_due.is_none());
    assert!(
        state.mpv_respawn_due,
        "the next play must not meet the broken mpv"
    );
}

/// An error for an entry that is NOT the current front (stale — superseded by
/// a rebuild or an mpv auto-advance) must be ignored entirely.
#[test]
fn stale_entry_error_is_ignored() {
    let idle = Arc::new(AtomicBool::new(false));
    let (_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(3, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    handle_mpv_event(
        &mut state,
        MpvEvent::EndFile {
            reason: "error".to_string(),
            playlist_entry_id: Some(999), // no such entry
            file_error: Some("Cannot open stream".to_string()),
        },
        &cb,
        &ipc,
    );

    assert!(
        state.recovery_due.is_none(),
        "no recovery for a stale error"
    );
    assert!(state.is_playing, "playback undisturbed");
    assert_eq!(state.queue_position, Some(0));
    assert!(loadfile_urls(&log).is_empty());
    assert!(events.lock().unwrap().is_empty(), "no events emitted");
}

// ============================================================================
// Listen credits: eof + entry id, pops are bookkeeping only
// ============================================================================

/// A playlist-pos change pops the passed entry and advances the current
/// track/queue position — but emits NO TrackFinished. The credit belongs to
/// the end-file(eof) event, which finds the popped track by entry id.
#[test]
fn playlist_pos_pops_without_credit_and_eof_credits_by_entry_id() {
    let idle = Arc::new(AtomicBool::new(false));
    let (_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(3, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    handle_mpv_event(
        &mut state,
        MpvEvent::PropertyChange {
            name: "playlist-pos".to_string(),
            data: Value::Number(1.into()),
        },
        &cb,
        &ipc,
    );

    assert_eq!(
        state.current_track.as_ref().map(|t| t.jellyfin_id.as_str()),
        Some("track-1"),
        "current_track must be track-1"
    );
    assert_eq!(state.queue_position, Some(1), "queue_position must be 1");
    assert!(
        track_finished_ids(&events).is_empty(),
        "a position pop alone must not credit a listen"
    );

    // The eof for the popped entry (id 1) arrives afterwards: the credit
    // resolves through the recently-popped park.
    handle_mpv_event(
        &mut state,
        MpvEvent::EndFile {
            reason: "eof".to_string(),
            playlist_entry_id: Some(1),
            file_error: None,
        },
        &cb,
        &ipc,
    );

    assert_eq!(
        track_finished_ids(&events),
        vec!["track-0"],
        "eof credits exactly the popped entry's track"
    );
}

/// An eof arriving BEFORE the position change (ordering is not guaranteed)
/// credits via the live tracking queue.
#[test]
fn eof_before_position_change_credits_current_entry() {
    let idle = Arc::new(AtomicBool::new(false));
    let (_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(3, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    handle_mpv_event(
        &mut state,
        MpvEvent::EndFile {
            reason: "eof".to_string(),
            playlist_entry_id: Some(1),
            file_error: None,
        },
        &cb,
        &ipc,
    );

    assert_eq!(track_finished_ids(&events), vec!["track-0"]);
}

// ============================================================================
// Repeat modes and queue semantics
// ============================================================================

/// With `repeat=One`, a MANUAL SkipNext advances to the next distinct track —
/// repeat-one applies to natural ends only.
#[test]
fn manual_skip_advances_under_repeat_one() {
    let idle = Arc::new(AtomicBool::new(false));
    let (socket_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(3, 0);
    state.repeat = RepeatMode::One;
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    let is_running = Arc::new(AtomicBool::new(true));
    let mut mpv_opt: Option<MpvIpc> = Some(ipc);
    let mut mpv_process: Option<std::process::Child> = None;

    handle_command(
        AudioCommand::SkipNext,
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );

    assert_eq!(
        state.queue_position,
        Some(1),
        "SkipNext must advance past RepeatOne to position 1"
    );
}

/// Under `repeat=One`, a natural end replays the same track and credits the
/// completed loop.
#[test]
fn natural_end_under_repeat_one_replays_and_credits() {
    let idle = Arc::new(AtomicBool::new(true));
    let (_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(2, 5_000);
    state.repeat = RepeatMode::One;
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    handle_mpv_event(
        &mut state,
        MpvEvent::PropertyChange {
            name: "idle-active".to_string(),
            data: Value::Bool(true),
        },
        &cb,
        &ipc,
    );

    assert_eq!(
        state.queue_position,
        Some(0),
        "queue_position must stay at 0 under RepeatOne"
    );

    let urls = loadfile_urls(&log);
    assert!(
        urls.iter().any(|u| u.contains("0.flac")),
        "loadfile for queue[0] must be sent for replay; got {:?}",
        urls
    );

    let finished = track_finished_ids(&events);
    assert!(
        finished.iter().any(|id| id == "track-0"),
        "TrackFinished for track-0 must be emitted; got {:?}",
        finished
    );
}

/// With queue [A, B] at position 0, `Play{track: C, clear_queue: false}` must
/// insert C after the current position, yielding [A, C, B], and move to it.
#[test]
fn play_without_clear_queue_inserts_after_current() {
    let idle = Arc::new(AtomicBool::new(false));
    let (socket_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(2, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    let is_running = Arc::new(AtomicBool::new(true));
    let mut mpv_opt: Option<MpvIpc> = Some(ipc);
    let mut mpv_process: Option<std::process::Child> = None;

    let (track_c, url_c) = make_track(99);
    let c_id = track_c.jellyfin_id.clone();

    handle_command(
        AudioCommand::Play {
            track: track_c,
            stream_url: url_c,
            clear_queue: false,
        },
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );

    let ids: Vec<&str> = state
        .queue
        .iter()
        .map(|q| q.track.jellyfin_id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec!["track-0", "track-99", "track-1"],
        "queue must be [A, C, B] after Play{{clear_queue:false}}"
    );
    assert_eq!(
        state.queue_position,
        Some(1),
        "queue_position must advance to C at index 1"
    );
    assert_eq!(
        state.current_track.as_ref().map(|t| t.jellyfin_id.as_str()),
        Some(c_id.as_str()),
        "current_track must be C"
    );
}

// ============================================================================
// Up Next panel: JumpToQueueIndex, the one queue-navigation
// path the double-click/Return activation shares with SkipNext/SkipPrevious,
// and the QueueEntrySource carried on every queue snapshot/event so the panel
// can group "Playing Next" without a second, locally-held queue.
// ============================================================================

/// Jumping to a later index sets `queue_position` there and starts that
/// track through the same `play_track` path a skip uses.
#[test]
fn jump_to_queue_index_sets_position_and_starts_playback() {
    let idle = Arc::new(AtomicBool::new(false));
    let (socket_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(3, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    let is_running = Arc::new(AtomicBool::new(true));
    let mut mpv_opt: Option<MpvIpc> = Some(ipc);
    let mut mpv_process: Option<std::process::Child> = None;

    handle_command(
        AudioCommand::JumpToQueueIndex(2),
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );

    assert_eq!(
        state.queue_position,
        Some(2),
        "queue_position must jump straight to the clicked index"
    );
    assert_eq!(
        state.current_track.as_ref().map(|t| t.jellyfin_id.as_str()),
        Some("track-2"),
        "current_track must be the jumped-to entry"
    );
    let urls = loadfile_urls(&log);
    assert!(
        urls.iter().any(|u| u.contains("2.flac")),
        "loadfile for queue[2] must be sent; got {:?}",
        urls
    );
}

/// Jumping to the already-current entry is a no-op — the panel never lists
/// the current track as a jump target, but the guard must hold regardless.
#[test]
fn jump_to_queue_index_on_current_position_is_a_no_op() {
    let idle = Arc::new(AtomicBool::new(false));
    let (socket_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(3, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    let is_running = Arc::new(AtomicBool::new(true));
    let mut mpv_opt: Option<MpvIpc> = Some(ipc);
    let mut mpv_process: Option<std::process::Child> = None;

    handle_command(
        AudioCommand::JumpToQueueIndex(0),
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );

    assert_eq!(state.queue_position, Some(0), "position must be unchanged");
    assert!(
        loadfile_urls(&log).is_empty(),
        "no loadfile must be sent for a no-op jump"
    );
}

/// An out-of-bounds index is ignored rather than panicking or clobbering state.
#[test]
fn jump_to_queue_index_out_of_bounds_is_ignored() {
    let idle = Arc::new(AtomicBool::new(false));
    let (socket_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(3, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    let is_running = Arc::new(AtomicBool::new(true));
    let mut mpv_opt: Option<MpvIpc> = Some(ipc);
    let mut mpv_process: Option<std::process::Child> = None;

    handle_command(
        AudioCommand::JumpToQueueIndex(99),
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );

    assert_eq!(state.queue_position, Some(0), "position must be unchanged");
    assert!(
        loadfile_urls(&log).is_empty(),
        "no loadfile must be sent for an out-of-bounds jump"
    );
}

/// `snapshot_queue_state` carries a `sources` entry per track, converted 1:1
/// from the engine's internal `QueueSource` — what the Up Next panel groups
/// "Playing Next" (user-queued) above the rest of the context on.
#[test]
fn snapshot_queue_state_carries_source_per_entry() {
    let mut state = make_state_n(2, 0);
    let (track, url) = make_track(50);
    state.queue.push(QueuedTrack {
        track,
        stream_url: url,
        source: QueueSource::UserQueued,
    });
    state.original_queue_order.push(2);

    let snapshot = snapshot_queue_state(&state);

    assert_eq!(
        snapshot.sources,
        vec![
            QueueEntrySource::Context,
            QueueEntrySource::Context,
            QueueEntrySource::UserQueued,
        ],
        "sources must mirror each entry's QueueSource in queue order"
    );
}

// ============================================================================
// PlayContext: the one mechanism every "play this row, queuing the
// rest of the list" entry point goes through, replacing the old
// Play{clear_queue:true} + AddToQueue two-call dance.
// ============================================================================

/// PlayContext must build the WHOLE queue (including tracks before
/// `start_index`) in one call and start playback at `start_index` — the old
/// two-call dance only ever sent the tail (`tracks.slice(startIndex + 1)`),
/// so tracks before the clicked row were never in the backend queue and
/// Previous had nothing to go back to.
#[test]
fn play_context_builds_whole_queue_and_plays_start_index() {
    let idle = Arc::new(AtomicBool::new(false));
    let (socket_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = EngineState::default();
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    let is_running = Arc::new(AtomicBool::new(true));
    let mut mpv_opt: Option<MpvIpc> = Some(ipc);
    let mut mpv_process: Option<std::process::Child> = None;

    let tracks: Vec<(Track, String)> = (0..4).map(make_track).collect();

    handle_command(
        AudioCommand::PlayContext {
            tracks: tracks.clone(),
            start_index: 2,
            shuffle: false,
        },
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );

    let ids: Vec<&str> = state
        .queue
        .iter()
        .map(|q| q.track.jellyfin_id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec!["track-0", "track-1", "track-2", "track-3"],
        "the whole context (including tracks before start_index) must be queued"
    );
    assert_eq!(
        state.queue_position,
        Some(2),
        "playback must start at start_index"
    );
    assert_eq!(
        state.current_track.as_ref().map(|t| t.jellyfin_id.as_str()),
        Some("track-2"),
        "the track at start_index must be the one actually playing"
    );
}

/// Tracks before `start_index` land in the queue, so Previous can navigate
/// back into the context a row was played from (Apple Music/Spotify: playing
/// a row from the middle of a list still lets you step back through what came
/// before it) — the old Play + AddToQueue dance never queued them at all.
#[test]
fn play_context_keeps_prior_tracks_reachable_by_previous() {
    let idle = Arc::new(AtomicBool::new(false));
    let (socket_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = EngineState::default();
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    let is_running = Arc::new(AtomicBool::new(true));
    let mut mpv_opt: Option<MpvIpc> = Some(ipc);
    let mut mpv_process: Option<std::process::Child> = None;

    let tracks: Vec<(Track, String)> = (0..3).map(make_track).collect();

    handle_command(
        AudioCommand::PlayContext {
            tracks,
            start_index: 2,
            shuffle: false,
        },
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );
    // play_track_at_position just started the track at position 0ms, well
    // under the 3s "restart current track instead" threshold, so
    // SkipPrevious below goes to the previous queue entry.

    handle_command(
        AudioCommand::SkipPrevious,
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );

    assert_eq!(
        state.queue_position,
        Some(1),
        "SkipPrevious must reach track-1, which PlayContext queued ahead of start_index"
    );
}

/// Replaces any existing queue outright (unlike `Play{clear_queue:false}`,
/// which inserts after the current track): a fresh "play this list" always
/// replaces the whole context, matching every existing PlayContext call site
/// (`startPlayback` never keeps a stale queue around).
#[test]
fn play_context_replaces_an_existing_queue() {
    let idle = Arc::new(AtomicBool::new(false));
    let (socket_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(3, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    let is_running = Arc::new(AtomicBool::new(true));
    let mut mpv_opt: Option<MpvIpc> = Some(ipc);
    let mut mpv_process: Option<std::process::Child> = None;

    let new_tracks: Vec<(Track, String)> = (50..52).map(make_track).collect();

    handle_command(
        AudioCommand::PlayContext {
            tracks: new_tracks,
            start_index: 0,
            shuffle: false,
        },
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );

    let ids: Vec<&str> = state
        .queue
        .iter()
        .map(|q| q.track.jellyfin_id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec!["track-50", "track-51"],
        "PlayContext must replace the old queue, not append to it"
    );
}

/// A not-yet-played "Play Next"/"Add to Queue" item survives a new
/// PlayContext, landing right after the new current track and before the
/// rest of the new context — Spotify's "Next in queue" plays before a newly
/// started context continues, instead of vanishing under it. An
/// ALREADY-PLAYED UserQueued entry (behind the old position) is history and
/// is correctly dropped along with the rest of the old queue.
#[test]
fn play_context_keeps_unplayed_user_queued_items_ahead_of_new_context() {
    let idle = Arc::new(AtomicBool::new(false));
    let (socket_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));

    // Old queue: [track-5 (UserQueued, already played), track-0 (Context,
    // playing), track-1 (Context, unplayed), track-99 (UserQueued, unplayed)].
    let mut state = EngineState::default();
    let (played_queued_track, played_queued_url) = make_track(5);
    state.queue.push(QueuedTrack {
        track: played_queued_track,
        stream_url: played_queued_url,
        source: QueueSource::UserQueued,
    });
    let (t0, u0) = make_track(0);
    state.queue.push(QueuedTrack {
        track: t0,
        stream_url: u0,
        source: QueueSource::Context,
    });
    let (t1, u1) = make_track(1);
    state.queue.push(QueuedTrack {
        track: t1,
        stream_url: u1,
        source: QueueSource::Context,
    });
    let (unplayed_queued_track, unplayed_queued_url) = make_track(99);
    state.queue.push(QueuedTrack {
        track: unplayed_queued_track,
        stream_url: unplayed_queued_url,
        source: QueueSource::UserQueued,
    });
    state.original_queue_order = (0..4).collect();
    state.queue_position = Some(1);
    state.current_track = Some(state.queue[1].track.clone());
    state.is_playing = true;
    state.track_start_time = Some(Instant::now());

    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));
    let is_running = Arc::new(AtomicBool::new(true));
    let mut mpv_opt: Option<MpvIpc> = Some(ipc);
    let mut mpv_process: Option<std::process::Child> = None;

    let new_tracks: Vec<(Track, String)> = (50..52).map(make_track).collect();

    handle_command(
        AudioCommand::PlayContext {
            tracks: new_tracks,
            start_index: 0,
            shuffle: false,
        },
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );

    let ids: Vec<&str> = state
        .queue
        .iter()
        .map(|q| q.track.jellyfin_id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec!["track-50", "track-99", "track-51"],
        "the unplayed user-queued track must sit right after the new current \
         track, ahead of the rest of the new context; the already-played \
         user-queued track and the old context's own tail must not reappear"
    );
    assert_eq!(state.queue_position, Some(0));
}

/// With shuffle already on, PlayContext must build an already-shuffled queue
/// in one shot rather than broadcasting an unshuffled queue and then a
/// second, shuffled one a moment later.
#[test]
fn play_context_applies_shuffle_before_the_first_broadcast() {
    let idle = Arc::new(AtomicBool::new(false));
    let (socket_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = EngineState::default();
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));
    let is_running = Arc::new(AtomicBool::new(true));
    let mut mpv_opt: Option<MpvIpc> = Some(ipc);
    let mut mpv_process: Option<std::process::Child> = None;

    let tracks: Vec<(Track, String)> = (0..8).map(make_track).collect();

    handle_command(
        AudioCommand::PlayContext {
            tracks,
            start_index: 0,
            shuffle: true,
        },
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );

    assert!(
        state.shuffle,
        "PlayContext must record the shuffle flag it was given"
    );
    assert_eq!(
        state.queue[0].track.jellyfin_id, "track-0",
        "the start track stays pinned first, same as toggling shuffle on live"
    );
    // Every track is still present exactly once (a permutation, not a rebuild).
    assert_eq!(sorted_queue_ids(&state), track_ids(8));
    // original_queue_order tracks each slot's origin so a later disable restores.
    let mut order = state.original_queue_order.clone();
    order.sort_unstable();
    assert_eq!(order, (0..8).collect::<Vec<usize>>());

    let queue_changed_count = events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| matches!(e, AudioEvent::QueueChanged { .. }))
        .count();
    assert_eq!(
        queue_changed_count, 1,
        "shuffle must be applied before the first broadcast: a caller with \
         shuffle already on must see exactly one (already-shuffled) \
         queue-changed event, never an unshuffled one immediately followed \
         by a second, shuffled one"
    );
}

/// An empty context must clear the queue cleanly rather than panicking on an
/// out-of-bounds start_index lookup.
#[test]
fn play_context_with_empty_tracks_clears_the_queue_without_playing() {
    let idle = Arc::new(AtomicBool::new(false));
    let (socket_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));

    // A pre-existing queue, to prove PlayContext actually clears it.
    let mut state = make_state_n(3, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));
    let is_running = Arc::new(AtomicBool::new(true));
    let mut mpv_opt: Option<MpvIpc> = Some(ipc);
    let mut mpv_process: Option<std::process::Child> = None;

    handle_command(
        AudioCommand::PlayContext {
            tracks: vec![],
            start_index: 0,
            shuffle: false,
        },
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );

    assert!(
        state.queue.is_empty(),
        "an empty context must clear the queue, not panic or leave stale entries"
    );
    assert_eq!(state.queue_position, None);
    assert!(state.original_queue_order.is_empty());
}

/// An out-of-range start_index (a stale index from a list that shrank
/// between click and dispatch) must clamp to the last track rather than
/// panicking on an out-of-bounds queue lookup.
#[test]
fn play_context_clamps_an_out_of_range_start_index_to_the_last_track() {
    let idle = Arc::new(AtomicBool::new(false));
    let (socket_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = EngineState::default();
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));
    let is_running = Arc::new(AtomicBool::new(true));
    let mut mpv_opt: Option<MpvIpc> = Some(ipc);
    let mut mpv_process: Option<std::process::Child> = None;

    let tracks: Vec<(Track, String)> = (0..3).map(make_track).collect();

    handle_command(
        AudioCommand::PlayContext {
            tracks,
            start_index: 99,
            shuffle: false,
        },
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );

    assert_eq!(
        state.queue_position,
        Some(2),
        "an out-of-range start_index must clamp to the last track, not panic"
    );
    assert_eq!(
        state.current_track.as_ref().map(|t| t.jellyfin_id.as_str()),
        Some("track-2")
    );
}

// ============================================================================
// Cumulative listened time
// ============================================================================

/// An uninterrupted end-to-end play still credits the full duration, so a
/// normal listen scrobbles exactly as before (acceptance b, no regression).
#[test]
fn full_play_at_eof_credits_full_duration() {
    let idle = Arc::new(AtomicBool::new(false));
    let (_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(1, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    handle_mpv_event(
        &mut state,
        MpvEvent::EndFile {
            reason: "eof".to_string(),
            playlist_entry_id: Some(1),
            file_error: None,
        },
        &cb,
        &ipc,
    );

    assert_eq!(
        finished_played_ms(&events),
        vec![("track-0".to_string(), 180_000)],
        "end-to-end play must credit full duration"
    );
}

/// Seeking to the tail and letting the track finish credits only the audio
/// actually heard (2s head + 3s tail), well under full duration (acceptance a).
#[test]
fn seek_to_tail_then_finish_credits_only_played_span() {
    let idle = Arc::new(AtomicBool::new(false));
    let (_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(1, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    // Play the first 2s, then jump to 3s before the end and let it finish.
    simulate_played_to(&mut state, 2_000);
    handle_seek(&mut state, 177_000, &cb, &ipc);
    handle_mpv_event(
        &mut state,
        MpvEvent::EndFile {
            reason: "eof".to_string(),
            playlist_entry_id: Some(1),
            file_error: None,
        },
        &cb,
        &ipc,
    );

    let credited = finished_played_ms(&events);
    assert_eq!(
        credited,
        vec![("track-0".to_string(), 5_000)],
        "only the 2s head + 3s tail actually heard may be credited"
    );
    assert!(credited[0].1 < 180_000, "must be well under full duration");
}

/// Listened time accumulates across multiple seeks: three 10s spans plus a 5s
/// final tail credit 35s, not the full duration.
#[test]
fn multi_seek_accumulates_each_played_span() {
    let idle = Arc::new(AtomicBool::new(false));
    let (_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(1, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    // 0..10s, jump to 60s; 60..70s, jump to 120s; 120..130s, jump to 175s; end.
    simulate_played_to(&mut state, 10_000);
    handle_seek(&mut state, 60_000, &cb, &ipc);
    simulate_played_to(&mut state, 70_000);
    handle_seek(&mut state, 120_000, &cb, &ipc);
    simulate_played_to(&mut state, 130_000);
    handle_seek(&mut state, 175_000, &cb, &ipc);
    handle_mpv_event(
        &mut state,
        MpvEvent::EndFile {
            reason: "eof".to_string(),
            playlist_entry_id: Some(1),
            file_error: None,
        },
        &cb,
        &ipc,
    );

    assert_eq!(
        finished_played_ms(&events),
        vec![("track-0".to_string(), 35_000)],
        "three 10s spans + a 5s tail must sum to 35s"
    );
}

/// Gapless Case 2: the position change pops the outgoing (seeked) track and
/// resets accounting for the incoming one BEFORE its eof arrives. The parked
/// seek-aware credit must still be used, not the reset-to-full-duration value.
#[test]
fn gapless_seeked_track_credits_played_span_when_pos_change_precedes_eof() {
    let idle = Arc::new(AtomicBool::new(false));
    let (_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(3, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    // Track-0: play 2s, seek to 3s-before-end (heard span = 5s), then let mpv
    // gaplessly advance (pos change to 1) and only afterwards deliver eof for 0.
    simulate_played_to(&mut state, 2_000);
    handle_seek(&mut state, 177_000, &cb, &ipc);
    handle_mpv_event(
        &mut state,
        MpvEvent::PropertyChange {
            name: "playlist-pos".to_string(),
            data: Value::Number(1.into()),
        },
        &cb,
        &ipc,
    );
    handle_mpv_event(
        &mut state,
        MpvEvent::EndFile {
            reason: "eof".to_string(),
            playlist_entry_id: Some(1),
            file_error: None,
        },
        &cb,
        &ipc,
    );

    assert_eq!(
        finished_played_ms(&events),
        vec![("track-0".to_string(), 5_000)],
        "the seeked track must credit its 5s heard span, not full duration"
    );
}

// ============================================================================
// Fade on pause/stop/resume
// ============================================================================

/// Pause with fade enabled flips `is_playing` immediately (capturing the
/// pre-fade position for listened-time accounting), starts a PauseOut ramp, emits a strictly
/// descending volume ramp, and pauses mpv only once the ramp completes.
#[test]
fn fade_pause_emits_descending_ramp_then_pause() {
    let idle = Arc::new(AtomicBool::new(false));
    let (socket_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(1, 0);
    state.fade_enabled = true;
    state.fade_duration_ms = 400;
    simulate_played_to(&mut state, 30_000); // pin position at 30s
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    let is_running = Arc::new(AtomicBool::new(true));
    let mut mpv_opt: Option<MpvIpc> = Some(ipc);
    let mut mpv_process: Option<std::process::Child> = None;

    handle_command(
        AudioCommand::Pause,
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );

    assert!(
        !state.is_playing,
        "pause flips is_playing immediately for the UI"
    );
    assert_eq!(
        state.paused_position_ms, 30_000,
        "position captured BEFORE flipping is_playing/track_start_time"
    );
    assert!(state.track_start_time.is_none());
    assert_eq!(
        state.active_fade.as_ref().map(|f| f.kind),
        Some(FadeKind::PauseOut),
        "a PauseOut fade must be active"
    );

    let ipc = mpv_opt.take().unwrap();

    // Tick 1 (100ms of 400 → factor 0.75).
    state.active_fade.as_mut().unwrap().start = Instant::now() - Duration::from_millis(100);
    advance_fade(&mut state, &ipc, &cb);
    // Tick 2 (250ms → factor 0.375).
    state.active_fade.as_mut().unwrap().start = Instant::now() - Duration::from_millis(250);
    advance_fade(&mut state, &ipc, &cb);
    assert!(
        state.active_fade.is_some(),
        "fade still in progress before completion"
    );
    // Tick 3 (500ms → complete).
    state.active_fade.as_mut().unwrap().start = Instant::now() - Duration::from_millis(500);
    advance_fade(&mut state, &ipc, &cb);
    assert!(state.active_fade.is_none(), "fade cleared at completion");

    let vols = volume_writes(&log);
    assert!(vols.len() >= 3, "expected ramp writes; got {:?}", vols);
    assert!(
        vols[0] > vols[1],
        "volume must strictly descend during the ramp; got {:?}",
        vols
    );
    assert!(
        vols[1] < 100,
        "mid-ramp volume must be below base; got {:?}",
        vols
    );
    assert_eq!(
        *vols.last().unwrap(),
        100,
        "completion restores the base volume"
    );

    assert_eq!(
        pause_writes(&log),
        vec![true],
        "mpv is paused exactly once, at ramp completion"
    );
}

/// Resume with fade enabled unpauses mpv, restores the resume bookkeeping
/// (position advances, not frozen), starts a ResumeIn ramp from silence, and
/// ascends back to the base volume.
#[test]
fn fade_resume_ramps_up_and_restores_position_bookkeeping() {
    let idle = Arc::new(AtomicBool::new(false));
    let (socket_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(1, 0);
    state.fade_enabled = true;
    state.fade_duration_ms = 400;
    // Paused at 30s.
    state.is_playing = false;
    state.track_start_time = None;
    state.paused_position_ms = 30_000;
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    let is_running = Arc::new(AtomicBool::new(true));
    let mut mpv_opt: Option<MpvIpc> = Some(ipc);
    let mut mpv_process: Option<std::process::Child> = None;

    handle_command(
        AudioCommand::Resume,
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );

    assert!(state.is_playing, "resume sets is_playing");
    assert!(
        state.track_start_time.is_some(),
        "track_start_time reset so position advances again"
    );
    assert_eq!(
        state.track_start_position_ms, 30_000,
        "resume re-pins the position to the paused offset, not 0"
    );
    assert_eq!(
        state.active_fade.as_ref().map(|f| f.kind),
        Some(FadeKind::ResumeIn),
        "a ResumeIn fade must be active"
    );
    assert_eq!(pause_writes(&log), vec![false], "resume unpauses mpv");
    assert_eq!(
        volume_writes(&log).first(),
        Some(&0),
        "resume fade starts from silence"
    );

    let ipc = mpv_opt.take().unwrap();
    state.active_fade.as_mut().unwrap().start = Instant::now() - Duration::from_millis(100);
    advance_fade(&mut state, &ipc, &cb);
    state.active_fade.as_mut().unwrap().start = Instant::now() - Duration::from_millis(500);
    advance_fade(&mut state, &ipc, &cb);
    assert!(state.active_fade.is_none(), "fade cleared at completion");

    let vols = volume_writes(&log);
    assert!(
        vols.windows(2).all(|w| w[1] >= w[0]),
        "resume volumes must ascend; got {:?}",
        vols
    );
    assert_eq!(
        *vols.last().unwrap(),
        100,
        "resume restores the base volume"
    );
}

/// With fade disabled, pause/resume are instant (no ramp, no active fade) and
/// the full pause/resume bookkeeping still fires — the non-fade path must
/// be untouched.
#[test]
fn fade_disabled_is_instant() {
    let idle = Arc::new(AtomicBool::new(false));
    let (socket_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(1, 0);
    state.fade_enabled = false;
    simulate_played_to(&mut state, 42_000);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    let is_running = Arc::new(AtomicBool::new(true));
    let mut mpv_opt: Option<MpvIpc> = Some(ipc);
    let mut mpv_process: Option<std::process::Child> = None;

    handle_command(
        AudioCommand::Pause,
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );
    assert!(state.active_fade.is_none(), "no fade when disabled");
    assert!(
        volume_writes(&log).is_empty(),
        "no volume ramp when fade disabled"
    );
    assert_eq!(pause_writes(&log), vec![true], "instant mpv pause");

    // The async mpv pause event drives the pause bookkeeping (regression
    // guard: the non-fade path is exactly as before).
    let ipc_ref = mpv_opt.as_ref().unwrap();
    handle_mpv_event(
        &mut state,
        MpvEvent::PropertyChange {
            name: "pause".to_string(),
            data: Value::Bool(true),
        },
        &cb,
        ipc_ref,
    );
    assert!(!state.is_playing, "pause event flips is_playing");
    assert_eq!(
        state.paused_position_ms, 42_000,
        "paused position captured on pause"
    );
    assert!(
        state.track_start_time.is_none(),
        "track_start_time cleared on pause"
    );

    // Resume with fade disabled = plain unpause + full resume bookkeeping.
    handle_command(
        AudioCommand::Resume,
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );
    assert!(
        state.active_fade.is_none(),
        "no fade on resume when disabled"
    );
    assert!(
        volume_writes(&log).is_empty(),
        "still no volume ramp on resume"
    );
    assert!(state.is_playing, "resume sets is_playing");
    assert!(
        state.track_start_time.is_some(),
        "track_start_time set on resume"
    );
    assert_eq!(
        state.track_start_position_ms, 42_000,
        "resume re-pins to the paused position"
    );
    assert_eq!(
        pause_writes(&log),
        vec![true, false],
        "instant pause then unpause, no fade in between"
    );
}

/// A manual SkipNext never fades: it cancels any in-progress fade and loads the
/// next track instantly.
#[test]
fn skip_does_not_fade() {
    let idle = Arc::new(AtomicBool::new(false));
    let (socket_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(3, 0);
    state.fade_enabled = true;
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    let is_running = Arc::new(AtomicBool::new(true));
    let mut mpv_opt: Option<MpvIpc> = Some(ipc);
    let mut mpv_process: Option<std::process::Child> = None;

    handle_command(
        AudioCommand::SkipNext,
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );

    assert!(state.active_fade.is_none(), "skip must not start a fade");
    assert_eq!(state.queue_position, Some(1), "skip advances to queue[1]");
    // No fade-out ramp: skip is instant. play_track may toggle mpv's own pause
    // to (re)start the new file, but there must be no PauseOut ramp — the fade
    // state is what we assert on above.
    let urls = loadfile_urls(&log);
    assert!(
        urls.iter().any(|u| u.contains("1.flac")),
        "skip loads queue[1] instantly; got {:?}",
        urls
    );
}

/// A Seek cancels any in-progress fade (restoring the base volume) and issues an
/// immediate mpv seek — scrubbing must never be left stuck at a low volume.
#[test]
fn seek_cancels_fade_and_stays_instant() {
    let idle = Arc::new(AtomicBool::new(false));
    let (socket_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(1, 0);
    state.fade_enabled = true;
    simulate_played_to(&mut state, 10_000);
    // A pause fade is mid-ramp.
    state.is_playing = false;
    state.active_fade = Some(ActiveFade {
        kind: FadeKind::PauseOut,
        start: Instant::now(),
        duration_ms: 400,
    });
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    let is_running = Arc::new(AtomicBool::new(true));
    let mut mpv_opt: Option<MpvIpc> = Some(ipc);
    let mut mpv_process: Option<std::process::Child> = None;

    handle_command(
        AudioCommand::Seek(20_000),
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );

    assert!(
        state.active_fade.is_none(),
        "seek cancels any in-progress fade"
    );
    assert_eq!(
        volume_writes(&log).last(),
        Some(&100),
        "cancelling the fade restores the base volume"
    );
    let seeked = log.lock().unwrap().iter().any(|cmd| {
        cmd.get("command")
            .and_then(|a| a.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
            == Some("seek")
    });
    assert!(seeked, "an absolute seek must be issued instantly");
}

/// A stream failure mid-fade restores the base volume (rather than leaving
/// playback silent after recovery) and clears the fade.
#[test]
fn fault_mid_fade_restores_volume() {
    let idle = Arc::new(AtomicBool::new(false));
    let (_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(2, 30_000);
    state.fade_enabled = true;
    state.is_loading = false;
    // A pause fade is mid-ramp when the stream faults.
    state.is_playing = false;
    state.active_fade = Some(ActiveFade {
        kind: FadeKind::PauseOut,
        start: Instant::now(),
        duration_ms: 400,
    });
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    let front = front_entry_id(&state);
    handle_mpv_event(
        &mut state,
        MpvEvent::EndFile {
            reason: "error".to_string(),
            playlist_entry_id: front,
            file_error: Some("Cannot open stream".to_string()),
        },
        &cb,
        &ipc,
    );

    assert!(
        state.active_fade.is_none(),
        "a stream failure clears the in-progress fade"
    );
    assert_eq!(
        volume_writes(&log).last(),
        Some(&100),
        "volume restored to base after a fault mid-fade (no silent playback)"
    );
}

/// Stop with fade enabled begins a StopOut ramp (flipping is_playing for the
/// UI but keeping the track until the ramp end); only the completing
/// advance_fade tick issues the real mpv stop and clears playback state.
#[test]
fn stop_fades_then_stops_at_ramp_end() {
    let idle = Arc::new(AtomicBool::new(false));
    let (socket_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(2, 0);
    state.fade_enabled = true;
    state.fade_duration_ms = 400;
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    let is_running = Arc::new(AtomicBool::new(true));
    let mut mpv_opt: Option<MpvIpc> = Some(ipc);
    let mut mpv_process: Option<std::process::Child> = None;

    handle_command(
        AudioCommand::Stop,
        &mut state,
        &mut mpv_opt,
        &mut mpv_process,
        &socket_path,
        &cb,
        &is_running,
    );

    assert!(!state.is_playing, "stop flips is_playing immediately");
    assert!(
        state.current_track.is_some(),
        "the track is kept until the ramp completes"
    );
    assert_eq!(
        state.active_fade.as_ref().map(|f| f.kind),
        Some(FadeKind::StopOut),
        "a StopOut fade must be active"
    );

    let ipc = mpv_opt.take().unwrap();
    let stops_before = log
        .lock()
        .unwrap()
        .iter()
        .filter(|cmd| {
            cmd.get("command")
                .and_then(|a| a.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())
                == Some("stop")
        })
        .count();
    assert_eq!(stops_before, 0, "no mpv stop before the ramp completes");

    // Complete the ramp.
    state.active_fade.as_mut().unwrap().start = Instant::now() - Duration::from_millis(500);
    advance_fade(&mut state, &ipc, &cb);

    assert!(state.active_fade.is_none(), "fade cleared at completion");
    assert!(
        state.current_track.is_none(),
        "current_track cleared on stop"
    );
    assert!(
        state.mpv_playlist_tracks.is_empty(),
        "playlist mirror cleared"
    );
    let stops_after = log
        .lock()
        .unwrap()
        .iter()
        .filter(|cmd| {
            cmd.get("command")
                .and_then(|a| a.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())
                == Some("stop")
        })
        .count();
    assert_eq!(stops_after, 1, "mpv stopped exactly once, at the ramp end");
    assert_eq!(
        volume_writes(&log).last(),
        Some(&100),
        "volume restored to base for the next track"
    );
}

/// A gapless transition while a PauseOut/StopOut fade is active must clear the
/// stale fade so the incoming track is not faded out / paused by a fade that
/// belonged to the previous track.
#[test]
fn gapless_during_fade_does_not_hijack_next_track() {
    let idle = Arc::new(AtomicBool::new(false));
    let (_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(3, 0);
    state.fade_enabled = true;
    // A pause fade is mid-ramp when the current track ends and mpv gaplessly
    // advances to the next entry.
    state.active_fade = Some(ActiveFade {
        kind: FadeKind::PauseOut,
        start: Instant::now(),
        duration_ms: 400,
    });
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    handle_mpv_event(
        &mut state,
        MpvEvent::PropertyChange {
            name: "playlist-pos".to_string(),
            data: Value::Number(1.into()),
        },
        &cb,
        &ipc,
    );

    assert!(
        state.active_fade.is_none(),
        "the gapless transition clears the stale fade"
    );
    assert_eq!(
        state.current_track.as_ref().map(|t| t.jellyfin_id.as_str()),
        Some("track-1"),
        "playback advanced to the next track"
    );
    // The user's Pause (caught mid-fade) is HONOURED on the incoming track: it is
    // paused, not left playing under a paused UI (review fix).
    assert!(
        !state.is_playing,
        "the incoming track must be paused, not playing"
    );
    assert_eq!(
        pause_writes(&log).last(),
        Some(&true),
        "mpv must actually be paused for the honoured Pause"
    );

    // Any further engine tick must NOT ramp the new track (fade already cleared).
    let vols_before = volume_writes(&log).len();
    advance_fade(&mut state, &ipc, &cb);
    assert_eq!(
        volume_writes(&log).len(),
        vols_before,
        "no fade ramp applied to the new track"
    );
}

/// Stop pressed mid-fade, then the track ends and mpv gaplessly advances: the
/// Stop intent must be honoured (playback stops) rather than the next track
/// playing under a stopped UI (review fix).
#[test]
fn stop_during_fade_gapless_honours_stop() {
    let idle = Arc::new(AtomicBool::new(false));
    let (_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));

    let mut state = make_state_n(3, 0);
    state.fade_enabled = true;
    state.active_fade = Some(ActiveFade {
        kind: FadeKind::StopOut,
        start: Instant::now(),
        duration_ms: 400,
    });
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    handle_mpv_event(
        &mut state,
        MpvEvent::PropertyChange {
            name: "playlist-pos".to_string(),
            data: Value::Number(1.into()),
        },
        &cb,
        &ipc,
    );

    assert!(
        state.active_fade.is_none(),
        "the transition clears the stale fade"
    );
    assert!(
        !state.is_playing,
        "a superseded Stop must leave playback stopped"
    );
    assert!(
        state.current_track.is_none(),
        "a superseded Stop must clear the current track"
    );
}

// ============================================================================
// Shuffle permutation invariants
// ============================================================================

/// Expected `track-0..track-{n-1}` ids in original order.
fn track_ids(n: u32) -> Vec<String> {
    (0..n).map(|i| format!("track-{i}")).collect()
}

/// Sorted jellyfin ids of the current queue — used to assert an operation is a
/// pure permutation (no track lost or duplicated).
fn sorted_queue_ids(state: &EngineState) -> Vec<String> {
    let mut ids: Vec<String> = state
        .queue
        .iter()
        .map(|q| q.track.jellyfin_id.clone())
        .collect();
    ids.sort();
    ids
}

#[test]
fn shuffle_enable_pins_current_and_permutes_the_rest() {
    let mut state = make_state_n(6, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    assert!(
        handle_set_shuffle(&mut state, true, &cb),
        "enabling shuffle must report a change"
    );
    assert!(state.shuffle);
    assert_eq!(
        state.queue_position,
        Some(0),
        "current position is unchanged"
    );
    assert_eq!(
        state.queue[0].track.jellyfin_id, "track-0",
        "the currently-playing track stays pinned in place"
    );
    // Every track is still present exactly once (a permutation, not a rebuild).
    assert_eq!(sorted_queue_ids(&state), track_ids(6));
    // original_queue_order tracks each slot's origin so a later disable restores.
    let mut order = state.original_queue_order.clone();
    order.sort_unstable();
    assert_eq!(order, (0..6).collect::<Vec<usize>>());
}

#[test]
fn shuffle_disable_restores_order_and_follows_playing_track() {
    let mut state = make_state_n(6, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    assert!(handle_set_shuffle(&mut state, true, &cb));

    // Advance to whatever track now sits at index 3, as pressing Next would.
    let playing_id = state.queue[3].track.jellyfin_id.clone();
    state.queue_position = Some(3);
    state.current_track = Some(state.queue[3].track.clone());

    assert!(
        handle_set_shuffle(&mut state, false, &cb),
        "disabling while on must report a change"
    );
    assert!(!state.shuffle);
    let ids: Vec<String> = state
        .queue
        .iter()
        .map(|q| q.track.jellyfin_id.clone())
        .collect();
    assert_eq!(ids, track_ids(6), "disable restores exact original order");
    let pos = state.queue_position.expect("a position is kept");
    assert_eq!(
        state.queue[pos].track.jellyfin_id, playing_id,
        "position follows the playing track back to its original index"
    );
}

#[test]
fn shuffle_disable_when_already_off_is_a_noop() {
    let mut state = make_state_n(3, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    assert!(
        !handle_set_shuffle(&mut state, false, &cb),
        "disabling when already off must report no change"
    );
    assert!(!state.shuffle);
    assert_eq!(sorted_queue_ids(&state), track_ids(3));
    assert!(
        events.lock().unwrap().is_empty(),
        "a no-op must not emit state/queue events"
    );
}

#[test]
fn shuffle_reenable_repermutes_a_fresh_in_order_queue() {
    // Mirrors "Play All while shuffle is already on": shuffle is on, then
    // a brand-new queue is assembled in order (the Play + AddToQueue path rebuilds
    // original_queue_order to identity). Re-enabling must NOT no-op — it must
    // permute the fresh queue so Play All honours the shuffle toggle.
    let mut state = make_state_n(6, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    assert!(handle_set_shuffle(&mut state, true, &cb));
    assert!(state.shuffle);

    // Rebuild a fresh in-order queue, as Play(clear) + AddToQueue would.
    state.queue.clear();
    state.original_queue_order.clear();
    for i in 0..6u32 {
        let (track, url) = make_track(i);
        state.queue.push(QueuedTrack {
            track,
            stream_url: url,
            source: QueueSource::Context,
        });
        state.original_queue_order.push(i as usize);
    }
    state.queue_position = Some(0);
    state.current_track = state.queue.first().map(|q| q.track.clone());

    assert!(
        handle_set_shuffle(&mut state, true, &cb),
        "re-enabling while already on must reshuffle the fresh queue, not no-op"
    );
    assert_eq!(
        state.queue[0].track.jellyfin_id, "track-0",
        "the currently-playing track stays first"
    );
    assert_eq!(sorted_queue_ids(&state), track_ids(6));
    let mut order = state.original_queue_order.clone();
    order.sort_unstable();
    assert_eq!(
        order,
        (0..6).collect::<Vec<usize>>(),
        "original order remains a valid permutation for a later restore"
    );
}

#[test]
fn shuffle_single_track_queue_is_safe() {
    let mut state = make_state_n(1, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    assert!(handle_set_shuffle(&mut state, true, &cb));
    assert_eq!(state.queue.len(), 1);
    assert_eq!(state.queue[0].track.jellyfin_id, "track-0");
    assert!(handle_set_shuffle(&mut state, false, &cb));
    assert_eq!(state.queue[0].track.jellyfin_id, "track-0");
}

// ============================================================================
// Idle reconciliation
// ============================================================================

/// The one idle-active event can be swallowed by a guard; the level check
/// must still advance once mpv is seen idle on two consecutive polls, and
/// must not on a single reading or while mpv is live.
#[test]
fn reconcile_advances_when_mpv_is_idle_but_engine_thinks_playing() {
    let idle = Arc::new(AtomicBool::new(true));
    let (_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));
    let mut state = make_state_n(3, 15_000);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    reconcile_idle_playback(&mut state, &cb, &ipc);
    assert_eq!(
        state.queue_position,
        Some(0),
        "one idle reading is not enough"
    );

    reconcile_idle_playback(&mut state, &cb, &ipc);
    assert_eq!(state.queue_position, Some(1));
    assert!(loadfile_urls(&log)[0].contains("1.flac"));
}

#[test]
fn reconcile_leaves_live_or_loading_playback_alone() {
    let idle = Arc::new(AtomicBool::new(false));
    let (_path, log, ipc) = start_fake_mpv(Arc::clone(&idle));
    let mut state = make_state_n(3, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    for _ in 0..4 {
        reconcile_idle_playback(&mut state, &cb, &ipc);
    }
    idle.store(true, Ordering::SeqCst);
    state.is_loading = true;
    for _ in 0..4 {
        reconcile_idle_playback(&mut state, &cb, &ipc);
    }
    assert_eq!(state.queue_position, Some(0));
    assert!(loadfile_urls(&log).is_empty());
}

// ============================================================================
// Telemetry target
// ============================================================================

/// Targets of every event emitted while `f` runs.
fn targets_during(f: impl FnOnce()) -> Vec<String> {
    use tracing_subscriber::layer::{Context, SubscriberExt};
    use tracing_subscriber::{registry, Layer};

    #[derive(Default)]
    struct Targets(Arc<Mutex<Vec<String>>>);
    impl<S: tracing::Subscriber> Layer<S> for Targets {
        fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
            self.0
                .lock()
                .unwrap()
                .push(event.metadata().target().to_string());
        }
    }

    let targets = Targets::default();
    let seen = Arc::clone(&targets.0);
    tracing::subscriber::with_default(registry().with(targets), f);
    let seen = seen.lock().unwrap().clone();
    seen
}

/// The events that explain a silent stop must carry the allow-listed target,
/// or they stay on stderr and the next stop leaves no record.
#[test]
fn stop_diagnosis_events_carry_the_exported_target() {
    let idle = Arc::new(AtomicBool::new(true));
    let (_path, _log, ipc) = start_fake_mpv(Arc::clone(&idle));
    let mut state = make_state_n(3, 0);
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = make_callback(Arc::clone(&events));

    let targets = targets_during(|| {
        handle_mpv_event(
            &mut state,
            MpvEvent::EndFile {
                reason: "eof".to_string(),
                playlist_entry_id: Some(1),
                file_error: None,
            },
            &cb,
            &ipc,
        );
        reconcile_idle_playback(&mut state, &cb, &ipc);
        reconcile_idle_playback(&mut state, &cb, &ipc);
    });

    let exported = targets
        .iter()
        .filter(|t| t.as_str() == super::TARGET)
        .count();
    assert!(
        exported >= 2,
        "end-file and the idle reconcile must both export; saw {targets:?}"
    );
}
