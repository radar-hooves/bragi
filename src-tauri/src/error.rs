//! Application error types for Bragi.
//!
//! Commands return [`CommandError`], which serialises as its message.

use crate::db::DatabaseError;
use thiserror::Error;

/// Application-level errors.
#[derive(Error, Debug)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Jellyfin API error: {0}")]
    Jellyfin(String),

    /// A protocol-level error from a media server (e.g. an OpenSubsonic
    /// `subsonic-response` with `status="failed"`). Backend-agnostic.
    #[error("Media server error: {0}")]
    MediaServer(String),

    #[error("Playback error: {0}")]
    Playback(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Authentication error: {0}")]
    Auth(String),

    /// A 401 on an *already-authenticated* session call (library/sync/stream).
    ///
    /// Distinct from [`AppError::Auth`], which stays for login-time credential
    /// rejection (`test_connection` / `authenticate_by_name` during connect and
    /// must surface inline on the Connections page). `AuthExpired` is the only
    /// variant that drives the deduplicated global "reconnect" event, so
    /// a wrong password on the connect screen never triggers the global prompt.
    #[error("Jellyfin session expired: {0}")]
    AuthExpired(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}

impl From<AppError> for String {
    fn from(err: AppError) -> String {
        err.to_string()
    }
}

impl From<DatabaseError> for AppError {
    fn from(err: DatabaseError) -> Self {
        AppError::Database(err.to_string())
    }
}

/// The error a Tauri command returns. It crosses IPC as its `Display` text,
/// never as a structure, so no internal detail beyond that message reaches the
/// frontend.
#[derive(Error, Debug)]
pub enum CommandError {
    #[error(transparent)]
    App(#[from] AppError),

    #[error(transparent)]
    Database(#[from] DatabaseError),

    /// A message written for the user at the point of failure.
    #[error("{0}")]
    Message(String),
}

impl CommandError {
    /// Wrap any displayable error from outside the crate's own error types.
    pub fn msg(error: impl std::fmt::Display) -> Self {
        Self::Message(error.to_string())
    }
}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

impl From<&str> for CommandError {
    fn from(message: &str) -> Self {
        Self::Message(message.to_string())
    }
}

impl serde::Serialize for CommandError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Result type alias for Tauri command returns.
pub type CommandResult<T> = Result<T, CommandError>;

/// Wraps a message that was never a real `Error` value to begin with — an mpv
/// or download failure that only ever was a formatted `String`, or a `sync`
/// result already collapsed to one — so [`telemetry::report_error_with_cause`]
/// can carry it with the same `chain` field a real `Error` gets, instead of
/// flattening it to a static label. Holds no more than the message already
/// held; it adds no content of its own.
#[derive(Debug)]
pub struct MessageError(pub String);

impl std::fmt::Display for MessageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MessageError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing::field::{Field, Visit};
    use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
    use tracing_subscriber::registry;

    /// Every event seen on `telemetry::panic`, as the rendered value of each
    /// field — the same technique `audit.rs` and `tauri-plugin-telemetry`'s
    /// own `span` module use to prove what actually reaches an allow-listed
    /// target, applied here to the crate's own force-allowed one.
    #[derive(Default)]
    struct Captured(Arc<Mutex<Vec<String>>>);

    struct Recorder(String);

    impl Visit for Recorder {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.0.push_str(&format!(" {}={:?}", field.name(), value));
        }
    }

    impl<S: tracing::Subscriber> Layer<S> for Captured {
        fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
            if event.metadata().target() != "telemetry::panic" {
                return;
            }
            let mut recorder = Recorder(String::new());
            event.record(&mut recorder);
            self.0.lock().unwrap().push(recorder.0);
        }
    }

    /// A caller with no real `Error` (an mpv/download failure that was always
    /// just a `String`) still gets its full message and cause chain into the
    /// exported record via `MessageError`, exactly as a real `Error` would.
    #[test]
    fn a_wrapped_message_reaches_report_error_with_cause_as_a_full_chain() {
        let captured = Captured::default();
        let events = captured.0.clone();
        let dispatch = tracing::Dispatch::new(registry().with(captured));
        let _guard = tracing::dispatcher::set_default(&dispatch);

        telemetry::report_error_with_cause(
            "download failed",
            &MessageError("disk full while writing cache file".to_string()),
        );

        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1, "{events:?}");
        assert!(events[0].contains("download failed"), "{}", events[0]);
        assert!(
            events[0].contains("disk full while writing cache file"),
            "{}",
            events[0]
        );
    }
}
