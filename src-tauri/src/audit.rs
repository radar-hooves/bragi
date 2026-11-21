//! The security events bragi may export, and the one target that carries them.
//!
//! `telemetry::init` is handed [`TARGET`] as its whole allow-list, so these
//! four events are the only thing bragi sends off the device. Everything else
//! the app logs — track titles, artists, album names, search terms, playlist
//! names, file paths, full URLs — carries its module's own target and stays on
//! stderr for a developer at a terminal.
//!
//! The boundary is structural, not a redaction pass: `detail` is `&'static str`
//! so an error string can never be passed as one, and an endpoint is trimmed to
//! a bare origin by [`origin`] because a Subsonic URL carries the salted auth
//! token in its query string.

use url::Url;

use crate::error::AppError;

/// The one `tracing` target whose events may leave this device.
///
/// Handed to `telemetry::init` in `lib.rs` as the entire allow-list.
pub const TARGET: &str = "bragi::audit";

/// Trim an endpoint to `scheme://host[:port]`.
///
/// A media-server URL reaches these events straight from the user or from a
/// client's base URL, and a Subsonic URL carries `?u=&t=&s=` — the salted auth
/// token — in its query. Anything that is not a URL with a host (a local state
/// change has no endpoint at all) records as `none`.
fn origin(endpoint: &str) -> String {
    let Ok(url) = Url::parse(endpoint) else {
        return "none".to_string();
    };
    match (url.host_str(), url.port()) {
        (Some(host), Some(port)) => format!("{}://{}:{}", url.scheme(), host, port),
        (Some(host), None) => format!("{}://{}", url.scheme(), host),
        (None, _) => "none".to_string(),
    }
}

/// Generate a new correlation ID for request tracing.
pub fn correlation_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Log a security audit event for authentication attempts.
pub fn log_auth_event(
    service: &str,
    action: &str,
    success: bool,
    detail: &'static str,
    correlation_id: &str,
) {
    if success {
        tracing::info!(
            target: TARGET,
            audit_category = "auth",
            service,
            action,
            success,
            detail,
            correlation_id,
            "Security: authentication event"
        );
    } else {
        tracing::warn!(
            target: TARGET,
            audit_category = "auth",
            service,
            action,
            success,
            detail,
            correlation_id,
            "Security: authentication event"
        );
    }
}

/// Log a security audit event for credential operations.
pub fn log_credential_event(service: &str, action: &str, success: bool, correlation_id: &str) {
    if success {
        tracing::info!(
            target: TARGET,
            audit_category = "credential",
            service,
            action,
            success,
            correlation_id,
            "Security: credential event"
        );
    } else {
        tracing::warn!(
            target: TARGET,
            audit_category = "credential",
            service,
            action,
            success,
            correlation_id,
            "Security: credential event"
        );
    }
}

/// Log a security audit event for connection state changes.
///
/// `endpoint` is trimmed to a bare origin; see [`origin`].
pub fn log_connection_event(service: &str, action: &str, endpoint: &str, correlation_id: &str) {
    tracing::info!(
        target: TARGET,
        audit_category = "connection",
        service,
        action,
        endpoint = origin(endpoint),
        correlation_id,
        "Security: connection state change"
    );
}

/// Log a security audit event for input validation failures.
pub fn log_validation_failure(
    component: &str,
    validation_type: &str,
    detail: &'static str,
    correlation_id: &str,
) {
    tracing::warn!(
        target: TARGET,
        audit_category = "validation",
        component,
        validation_type,
        detail,
        correlation_id,
        "Security: input validation failure"
    );
}

/// The error class an audit event may carry, in place of the error itself.
///
/// `AppError`'s `Display` wraps a message built from a server response or a
/// `reqwest::Error`, and reqwest's own `Display` appends the full request URL —
/// query string and all. None of that may leave the device, so a failed audit
/// event records the class and the developer's stderr keeps the rest.
pub fn error_kind(error: &AppError) -> &'static str {
    match error {
        AppError::Database(_) => "database",
        AppError::Jellyfin(_) => "jellyfin",
        AppError::MediaServer(_) => "media_server",
        AppError::Playback(_) => "playback",
        AppError::Config(_) => "config",
        AppError::Network(_) => "network",
        AppError::Auth(_) => "auth",
        AppError::AuthExpired(_) => "auth_expired",
        AppError::NotFound(_) => "not_found",
        AppError::InvalidOperation(_) => "invalid_operation",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing::field::{Field, Visit};
    use tracing::subscriber::with_default;
    use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
    use tracing_subscriber::registry;

    use super::*;

    #[test]
    fn test_correlation_id_format() {
        let id = correlation_id();
        // UUID v4 format: 8-4-4-4-12
        assert_eq!(id.len(), 36);
        assert_eq!(id.chars().filter(|c| *c == '-').count(), 4);
    }

    #[test]
    fn test_correlation_id_uniqueness() {
        let id1 = correlation_id();
        let id2 = correlation_id();
        assert_ne!(id1, id2);
    }

    #[test]
    fn origin_drops_the_path_and_the_query() {
        // A Subsonic URL carries the salted auth token in its query string.
        assert_eq!(
            origin("https://music.example.com/rest/ping.view?u=demo&t=abc&s=xyz"),
            "https://music.example.com"
        );
    }

    #[test]
    fn origin_keeps_a_non_default_port() {
        assert_eq!(
            origin("http://192.168.1.10:8096/web/index.html"),
            "http://192.168.1.10:8096"
        );
    }

    #[test]
    fn origin_keeps_the_brackets_around_an_ipv6_host() {
        assert_eq!(origin("http://[::1]:8096/x"), "http://[::1]:8096");
    }

    #[test]
    fn origin_of_anything_that_is_not_an_endpoint_is_none() {
        assert_eq!(origin(""), "none");
        assert_eq!(origin("jellyfin.example.com"), "none");
    }

    #[test]
    fn error_kind_carries_no_message() {
        let error = AppError::Network(
            "error sending request for url (https://music.example.com/rest/ping?t=secret)"
                .to_string(),
        );
        assert_eq!(error_kind(&error), "network");
    }

    /// Every event seen, as its target plus the rendered value of each field.
    #[derive(Default)]
    struct Captured(Arc<Mutex<Vec<(&'static str, String)>>>);

    struct Recorder(String);

    impl Visit for Recorder {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.0.push_str(&format!(" {}={:?}", field.name(), value));
        }
    }

    impl<S: tracing::Subscriber> Layer<S> for Captured {
        fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
            let mut recorder = Recorder(String::new());
            event.record(&mut recorder);
            self.0
                .lock()
                .unwrap()
                .push((event.metadata().target(), recorder.0));
        }
    }

    #[test]
    fn every_audit_event_carries_the_allow_listed_target_and_no_content() {
        let captured = Captured::default();
        let events = captured.0.clone();
        let cid = correlation_id();

        with_default(registry().with(captured), || {
            log_auth_event("subsonic", "login", false, "network", &cid);
            log_credential_event("jellyfin", "store", true, &cid);
            log_connection_event(
                "subsonic",
                "login_attempt",
                "https://music.example.com/rest/ping.view?u=demo&t=leaked&s=salt",
                &cid,
            );
            log_validation_failure("proxy", "token", "invalid proxy token", &cid);
        });

        let events = events.lock().unwrap();
        assert_eq!(events.len(), 4, "one line per event: {events:?}");
        for (target, fields) in events.iter() {
            assert_eq!(
                *target, TARGET,
                "an event off this target leaves the device unfiltered"
            );
            assert!(
                !fields.contains("leaked"),
                "query string exported: {fields}"
            );
            assert!(!fields.contains("rest/ping"), "path exported: {fields}");
            assert!(fields.contains(&cid), "no correlation_id: {fields}");
        }
    }
}
