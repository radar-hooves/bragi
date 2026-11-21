//! Settings database operations.
//!
//! Key-value store for user preferences and application settings.

use std::collections::HashMap;

use rusqlite::{params, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};

use super::{Database, DatabaseError};

impl Database {
    /// Get a setting value as a string.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, DatabaseError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
            let value = stmt.query_row([key], |row| row.get(0)).optional()?;
            Ok(value)
        })
    }

    /// Get a setting parsed as `T`; unset or unparseable reads as `None`.
    pub fn get_parsed<T: std::str::FromStr>(&self, key: &str) -> Result<Option<T>, DatabaseError> {
        Ok(self.get_setting(key)?.and_then(|v| v.parse().ok()))
    }

    /// Read every setting in one query.
    pub fn get_all_settings(&self) -> Result<HashMap<String, String>, DatabaseError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<Result<_, _>>()
        })
    }

    /// Get a setting value deserialised from JSON.
    pub fn get_setting_json<T: DeserializeOwned>(
        &self,
        key: &str,
    ) -> Result<Option<T>, DatabaseError> {
        let value = self.get_setting(key)?;
        match value {
            Some(json) => {
                let parsed = serde_json::from_str(&json)
                    .map_err(|e| DatabaseError::Serialisation(e.to_string()))?;
                Ok(Some(parsed))
            }
            None => Ok(None),
        }
    }

    /// Set a setting value as a string.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), DatabaseError> {
        self.with_conn(|conn| {
            conn.execute(
                r#"
                INSERT INTO settings (key, value) VALUES (?1, ?2)
                ON CONFLICT(key) DO UPDATE SET value = excluded.value
                "#,
                params![key, value],
            )?;
            Ok(())
        })
    }

    /// Set a setting value serialised as JSON.
    pub fn set_setting_json<T: Serialize>(
        &self,
        key: &str,
        value: &T,
    ) -> Result<(), DatabaseError> {
        let json = serde_json::to_string(value)
            .map_err(|e| DatabaseError::Serialisation(e.to_string()))?;
        self.set_setting(key, &json)
    }

    /// Delete a setting.
    pub fn delete_setting(&self, key: &str) -> Result<bool, DatabaseError> {
        self.with_conn(|conn| {
            let rows = conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
            Ok(rows > 0)
        })
    }

    /// Set multiple settings in a transaction.
    pub fn set_settings(&self, settings: &[(&str, &str)]) -> Result<(), DatabaseError> {
        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            {
                let mut stmt = tx.prepare(
                    r#"
                    INSERT INTO settings (key, value) VALUES (?1, ?2)
                    ON CONFLICT(key) DO UPDATE SET value = excluded.value
                    "#,
                )?;

                for (key, value) in settings {
                    stmt.execute([*key, *value])?;
                }
            }

            tx.commit()?;
            Ok(())
        })
    }
}

/// Well-known setting keys.
pub mod keys {
    use crate::mediaserver::ServerKind;

    /// Legacy/global last library sync timestamp (Unix seconds).
    ///
    /// Superseded by the per-backend [`last_library_sync_key`] keys, but
    /// kept as a read-time fallback so existing installs do not trigger a
    /// redundant re-sync on the first tick after upgrade. Still written by the
    /// scrobble-sync path (`commands/scrobble.rs`) for back-compat.
    pub const LAST_LIBRARY_SYNC: &str = "last_library_sync";

    /// Per-backend last library sync timestamp key.
    ///
    /// Each `ServerKind` gets its own timestamp row (`last_library_sync_jellyfin`
    /// / `last_library_sync_opensubsonic`) so both backends refresh on their own
    /// interval instead of sharing a single global timestamp.
    pub fn last_library_sync_key(kind: ServerKind) -> String {
        format!("last_library_sync_{}", kind.as_str())
    }

    /// Library sync interval in seconds (0 = manual only).
    pub const LIBRARY_SYNC_INTERVAL: &str = "library_sync_interval";

    /// Volume level (0.0 to 1.0).
    pub const VOLUME: &str = "volume";

    /// Shuffle enabled.
    pub const SHUFFLE: &str = "shuffle";

    /// Repeat mode (off, all, one).
    pub const REPEAT_MODE: &str = "repeat_mode";

    /// ReplayGain mode (track, album).
    pub const REPLAYGAIN_MODE: &str = "replaygain_mode";

    /// ReplayGain pre-amp (dB).
    pub const REPLAYGAIN_PREAMP: &str = "replaygain_preamp";

    /// Maximum cache size for downloaded audio (bytes).
    pub const CACHE_SIZE_LIMIT: &str = "cache_size_limit";

    /// Device ID for this installation.
    pub const DEVICE_ID: &str = "device_id";

    /// Jellyfin server URL.
    pub const JELLYFIN_SERVER_URL: &str = "jellyfin_server_url";

    /// Active media-server backend (`jellyfin` | `opensubsonic`).
    pub const SERVER_TYPE: &str = "server_type";

    /// Offline mode enabled.
    pub const OFFLINE_MODE: &str = "offline_mode";

    /// Crossfade duration in milliseconds.
    pub const CROSSFADE_DURATION: &str = "crossfade_duration";

    // UI layout state
    /// Whether the sidebar is collapsed.
    pub const SIDEBAR_COLLAPSED: &str = "sidebar_collapsed";

    /// View mode (grid or list).
    pub const VIEW_MODE: &str = "view_mode";

    /// Artist view mode (grid or list).
    pub const ARTIST_VIEW_MODE: &str = "artist_view_mode";

    /// Genre view mode (grid or list).
    pub const GENRE_VIEW_MODE: &str = "genre_view_mode";

    /// Whether the column browser is visible.
    pub const COLUMN_BROWSER_VISIBLE: &str = "column_browser_visible";

    /// Column browser height in pixels.
    pub const COLUMN_BROWSER_HEIGHT: &str = "column_browser_height";

    /// Whether the queue sidebar is visible.
    pub const QUEUE_SIDEBAR_VISIBLE: &str = "queue_sidebar_visible";

    /// Per-view track list column state (JSON object: view ID -> { columns,
    /// widths }) — visibility, order and width are one unit per view (Songs,
    /// Recently Played, Search never share one column layout).
    pub const TRACK_LIST_VIEW_COLUMNS: &str = "track_list_view_columns";

    /// Per-playlist track list column state (JSON object: Jellyfin playlist ID
    /// -> { columns, widths }), the same `TrackListViewColumns` shape as
    /// above — Apple Music remembers a playlist's column layout per playlist,
    /// confirmed live: "the options you select will
    /// apply to that playlist only, and have to be re-selected for each
    /// playlist." A playlist missing from this map falls back to the
    /// frontend's own default (title, artist, album, time).
    pub const PLAYLIST_TRACK_COLUMNS: &str = "playlist_track_columns";

    // Sort preferences
    /// Sort order for albums view.
    pub const ALBUMS_SORT: &str = "albums_sort";

    /// Genres view sort key.
    pub const GENRES_SORT: &str = "genres_sort";

    /// Sort order for artists view.
    pub const ARTISTS_SORT: &str = "artists_sort";

    /// Sort order for albums within artist detail view.
    pub const ARTIST_ALBUMS_SORT: &str = "artist_albums_sort";
    pub const RECENTLY_ADDED_SORT: &str = "recently_added_sort";

    /// Sort order for tracks view.
    pub const TRACKS_SORT: &str = "tracks_sort";

    /// Per-view grid tile size in pixels (120-280; JSON object: view ID ->
    /// size) — albums, artists, genres and recentlyAdded each remember their
    /// own, the way Apple Music's View Options does per view rather than
    /// sharing one global setting. Replaces the old flat
    /// `grid_tile_size` key; an install upgrading from it just gets the
    /// 200px default once, since the two are different shapes.
    pub const GRID_TILE_SIZES: &str = "grid_tile_sizes";

    /// Visible columns in the column browser (JSON array: genres, artists, albums).
    pub const COLUMN_BROWSER_COLUMNS: &str = "column_browser_columns";

    /// Whether the status bar is visible.
    pub const STATUS_BAR_VISIBLE: &str = "status_bar_visible";

    /// Theme preference (dark, light, or system).
    pub const THEME: &str = "theme";

    /// Accent colour preference.
    pub const ACCENT_COLOUR: &str = "accent_colour";

    /// Gapless playback enabled.
    pub const GAPLESS_PLAYBACK: &str = "gapless_playback";

    /// Scrobbling enabled.
    pub const SCROBBLE_ENABLED: &str = "scrobble_enabled";

    /// Scrobble threshold as percentage (25-90).
    pub const SCROBBLE_THRESHOLD: &str = "scrobble_threshold";

    /// UI zoom level as percentage (80-130).
    pub const UI_ZOOM_LEVEL: &str = "ui_zoom_level";

    /// Number of days for "recently added" filter (7, 14, 30, 60, 90).
    pub const RECENTLY_ADDED_DAYS: &str = "recently_added_days";

    /// Recently played tracks (JSON array).
    pub const RECENTLY_PLAYED_JSON: &str = "recently_played_json";

    /// Song change notifications enabled.
    pub const SONG_NOTIFICATIONS_ENABLED: &str = "song_notifications_enabled";

    // Equalizer settings
    /// Whether the equalizer is enabled.
    pub const EQ_ENABLED: &str = "eq_enabled";

    /// Current EQ preset name.
    pub const EQ_PRESET: &str = "eq_preset";

    /// Custom EQ bands (JSON array of 18 integers, -20 to +20 dB).
    pub const EQ_CUSTOM_BANDS: &str = "eq_custom_bands";

    /// User-saved named EQ profiles (JSON array of `{name, bands18}`).
    pub const EQ_USER_PROFILES: &str = "eq_user_profiles";

    // Fade settings
    /// Whether the pause/stop/resume volume fade is enabled.
    pub const FADE_ENABLED: &str = "fade_enabled";

    /// Fade ramp duration in milliseconds.
    pub const FADE_DURATION_MS: &str = "fade_duration_ms";

    /// Manual lyrics timing offset in milliseconds. Signed: positive shows
    /// synced lyrics earlier (ahead of the audio), negative delays them. A single
    /// global value, local-only — never written back to Jellyfin.
    pub const LYRICS_OFFSET_MS: &str = "lyrics_offset_ms";

    /// Window decorations / title bar visible (Linux tiling-WM setting).
    pub const TITLE_BAR_VISIBLE: &str = "title_bar_visible";

    // Startup behaviour
    /// Launch Bragi automatically on system login. Mirrors OS truth
    /// (reconciled from the autostart plugin at startup).
    pub const STARTUP_LAUNCH_ON_LOGIN: &str = "startup_launch_on_login";

    /// Start Bragi hidden to the tray instead of showing the window.
    pub const START_MINIMISED: &str = "start_minimised";

    // System tray behaviour
    /// Hide the window to the tray on close instead of quitting.
    pub const CLOSE_TO_TRAY: &str = "close_to_tray";

    /// Hide the window to the tray when it is minimised.
    pub const MINIMISE_TO_TRAY: &str = "minimise_to_tray";

    // MiniPlayer
    /// Whether the MiniPlayer window floats above other windows.
    pub const MINI_PLAYER_ALWAYS_ON_TOP: &str = "mini_player_always_on_top";

    // Download / transcoding settings
    /// Preferred download format (original, flac, mp3, aac, opus).
    pub const DOWNLOAD_FORMAT: &str = "download_format";

    /// Preferred download bitrate in kbps for lossy formats (128/192/256/320).
    pub const DOWNLOAD_BITRATE: &str = "download_bitrate";

    /// Where play counts are sourced (auto | jellyfin | earworm). `auto` prefers
    /// Jellyfin and falls back to the scrobble server. Used by AI-playlist "most played"
    /// sorting.
    pub const PLAY_COUNT_SOURCE: &str = "play_count_source";

    // AI Playlists. API keys are NOT here — they live in the keyring.
    /// Whether the AI-playlist feature is enabled.
    pub const AI_PLAYLIST_ENABLED: &str = "ai_playlist_enabled";
    /// Selected LLM provider (ollama, openai, anthropic).
    pub const LLM_PROVIDER: &str = "llm_provider";
    /// Selected model (empty = provider default).
    pub const LLM_MODEL: &str = "llm_model";
    /// User-set endpoint base URL (empty = provider default).
    pub const LLM_BASE_URL: &str = "llm_base_url";
    /// Max tokens per LLM completion.
    pub const LLM_MAX_TOKENS: &str = "llm_max_tokens";
    /// Optional custom system prompt for playlist curation (empty = built-in).
    pub const LLM_SYSTEM_PROMPT: &str = "llm_system_prompt";

    // Telemetry. A Dock-launched app inherits no fleet environment, so these
    // are the only door on a machine the fleet does not configure. Where the
    // environment sets the exporter it wins and these are not applied.
    /// The collector's OTLP/HTTP base address. Empty means local only.
    pub const TELEMETRY_ENDPOINT: &str = "telemetry.endpoint";

    /// A command printing a JSON object of headers, run whenever the exporter is
    /// installed or probed. The command string is all that is stored — never a
    /// header value. Empty means no authorisation headers.
    pub const TELEMETRY_HEADERS_HELPER: &str = "telemetry.headers_helper";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_set_setting() {
        let db = Database::open_in_memory().unwrap();

        // Initially no setting
        assert!(db.get_setting("test_key").unwrap().is_none());

        // Set and retrieve
        db.set_setting("test_key", "test_value").unwrap();
        assert_eq!(
            db.get_setting("test_key").unwrap(),
            Some("test_value".to_string())
        );

        // Update
        db.set_setting("test_key", "new_value").unwrap();
        assert_eq!(
            db.get_setting("test_key").unwrap(),
            Some("new_value".to_string())
        );
    }

    #[test]
    fn test_json_settings() {
        let db = Database::open_in_memory().unwrap();

        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        struct TestConfig {
            enabled: bool,
            count: u32,
        }

        let config = TestConfig {
            enabled: true,
            count: 42,
        };

        db.set_setting_json("config", &config).unwrap();

        let retrieved: TestConfig = db.get_setting_json("config").unwrap().unwrap();
        assert_eq!(retrieved, config);
    }

    #[test]
    fn test_delete_setting() {
        let db = Database::open_in_memory().unwrap();

        db.set_setting("to_delete", "value").unwrap();
        assert!(db.get_setting("to_delete").unwrap().is_some());

        let deleted = db.delete_setting("to_delete").unwrap();
        assert!(deleted);
        assert!(db.get_setting("to_delete").unwrap().is_none());

        // Deleting non-existent returns false
        let deleted = db.delete_setting("to_delete").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn test_startup_and_tray_prefs_round_trip() {
        let db = Database::open_in_memory().unwrap();

        // Unset keys default to None (callers fall back to their defaults).
        assert!(db
            .get_setting(keys::STARTUP_LAUNCH_ON_LOGIN)
            .unwrap()
            .is_none());
        assert!(db.get_setting(keys::START_MINIMISED).unwrap().is_none());
        assert!(db.get_setting(keys::CLOSE_TO_TRAY).unwrap().is_none());
        assert!(db.get_setting(keys::MINIMISE_TO_TRAY).unwrap().is_none());
        assert!(db
            .get_setting(keys::MINI_PLAYER_ALWAYS_ON_TOP)
            .unwrap()
            .is_none());

        // Persist a representative mix of true/false and read them back as bools.
        db.set_settings(&[
            (keys::STARTUP_LAUNCH_ON_LOGIN, "true"),
            (keys::START_MINIMISED, "true"),
            (keys::CLOSE_TO_TRAY, "false"),
            (keys::MINIMISE_TO_TRAY, "true"),
            (keys::MINI_PLAYER_ALWAYS_ON_TOP, "false"),
        ])
        .unwrap();

        let parse = |k: &str| {
            db.get_setting(k)
                .unwrap()
                .and_then(|v| v.parse::<bool>().ok())
        };
        assert_eq!(parse(keys::STARTUP_LAUNCH_ON_LOGIN), Some(true));
        assert_eq!(parse(keys::START_MINIMISED), Some(true));
        assert_eq!(parse(keys::CLOSE_TO_TRAY), Some(false));
        assert_eq!(parse(keys::MINIMISE_TO_TRAY), Some(true));
        assert_eq!(parse(keys::MINI_PLAYER_ALWAYS_ON_TOP), Some(false));
    }

    #[test]
    fn test_download_format_bitrate_round_trip() {
        let db = Database::open_in_memory().unwrap();

        // Unset by default (caller applies defaults).
        assert!(db.get_setting(keys::DOWNLOAD_FORMAT).unwrap().is_none());
        assert!(db.get_setting(keys::DOWNLOAD_BITRATE).unwrap().is_none());

        db.set_settings(&[
            (keys::DOWNLOAD_FORMAT, "mp3"),
            (keys::DOWNLOAD_BITRATE, "320"),
        ])
        .unwrap();

        assert_eq!(
            db.get_setting(keys::DOWNLOAD_FORMAT).unwrap(),
            Some("mp3".to_string())
        );
        assert_eq!(
            db.get_setting(keys::DOWNLOAD_BITRATE).unwrap(),
            Some("320".to_string())
        );
    }

    #[test]
    fn test_lyrics_offset_round_trips_signed() {
        let db = Database::open_in_memory().unwrap();

        // Unset by default (the settings command applies the 0 default).
        assert!(db.get_setting(keys::LYRICS_OFFSET_MS).unwrap().is_none());

        // A NEGATIVE offset must round-trip: lyrics_offset_ms is signed (i32) so
        // the user can pull lyrics both earlier and later. Storing it as a
        // u32 (like FADE_DURATION_MS) would silently forbid negatives.
        db.set_setting(keys::LYRICS_OFFSET_MS, "-750").unwrap();
        let stored = db.get_setting(keys::LYRICS_OFFSET_MS).unwrap();
        assert_eq!(stored, Some("-750".to_string()));
        assert_eq!(stored.and_then(|v| v.parse::<i32>().ok()), Some(-750));

        // And a positive one.
        db.set_setting(keys::LYRICS_OFFSET_MS, "500").unwrap();
        assert_eq!(
            db.get_setting(keys::LYRICS_OFFSET_MS)
                .unwrap()
                .and_then(|v| v.parse::<i32>().ok()),
            Some(500)
        );
    }

    #[test]
    fn test_last_library_sync_key_is_per_backend_and_round_trips() {
        use crate::mediaserver::ServerKind;

        // Distinct, stable key strings per backend.
        assert_eq!(
            keys::last_library_sync_key(ServerKind::Jellyfin),
            "last_library_sync_jellyfin"
        );
        assert_eq!(
            keys::last_library_sync_key(ServerKind::OpenSubsonic),
            "last_library_sync_opensubsonic"
        );

        let db = Database::open_in_memory().unwrap();
        for kind in [ServerKind::Jellyfin, ServerKind::OpenSubsonic] {
            let key = keys::last_library_sync_key(kind);
            db.set_setting(&key, "1234").unwrap();
            assert_eq!(db.get_setting(&key).unwrap(), Some("1234".to_string()));
        }
        // The two backends do not clobber each other.
        db.set_setting(&keys::last_library_sync_key(ServerKind::Jellyfin), "1")
            .unwrap();
        db.set_setting(&keys::last_library_sync_key(ServerKind::OpenSubsonic), "2")
            .unwrap();
        assert_eq!(
            db.get_setting(&keys::last_library_sync_key(ServerKind::Jellyfin))
                .unwrap(),
            Some("1".to_string())
        );
        assert_eq!(
            db.get_setting(&keys::last_library_sync_key(ServerKind::OpenSubsonic))
                .unwrap(),
            Some("2".to_string())
        );
    }

    #[test]
    fn test_telemetry_keys_round_trip_and_default_to_unset() {
        let db = Database::open_in_memory().unwrap();

        // Unset by default: a machine the fleet does not configure exports
        // nothing until someone fills the Settings pane in.
        assert!(db.get_setting(keys::TELEMETRY_ENDPOINT).unwrap().is_none());
        assert!(db
            .get_setting(keys::TELEMETRY_HEADERS_HELPER)
            .unwrap()
            .is_none());

        db.set_settings(&[
            (keys::TELEMETRY_ENDPOINT, "https://otlp.example"),
            (keys::TELEMETRY_HEADERS_HELPER, "headers-helper"),
        ])
        .unwrap();

        assert_eq!(
            db.get_setting(keys::TELEMETRY_ENDPOINT).unwrap(),
            Some("https://otlp.example".to_string())
        );
        assert_eq!(
            db.get_setting(keys::TELEMETRY_HEADERS_HELPER).unwrap(),
            Some("headers-helper".to_string())
        );

        // Clearing the endpoint is how a user turns export off, so an empty
        // string must survive as an empty string rather than becoming unset.
        db.set_settings(&[
            (keys::TELEMETRY_ENDPOINT, ""),
            (keys::TELEMETRY_HEADERS_HELPER, ""),
        ])
        .unwrap();
        assert_eq!(
            db.get_setting(keys::TELEMETRY_ENDPOINT).unwrap(),
            Some(String::new())
        );
    }

    #[test]
    fn test_get_all_settings_and_get_parsed() {
        let db = Database::open_in_memory().unwrap();
        db.set_settings(&[("a", "7"), ("b", "x")]).unwrap();
        let all = db.get_all_settings().unwrap();
        assert_eq!(all.get("a").map(String::as_str), Some("7"));
        assert_eq!(db.get_parsed::<u32>("a").unwrap(), Some(7));
        assert_eq!(db.get_parsed::<u32>("b").unwrap(), None);
        assert_eq!(db.get_parsed::<u32>("c").unwrap(), None);
    }
}
