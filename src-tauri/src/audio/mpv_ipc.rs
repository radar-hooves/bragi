//! mpv JSON IPC communication layer.
//!
//! Handles communication with mpv via Unix socket using the JSON IPC protocol.
//! See: https://mpv.io/manual/master/#json-ipc

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How long `command()` waits for mpv to reply before giving up (see
/// [`MpvIpc::response_timeout`]).
const DEFAULT_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a write to the mpv socket may block before giving up (see
/// [`MpvIpc::connect`]).
///
/// A wedged-but-still-connected mpv (its own thread stuck — e.g. after
/// sleep/wake, a dead audio device, or a PipeWire/PulseAudio restart — while
/// the socket itself stays open) never trips the read-side EOF/error detection
/// `is_connected()` relies on. Without a write timeout, `writeln!`/`flush()`
/// on a full kernel send buffer blocks forever, freezing the single-threaded
/// engine loop permanently: no command is ever dequeued again, and — because
/// nothing returns — no error is ever logged or surfaced to the frontend. This
/// is the root cause of "press play, nothing happens" after long uptime: the
/// command that hit the wedge appears to hang, and every command after it
/// (including the next several play presses) silently piles up in the
/// engine's unbounded channel, never executed.
const DEFAULT_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// How long `MpvIpc::connect` waits for a read before looping (lets the reader
/// thread poll `running` instead of blocking forever on a dead peer).
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_millis(100);

/// Consecutive response timeouts before the connection is declared dead.
///
/// A single slow reply is ordinary (network buffering, a cold transcode
/// start, a big seek) and must not itself look like a dead mpv — only a
/// write failure or write timeout, a read EOF or error, or SUSTAINED
/// non-response does. Matches the three-strikes convention already used for
/// stream-failure recovery elsewhere in this engine.
const MAX_CONSECUTIVE_RESPONSE_TIMEOUTS: u32 = 3;

/// mpv IPC client for sending commands and receiving events.
pub struct MpvIpc {
    /// Unix socket connection to mpv.
    socket: Arc<Mutex<UnixStream>>,
    /// Request ID counter for correlating responses.
    request_id: AtomicU64,
    /// Pending requests awaiting responses.
    pending_requests: Arc<Mutex<HashMap<u64, Sender<MpvResponse>>>>,
    /// Channel for receiving events from the reader thread.
    event_rx: Receiver<MpvEvent>,
    /// Reader thread handle.
    reader_handle: Option<JoinHandle<()>>,
    /// Flag to signal reader thread to stop.
    running: Arc<AtomicBool>,
    /// How long `command()` waits for a reply before giving up (shrunk in
    /// tests so a wedged-fake-mpv scenario doesn't need a real multi-second
    /// wait).
    response_timeout: Duration,
    /// Consecutive response timeouts since the last successful reply. Reset
    /// on every success; the connection is only declared dead once this
    /// reaches [`MAX_CONSECUTIVE_RESPONSE_TIMEOUTS`] — a single slow reply
    /// must not look like a dead mpv.
    consecutive_response_timeouts: AtomicU32,
}

/// Response from mpv for a command request.
#[derive(Debug, Clone, Deserialize)]
pub struct MpvResponse {
    /// Error message (empty string on success).
    #[serde(default)]
    pub error: String,
    /// Result data (if any).
    #[serde(default)]
    pub data: Value,
}

/// Event from mpv (property changes, end-file, etc.).
#[derive(Debug, Clone)]
pub enum MpvEvent {
    /// A property value changed.
    PropertyChange { name: String, data: Value },
    /// A file ended playback.
    EndFile {
        reason: String,
        playlist_entry_id: Option<i64>,
        file_error: Option<String>,
    },
    /// mpv started idling (nothing to play).
    Idle,
    /// A track started playing.
    StartFile { playlist_entry_id: Option<i64> },
    /// Playback was restarted (seek completed).
    PlaybackRestart,
    /// Unknown event type.
    Unknown(String),
}

/// Command to send to mpv.
#[derive(Debug, Serialize)]
struct MpvCommand {
    command: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<u64>,
}

/// Internal message from mpv (can be response or event).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MpvMessage {
    Response {
        request_id: u64,
        #[serde(default)]
        error: String,
        #[serde(default)]
        data: Value,
    },
    Event {
        event: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        data: Value,
        #[serde(default)]
        reason: Option<String>,
        #[serde(default)]
        playlist_entry_id: Option<i64>,
        #[serde(default)]
        file_error: Option<String>,
    },
}

impl MpvIpc {
    /// Connect to an mpv instance via Unix socket.
    pub fn connect(socket_path: &PathBuf) -> Result<Self, String> {
        Self::connect_with_timeouts(
            socket_path,
            DEFAULT_READ_TIMEOUT,
            DEFAULT_WRITE_TIMEOUT,
            DEFAULT_RESPONSE_TIMEOUT,
        )
    }

    /// Connect with explicit timeouts. Production always uses [`Self::connect`];
    /// tests shrink `write_timeout`/`response_timeout` so a wedged-fake-mpv
    /// regression test completes in milliseconds rather than seconds.
    pub(crate) fn connect_with_timeouts(
        socket_path: &PathBuf,
        read_timeout: Duration,
        write_timeout: Duration,
        response_timeout: Duration,
    ) -> Result<Self, String> {
        let socket = UnixStream::connect(socket_path)
            .map_err(|e| format!("Failed to connect to mpv socket: {}", e))?;

        // Read timeout lets the reader thread poll `running` instead of
        // blocking forever on a dead peer. Write timeout is the root-cause fix
        // above `DEFAULT_WRITE_TIMEOUT`: without it, a wedged-but-connected
        // mpv freezes the engine loop forever on the next `writeln!`/`flush()`.
        socket
            .set_read_timeout(Some(read_timeout))
            .map_err(|e| format!("Failed to set socket read timeout: {}", e))?;
        socket
            .set_write_timeout(Some(write_timeout))
            .map_err(|e| format!("Failed to set socket write timeout: {}", e))?;

        let socket = Arc::new(Mutex::new(socket));
        let pending_requests: Arc<Mutex<HashMap<u64, Sender<MpvResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let running = Arc::new(AtomicBool::new(true));

        // Channel for events
        let (event_tx, event_rx) = mpsc::channel();

        // Clone for reader thread
        let reader_socket = socket
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?
            .try_clone()
            .map_err(|e| format!("Failed to clone socket: {}", e))?;
        let reader_pending = Arc::clone(&pending_requests);
        let reader_running = Arc::clone(&running);

        // Start reader thread
        let reader_handle = thread::Builder::new()
            .name("mpv-reader".to_string())
            .spawn(move || {
                Self::reader_loop(reader_socket, reader_pending, event_tx, reader_running);
            })
            .map_err(|e| format!("Failed to spawn reader thread: {}", e))?;

        Ok(Self {
            socket,
            request_id: AtomicU64::new(1),
            pending_requests,
            response_timeout,
            consecutive_response_timeouts: AtomicU32::new(0),
            event_rx,
            reader_handle: Some(reader_handle),
            running,
        })
    }

    /// Reader loop that processes incoming messages from mpv.
    fn reader_loop(
        socket: UnixStream,
        pending_requests: Arc<Mutex<HashMap<u64, Sender<MpvResponse>>>>,
        event_tx: Sender<MpvEvent>,
        running: Arc<AtomicBool>,
    ) {
        let mut reader = BufReader::new(socket);
        let mut line = String::new();

        while running.load(Ordering::SeqCst) {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    // EOF - mpv closed the connection
                    tracing::info!("mpv socket closed");
                    running.store(false, Ordering::SeqCst);
                    break;
                }
                Ok(_) => {
                    if line.trim().is_empty() {
                        continue;
                    }

                    match serde_json::from_str::<MpvMessage>(&line) {
                        Ok(MpvMessage::Response {
                            request_id,
                            error,
                            data,
                        }) => {
                            // Send response to waiting request
                            if let Ok(mut pending) = pending_requests.lock() {
                                if let Some(tx) = pending.remove(&request_id) {
                                    let _ = tx.send(MpvResponse { error, data });
                                }
                            }
                        }
                        Ok(MpvMessage::Event {
                            event,
                            name,
                            data,
                            reason,
                            playlist_entry_id,
                            file_error,
                            ..
                        }) => {
                            let mpv_event = match event.as_str() {
                                "property-change" => {
                                    if let Some(name) = name {
                                        MpvEvent::PropertyChange { name, data }
                                    } else {
                                        continue;
                                    }
                                }
                                "end-file" => MpvEvent::EndFile {
                                    reason: reason.unwrap_or_default(),
                                    playlist_entry_id,
                                    file_error,
                                },
                                "idle" => MpvEvent::Idle,
                                "start-file" => MpvEvent::StartFile { playlist_entry_id },
                                "playback-restart" => MpvEvent::PlaybackRestart,
                                _ => MpvEvent::Unknown(event),
                            };
                            let _ = event_tx.send(mpv_event);
                        }
                        Err(e) => {
                            tracing::debug!("Failed to parse mpv message: {} - {}", e, line.trim());
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Timeout, continue loop
                    continue;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    // Timeout, continue loop
                    continue;
                }
                Err(e) => {
                    tracing::error!("Error reading from mpv socket: {}", e);
                    running.store(false, Ordering::SeqCst);
                    break;
                }
            }
        }

        tracing::debug!("mpv reader thread exiting");
    }

    /// Send a command to mpv and wait for a response.
    pub fn command(&self, args: Vec<Value>) -> Result<Value, String> {
        let request_id = self.request_id.fetch_add(1, Ordering::SeqCst);

        let cmd = MpvCommand {
            command: args,
            request_id: Some(request_id),
        };

        let json = serde_json::to_string(&cmd).map_err(|e| format!("JSON encode error: {}", e))?;

        // Create response channel
        let (tx, rx) = mpsc::channel();
        {
            let mut pending = self
                .pending_requests
                .lock()
                .map_err(|e| format!("Lock error: {}", e))?;
            pending.insert(request_id, tx);
        }

        // Send command
        {
            let mut socket = self
                .socket
                .lock()
                .map_err(|e| format!("Lock error: {}", e))?;
            if let Err(e) = writeln!(socket, "{}", json) {
                self.running.store(false, Ordering::SeqCst);
                return Err(format!("Write error: {}", e));
            }
            if let Err(e) = socket.flush() {
                self.running.store(false, Ordering::SeqCst);
                return Err(format!("Flush error: {}", e));
            }
        }

        // Wait for response with timeout
        let response = match rx.recv_timeout(self.response_timeout) {
            Ok(r) => {
                // A reply arrived — whatever run of slow replies preceded it
                // is over; the connection is healthy.
                self.consecutive_response_timeouts
                    .store(0, Ordering::SeqCst);
                r
            }
            Err(_) => {
                // Clean up pending request on timeout to prevent memory leak
                if let Ok(mut pending) = self.pending_requests.lock() {
                    pending.remove(&request_id);
                }
                // A write can succeed (mpv's kernel receive buffer had room)
                // while mpv is merely slow to reply — a big seek, a cold
                // transcode start, a busy moment — which is ordinary and must
                // not by itself look like a dead mpv. Only mark the
                // connection dead (and stop the reader thread, via `running`)
                // after several consecutive timeouts with no successful reply
                // in between: that is what distinguishes a genuinely wedged
                // peer from one request that happened to run long.
                let failures = self
                    .consecutive_response_timeouts
                    .fetch_add(1, Ordering::SeqCst)
                    + 1;
                if failures >= MAX_CONSECUTIVE_RESPONSE_TIMEOUTS {
                    self.running.store(false, Ordering::SeqCst);
                    return Err(format!(
                        "mpv command timeout ({failures} consecutive — connection declared dead)"
                    ));
                }
                return Err(format!("mpv command timeout ({failures} consecutive)"));
            }
        };

        if response.error.is_empty() || response.error == "success" {
            Ok(response.data)
        } else {
            Err(format!("mpv error: {}", response.error))
        }
    }

    /// Get a property value from mpv.
    pub fn get_property(&self, name: &str) -> Result<Value, String> {
        self.command(vec![
            Value::String("get_property".to_string()),
            Value::String(name.to_string()),
        ])
    }

    /// Get a property value as a specific type.
    pub fn get_property_typed<T: for<'de> Deserialize<'de>>(
        &self,
        name: &str,
    ) -> Result<T, String> {
        let value = self.get_property(name)?;
        serde_json::from_value(value).map_err(|e| format!("Type conversion error: {}", e))
    }

    /// Set a property value.
    pub fn set_property(&self, name: &str, value: Value) -> Result<(), String> {
        self.command(vec![
            Value::String("set_property".to_string()),
            Value::String(name.to_string()),
            value,
        ])?;
        Ok(())
    }

    /// Observe a property for changes.
    pub fn observe_property(&self, id: i64, name: &str) -> Result<(), String> {
        self.command(vec![
            Value::String("observe_property".to_string()),
            Value::Number(id.into()),
            Value::String(name.to_string()),
        ])?;
        Ok(())
    }

    /// Try to receive the next event (non-blocking).
    pub fn try_recv_event(&self) -> Option<MpvEvent> {
        self.event_rx.try_recv().ok()
    }

    /// Check if the connection is still active.
    pub fn is_connected(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Drop for MpvIpc {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.reader_handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::AtomicU64 as TestSocketCounter;
    use std::time::Instant;

    static SOCKET_COUNTER: TestSocketCounter = TestSocketCounter::new(0);

    fn unique_test_socket_path() -> PathBuf {
        let id = SOCKET_COUNTER.fetch_add(1, Ordering::SeqCst);
        PathBuf::from(format!(
            "/tmp/bragi-mpv-ipc-test-{}-{}.sock",
            std::process::id(),
            id
        ))
    }

    /// Root-cause regression for "press play, nothing happens" after long
    /// uptime: a peer that accepts every command (so the write side never
    /// errors) but never replies must not hang `command()` forever, and a
    /// SUSTAINED run of non-responses must mark the connection dead so
    /// `ensure_mpv_running` respawns mpv on the next command rather than
    /// silently re-paying the same timeout forever.
    #[test]
    fn sustained_non_response_marks_connection_dead() {
        let socket_path = unique_test_socket_path();
        let listener = UnixListener::bind(&socket_path).expect("bind fake socket");

        // Fake mpv: reads and discards every line, replies to none. Held
        // alive for the whole test via the thread handle.
        let _wedged = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
            }
        });
        thread::sleep(Duration::from_millis(20));

        let ipc = MpvIpc::connect_with_timeouts(
            &socket_path,
            Duration::from_millis(50),
            Duration::from_secs(2),     // write succeeds; the peer is reading
            Duration::from_millis(100), // response never arrives
        )
        .expect("connect to wedged fake mpv");

        assert!(ipc.is_connected(), "freshly connected socket starts alive");

        for n in 1..MAX_CONSECUTIVE_RESPONSE_TIMEOUTS {
            let start = Instant::now();
            assert!(
                ipc.get_property("pause").is_err(),
                "a command against a non-responding peer must fail, not hang"
            );
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "must fail close to response_timeout (100ms), not the old fixed 5s"
            );
            assert!(
                ipc.is_connected(),
                "timeout {n} of {MAX_CONSECUTIVE_RESPONSE_TIMEOUTS}: a single slow \
                 reply (or a short run of them) must not itself look like a dead \
                 mpv — only sustained non-response does"
            );
        }

        assert!(
            ipc.get_property("pause").is_err(),
            "the final timeout in the sustained run must still fail"
        );
        assert!(
            !ipc.is_connected(),
            "after {MAX_CONSECUTIVE_RESPONSE_TIMEOUTS} consecutive timeouts with no \
             reply in between, the connection must be declared dead so the engine \
             respawns mpv on the next command"
        );

        let _ = std::fs::remove_file(&socket_path);
    }

    /// The other half of the same fix: a single abnormally slow reply (a big
    /// seek, a cold transcode start, a busy moment) must not itself kill the
    /// connection — the next command, sent to the same still-alive peer, must
    /// succeed normally once it starts replying again.
    #[test]
    fn a_single_slow_reply_does_not_kill_the_connection() {
        let socket_path = unique_test_socket_path();
        let listener = UnixListener::bind(&socket_path).expect("bind fake socket");

        let _fake = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut writer = stream.try_clone().expect("clone for writer");
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            let mut first = true;
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                if first {
                    // Simulate one abnormally slow command: never answer it
                    // (the client's short response_timeout below gives up
                    // long before this thread would ever reply anyway).
                    first = false;
                    continue;
                }
                let Ok(cmd) = serde_json::from_str::<Value>(line.trim()) else {
                    continue;
                };
                let request_id = cmd.get("request_id").cloned().unwrap_or(Value::Null);
                let reply = serde_json::to_string(&serde_json::json!({
                    "request_id": request_id,
                    "error": "success",
                    "data": Value::Null,
                }))
                .expect("serialise reply");
                let _ = writeln!(writer, "{reply}");
                let _ = writer.flush();
            }
        });
        thread::sleep(Duration::from_millis(20));

        // response_timeout is generous (not tight, as the mpv-wedge tests
        // above use): this test's fake server thread must win a genuine
        // scheduling race to answer the second command promptly, and a busy
        // host can starve it well past 100ms without anything actually being
        // wrong — the property under test is "one timeout doesn't doom the
        // connection", not "replies are fast".
        let ipc = MpvIpc::connect_with_timeouts(
            &socket_path,
            Duration::from_millis(50),
            Duration::from_secs(2),
            Duration::from_millis(500),
        )
        .expect("connect to fake mpv");

        assert!(
            ipc.get_property("pause").is_err(),
            "the first (slow/unanswered) command must fail"
        );
        assert!(
            ipc.is_connected(),
            "one slow reply must leave the connection alive"
        );

        assert!(
            ipc.get_property("pause").is_ok(),
            "the next command, against the same still-alive peer, must succeed \
             — a single slow reply must not have doomed every command after it"
        );
        assert!(ipc.is_connected());

        let _ = std::fs::remove_file(&socket_path);
    }

    /// `connect_with_timeouts` must actually apply a write timeout (rather
    /// than silently ignoring it) — the smoke-test half of the root-cause fix:
    /// a socket with no write timeout at all is what let a wedged-but-open
    /// mpv freeze the engine loop forever on `writeln!`/`flush()`.
    #[test]
    fn connect_applies_a_write_timeout() {
        let socket_path = unique_test_socket_path();
        let listener = UnixListener::bind(&socket_path).expect("bind fake socket");
        let _accept = thread::spawn(move || {
            // Bind the accepted stream so it stays open for the sleep below —
            // `let _ = listener.accept()` would drop it immediately, closing
            // the connection right away instead of holding it open.
            let (_stream, _) = listener.accept().expect("accept");
            thread::sleep(Duration::from_secs(2));
        });
        thread::sleep(Duration::from_millis(20));

        let ipc = MpvIpc::connect_with_timeouts(
            &socket_path,
            Duration::from_millis(50),
            Duration::from_millis(50),
            Duration::from_millis(50),
        )
        .expect("connect");

        // The write timeout is set at the OS socket level; we can't assert its
        // exact value without an extra syscall, but a bounded connect proves
        // `set_write_timeout` did not error, and the response-timeout test
        // above proves the end-to-end "never hangs" behaviour this enables.
        assert!(ipc.is_connected());

        let _ = std::fs::remove_file(&socket_path);
    }
}
