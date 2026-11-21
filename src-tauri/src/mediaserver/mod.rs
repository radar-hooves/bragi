//! Media-server abstraction.
//!
//! Bragi speaks to either Jellyfin or an OpenSubsonic-compatible server. This
//! module holds the backend tag (`kind::ServerKind`) and the OpenSubsonic
//! client (`subsonic`). A `MediaClient` enum-dispatch wrapper will
//! unify Jellyfin + OpenSubsonic behind the capability surface the app calls,
//! mirroring the enum-dispatch style of `llm/`.
//!
// The MediaClient enum exposes the full backend capability surface for parity;
// a few wrappers (http/fire_auth_expired/get_album) aren't called yet but are
// kept so the abstraction is complete and Phase 2 can wire them without churn.

pub mod client;
pub mod kind;
pub mod subsonic;

pub use client::MediaClient;
pub use kind::ServerKind;

/// Is `u` a cleartext (`http://`) connection to a non-loopback host?
///
/// Explicit `http://` is still *allowed* for LAN parity (Jellyfin and Subsonic
/// both accept it), but a non-loopback cleartext connection means credentials —
/// for Subsonic, a non-expiring salted token — and all audio/metadata traverse
/// the network unencrypted, so it warrants an audit event and a UI warning.
///
/// Loopback (`http://127.0.0.1`, `http://localhost`, `[::1]`) is exempt: there
/// is no network hop to sniff, and the local stream proxy itself is plain
/// `http` on loopback by design.
pub fn is_cleartext_url(u: &url::Url) -> bool {
    if u.scheme() != "http" {
        return false;
    }
    match u.host() {
        Some(url::Host::Domain(h)) => !h.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => !ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => !ip.is_loopback(),
        None => false,
    }
}

#[cfg(test)]
mod cleartext_tests {
    use super::is_cleartext_url;
    use url::Url;

    fn cleartext(s: &str) -> bool {
        is_cleartext_url(&Url::parse(s).unwrap())
    }

    #[test]
    fn https_is_never_cleartext() {
        assert!(!cleartext("https://music.example.com"));
        assert!(!cleartext("https://192.168.1.10:4533"));
    }

    #[test]
    fn http_to_named_host_is_cleartext() {
        assert!(cleartext("http://music.example.com"));
        assert!(cleartext("http://192.168.1.10:4533"));
    }

    #[test]
    fn http_to_loopback_is_exempt() {
        assert!(!cleartext("http://127.0.0.1:4533"));
        assert!(!cleartext("http://localhost:8096"));
        assert!(!cleartext("http://[::1]:8096"));
    }
}
