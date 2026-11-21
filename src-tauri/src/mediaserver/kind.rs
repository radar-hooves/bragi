//! Media-server backend identity.
//!
//! Bragi speaks to either Jellyfin or an OpenSubsonic-compatible server
//! (Navidrome, Gonic, etc.). The two are dispatched by a `MediaClient` enum
//! (see `client.rs`), mirroring the enum-dispatch style of `llm/`. This module
//! is just the backend tag and its settings string.

use serde::{Deserialize, Serialize};

/// A supported media-server backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerKind {
    /// Jellyfin (the original backend).
    #[default]
    Jellyfin,
    /// An OpenSubsonic-compatible server (Navidrome, Gonic, Airsonic, …).
    OpenSubsonic,
}

impl ServerKind {
    /// Parse from the lowercase settings string; `None` if unknown.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "jellyfin" => Some(Self::Jellyfin),
            // Accept the shorter "subsonic" alias for convenience.
            "opensubsonic" | "subsonic" => Some(Self::OpenSubsonic),
            _ => None,
        }
    }

    /// The canonical lowercase settings string for this backend.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jellyfin => "jellyfin",
            Self::OpenSubsonic => "opensubsonic",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_round_trips() {
        for k in [ServerKind::Jellyfin, ServerKind::OpenSubsonic] {
            assert_eq!(ServerKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(
            ServerKind::parse("subsonic"),
            Some(ServerKind::OpenSubsonic)
        );
        assert_eq!(ServerKind::parse("nope"), None);
    }

    #[test]
    fn default_is_jellyfin() {
        assert_eq!(ServerKind::default(), ServerKind::Jellyfin);
    }
}
