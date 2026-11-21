//! Settings-related Tauri commands.
//!
//! Handles application settings and configuration, persisted to SQLite
//! via the `db::settings` module.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::Ordering;

use crate::audio::ReplayGainMode;
use crate::db::settings::keys;
use crate::downloads::DownloadFormat;
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tauri::Manager;

/// One view's track-list column state: which columns show, in what order
/// (order lives in `columns` itself), and any widths the user dragged away
/// from their default. One of these per view (Songs, Recently Played,
/// Search) — Apple Music keeps track-list columns per playlist/view rather
/// than sharing one global layout, and Bragi's single shared
/// layout was leaking between views (Recently Played force-injecting a column
/// the user could never actually hide there).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrackListViewColumns {
    pub columns: Vec<String>,
    pub widths: HashMap<String, f64>,
}

/// Grid-shaped view IDs that each keep their own tile size.
const GRID_VIEW_IDS: [&str; 4] = ["albums", "artists", "genres", "recentlyAdded"];

fn default_grid_tile_sizes() -> HashMap<String, u32> {
    GRID_VIEW_IDS
        .into_iter()
        .map(|id| (id.to_string(), 200))
        .collect()
}

fn default_track_list_view_columns() -> HashMap<String, TrackListViewColumns> {
    let mut views = HashMap::new();
    views.insert(
        "songs".to_string(),
        TrackListViewColumns {
            columns: ["title", "artist", "album", "time"]
                .into_iter()
                .map(String::from)
                .collect(),
            widths: HashMap::new(),
        },
    );
    views.insert(
        "recentlyPlayed".to_string(),
        TrackListViewColumns {
            columns: ["title", "artist", "album", "time", "lastPlayed"]
                .into_iter()
                .map(String::from)
                .collect(),
            widths: HashMap::new(),
        },
    );
    views.insert(
        "search".to_string(),
        TrackListViewColumns {
            columns: ["title", "artist", "album", "time"]
                .into_iter()
                .map(String::from)
                .collect(),
            widths: HashMap::new(),
        },
    );
    views
}

/// Application settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jellyfin_server_url: Option<String>,
    pub offline_mode: bool,
    pub replay_gain_mode: ReplayGainMode,
    pub crossfade_duration: u32,
    /// Pause/stop/resume volume fade enabled. Read-only here; writes go
    /// through `set_fade_settings`, not `update_settings`.
    pub fade_enabled: bool,
    /// Fade ramp duration in milliseconds.
    pub fade_duration_ms: u32,
    /// Manual lyrics timing offset in milliseconds. Signed so lyrics can
    /// be pulled earlier (positive) or later (negative); a single global value.
    pub lyrics_offset_ms: i32,
    // Playback preferences
    pub volume: f64,
    pub shuffle: bool,
    pub repeat_mode: String,
    // UI layout state
    pub sidebar_collapsed: bool,
    pub view_mode: String,
    pub artist_view_mode: String,
    pub genre_view_mode: String,
    pub column_browser_visible: bool,
    pub column_browser_height: u32,
    pub column_browser_columns: Vec<String>,
    pub queue_sidebar_visible: bool,
    pub status_bar_visible: bool,
    pub track_list_view_columns: HashMap<String, TrackListViewColumns>,
    /// Per-playlist column state, keyed by Jellyfin playlist ID — a playlist missing here uses the frontend's own default.
    pub playlist_track_columns: HashMap<String, TrackListViewColumns>,
    // Sort preferences
    pub albums_sort: String,
    pub genres_sort: String,
    pub artists_sort: String,
    pub artist_albums_sort: String,
    pub tracks_sort: String,
    pub recently_added_sort: String,
    // Library sync
    /// Last library sync timestamp (Unix seconds), None if never synced.
    pub last_library_sync: Option<i64>,
    /// Library sync interval in seconds (0 = manual only).
    pub library_sync_interval: u32,
    // Display
    /// Per-view grid tile size in pixels (120-280): albums, artists, genres,
    /// recentlyAdded.
    pub grid_tile_sizes: HashMap<String, u32>,
    /// Theme preference (dark, light, or system).
    pub theme: String,
    /// Accent colour preference.
    pub accent_colour: String,
    /// Gapless playback enabled.
    pub gapless_playback: bool,
    /// ReplayGain preamp adjustment in dB (-12 to +12).
    pub replay_gain_preamp: f32,
    /// Scrobbling enabled.
    pub scrobble_enabled: bool,
    /// Scrobble threshold as percentage (25-90).
    pub scrobble_threshold: u32,
    /// UI zoom level as percentage (80-130).
    pub ui_zoom_level: u32,
    /// Number of days for "recently added" filter.
    pub recently_added_days: u32,
    /// Recently played tracks (JSON-serialised array).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recently_played_json: Option<String>,
    /// Song change notifications enabled.
    pub song_notifications_enabled: bool,
    /// Window decorations / title bar visible (Linux, for tiling WMs).
    pub title_bar_visible: bool,
    // Startup behaviour
    /// Launch Bragi automatically on system login.
    pub startup_launch_on_login: bool,
    /// Start Bragi hidden to the tray instead of showing the window.
    pub start_minimised: bool,
    // System tray behaviour
    /// Hide the window to the tray on close instead of quitting.
    pub close_to_tray: bool,
    /// Hide the window to the tray when it is minimised.
    pub minimise_to_tray: bool,
    // MiniPlayer
    /// Whether the MiniPlayer window floats above other windows.
    pub mini_player_always_on_top: bool,
    /// Preferred download format (original, flac, mp3, aac, opus).
    ///
    /// `original` (the default) keeps the exact source file; other formats
    /// request a server-side transcode. Applies to new downloads only.
    pub download_format: String,
    /// Preferred download bitrate in kbps for lossy formats (128/192/256/320).
    pub download_bitrate: u32,
    /// Where play counts come from: `auto` | `jellyfin` | `earworm`.
    pub play_count_source: String,
    // AI Playlists. API keys are stored in the keyring, never here.
    /// Whether the AI-playlist feature is enabled.
    pub ai_playlist_enabled: bool,
    /// Active media-server backend: `jellyfin` | `opensubsonic`.
    pub server_type: String,
    /// Selected LLM provider (ollama, openai, anthropic).
    pub llm_provider: String,
    /// Selected model (empty string = the provider's default).
    pub llm_model: String,
    /// User-set LLM endpoint base URL (empty = the provider's default).
    pub llm_base_url: String,
    /// Max tokens per LLM completion.
    pub llm_max_tokens: u32,
    /// Optional custom system prompt for curation (empty = built-in default).
    pub llm_system_prompt: String,
    /// Whether an API key is stored for the selected provider (read-only; the
    /// key value itself is never exposed to the frontend).
    pub llm_has_api_key: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            jellyfin_server_url: None,
            offline_mode: false,
            replay_gain_mode: ReplayGainMode::Track,
            crossfade_duration: 0,
            fade_enabled: true,    // Fade on pause/stop on by default
            fade_duration_ms: 400, // 400ms ramp default
            lyrics_offset_ms: 0,   // No lyrics timing correction by default
            volume: 1.0,
            shuffle: false,
            repeat_mode: "off".to_string(),
            sidebar_collapsed: false,
            view_mode: "grid".to_string(),
            artist_view_mode: "list".to_string(),
            genre_view_mode: "grid".to_string(),
            column_browser_visible: false,
            column_browser_height: 200,
            column_browser_columns: vec![
                "genres".to_string(),
                "artists".to_string(),
                "albums".to_string(),
            ],
            queue_sidebar_visible: false,
            status_bar_visible: true,
            track_list_view_columns: default_track_list_view_columns(),
            playlist_track_columns: HashMap::new(),
            albums_sort: "year-desc".to_string(),
            genres_sort: "name-asc".to_string(),
            artists_sort: "name-asc".to_string(),
            artist_albums_sort: "year-desc".to_string(),
            recently_added_sort: "date-added-desc".to_string(),
            tracks_sort: "name-asc".to_string(),
            last_library_sync: None,
            library_sync_interval: 0, // Manual only by default
            grid_tile_sizes: default_grid_tile_sizes(),
            // Follows the system appearance by default (Apple Music/macOS
            // convention), with an explicit override in Settings > Appearance.
            theme: "system".to_string(),
            accent_colour: "blue".to_string(),
            gapless_playback: true,                  // Enabled by default
            replay_gain_preamp: 0.0,                 // No adjustment by default
            scrobble_enabled: true,                  // Enabled by default
            scrobble_threshold: 50,                  // 50% default
            ui_zoom_level: 100,                      // 100% default
            recently_added_days: 30,                 // 30 days default
            recently_played_json: None,              // Empty by default
            song_notifications_enabled: false,       // Disabled by default
            title_bar_visible: true,                 // Decorations shown by default
            startup_launch_on_login: false,          // Do not autostart by default
            start_minimised: false,                  // Show the window on launch
            close_to_tray: true,                     // Preserve historical hide-on-close
            minimise_to_tray: false,                 // Normal minimise by default
            mini_player_always_on_top: true,         // MiniPlayer floats by default
            download_format: "original".to_string(), // No transcode by default
            download_bitrate: 256,                   // 256 kbps default for lossy
            play_count_source: "auto".to_string(),   // Jellyfin then the scrobble server
            ai_playlist_enabled: false,              // AI playlists off by default
            server_type: "jellyfin".to_string(),     // Jellyfin backend by default
            llm_provider: "ollama".to_string(),      // Local/offline default
            llm_model: String::new(),                // Provider default model
            llm_base_url: String::new(),             // Provider default endpoint
            llm_max_tokens: 2048,                    // Reasonable completion budget
            llm_system_prompt: String::new(),        // Built-in curation prompt
            llm_has_api_key: false,                  // Computed in get_settings
        }
    }
}

/// Partial settings update.
/// All fields are optional to allow partial updates.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdate {
    pub jellyfin_server_url: Option<String>,
    pub offline_mode: Option<bool>,
    pub replay_gain_mode: Option<ReplayGainMode>,
    pub crossfade_duration: Option<u32>,
    // Playback preferences
    pub volume: Option<f64>,
    pub shuffle: Option<bool>,
    pub repeat_mode: Option<String>,
    // Lyrics
    pub lyrics_offset_ms: Option<i32>,
    // UI layout state
    pub sidebar_collapsed: Option<bool>,
    pub view_mode: Option<String>,
    pub artist_view_mode: Option<String>,
    pub genre_view_mode: Option<String>,
    pub column_browser_visible: Option<bool>,
    pub column_browser_height: Option<u32>,
    pub column_browser_columns: Option<Vec<String>>,
    pub queue_sidebar_visible: Option<bool>,
    pub status_bar_visible: Option<bool>,
    pub track_list_view_columns: Option<HashMap<String, TrackListViewColumns>>,
    pub playlist_track_columns: Option<HashMap<String, TrackListViewColumns>>,
    // Sort preferences
    pub albums_sort: Option<String>,
    pub genres_sort: Option<String>,
    pub artists_sort: Option<String>,
    pub artist_albums_sort: Option<String>,
    pub recently_added_sort: Option<String>,
    pub tracks_sort: Option<String>,
    // Library sync
    pub library_sync_interval: Option<u32>,
    // Display
    pub grid_tile_sizes: Option<HashMap<String, u32>>,
    pub theme: Option<String>,
    pub accent_colour: Option<String>,
    // Playback
    pub gapless_playback: Option<bool>,
    pub replay_gain_preamp: Option<f32>,
    // Scrobbling
    pub scrobble_enabled: Option<bool>,
    pub scrobble_threshold: Option<u32>,
    // Display
    pub ui_zoom_level: Option<u32>,
    // Library
    pub recently_added_days: Option<u32>,
    // Recently played
    pub recently_played_json: Option<String>,
    // Notifications
    pub song_notifications_enabled: Option<bool>,
    // Window decorations (Linux)
    pub title_bar_visible: Option<bool>,
    // Startup behaviour. Note: startup_launch_on_login is NOT accepted
    // here — it is owned by the dedicated set_launch_on_login command which
    // also performs the OS-level enable/disable.
    pub start_minimised: Option<bool>,
    // System tray behaviour
    pub close_to_tray: Option<bool>,
    pub minimise_to_tray: Option<bool>,
    // MiniPlayer
    pub mini_player_always_on_top: Option<bool>,
    // Downloads / transcoding
    pub download_format: Option<String>,
    pub download_bitrate: Option<u32>,
    pub play_count_source: Option<String>,
    /// Media-server backend selector.
    pub server_type: Option<String>,
    // AI Playlists. API keys are set via set_llm_api_key, not here.
    pub ai_playlist_enabled: Option<bool>,
    pub llm_provider: Option<String>,
    pub llm_model: Option<String>,
    pub llm_base_url: Option<String>,
    pub llm_max_tokens: Option<u32>,
    pub llm_system_prompt: Option<String>,
}

/// A stored value parsed as `T`, or `default` when unset or unparseable.
fn parsed<T: FromStr>(all: &HashMap<String, String>, key: &str, default: T) -> T {
    all.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// A stored string accepted by `valid`, or `default`.
fn text(
    all: &HashMap<String, String>,
    key: &str,
    default: String,
    valid: impl Fn(&str) -> bool,
) -> String {
    all.get(key)
        .filter(|v| valid(v))
        .cloned()
        .unwrap_or(default)
}

/// A stored JSON value, or `default` when unset; malformed JSON is an error.
fn json<T: DeserializeOwned>(
    all: &HashMap<String, String>,
    key: &str,
    default: T,
) -> CommandResult<T> {
    match all.get(key) {
        Some(v) => serde_json::from_str(v).map_err(CommandError::msg),
        None => Ok(default),
    }
}

/// Get current application settings.
#[tauri::command]
pub async fn get_settings(state: tauri::State<'_, AppState>) -> CommandResult<AppSettings> {
    let db = &state.db;
    let d = AppSettings::default();
    let all = db.get_all_settings()?;
    let any = |_: &str| true;

    // Surface the ACTIVE backend's last-sync time: the timestamp is now
    // keyed per backend, so fall back to the legacy global key for pre-upgrade
    // installs that have not yet re-synced.
    let last_library_sync = all
        .get(&keys::last_library_sync_key(db.active_source()))
        .or_else(|| all.get(keys::LAST_LIBRARY_SYNC))
        .and_then(|v| v.parse::<i64>().ok());

    let llm_provider = text(&all, keys::LLM_PROVIDER, d.llm_provider, |v| {
        crate::llm::Provider::parse(v).is_some()
    });

    // Whether a key is stored for the selected provider (never the value).
    // Providers that need no key (Ollama, the default) skip the keyring
    // entirely; the rest read it on a blocking thread under a timeout so a
    // stalled macOS Keychain prompt can't hang this hot, sidebar-polled
    // command — the "AI Playlist button never appears" regression.
    let llm_has_api_key = match crate::llm::Provider::parse(&llm_provider) {
        Some(provider) if provider.needs_api_key() => {
            crate::llm::credentials::has_key_timeout(provider).await
        }
        _ => false,
    };

    Ok(AppSettings {
        jellyfin_server_url: all.get(keys::JELLYFIN_SERVER_URL).cloned(),
        offline_mode: parsed(&all, keys::OFFLINE_MODE, d.offline_mode),
        replay_gain_mode: all
            .get(keys::REPLAYGAIN_MODE)
            .and_then(|v| serde_json::from_value(v.as_str().into()).ok())
            .unwrap_or(d.replay_gain_mode),
        crossfade_duration: parsed(&all, keys::CROSSFADE_DURATION, d.crossfade_duration),
        fade_enabled: parsed(&all, keys::FADE_ENABLED, d.fade_enabled),
        fade_duration_ms: parsed(&all, keys::FADE_DURATION_MS, d.fade_duration_ms),
        lyrics_offset_ms: parsed(&all, keys::LYRICS_OFFSET_MS, d.lyrics_offset_ms),
        volume: parsed(&all, keys::VOLUME, d.volume),
        shuffle: parsed(&all, keys::SHUFFLE, d.shuffle),
        repeat_mode: text(&all, keys::REPEAT_MODE, d.repeat_mode, any),
        sidebar_collapsed: parsed(&all, keys::SIDEBAR_COLLAPSED, d.sidebar_collapsed),
        view_mode: text(&all, keys::VIEW_MODE, d.view_mode, any),
        artist_view_mode: text(&all, keys::ARTIST_VIEW_MODE, d.artist_view_mode, any),
        genre_view_mode: text(&all, keys::GENRE_VIEW_MODE, d.genre_view_mode, any),
        column_browser_visible: parsed(
            &all,
            keys::COLUMN_BROWSER_VISIBLE,
            d.column_browser_visible,
        ),
        column_browser_height: parsed(&all, keys::COLUMN_BROWSER_HEIGHT, d.column_browser_height),
        column_browser_columns: json(&all, keys::COLUMN_BROWSER_COLUMNS, d.column_browser_columns)?,
        queue_sidebar_visible: parsed(&all, keys::QUEUE_SIDEBAR_VISIBLE, d.queue_sidebar_visible),
        status_bar_visible: parsed(&all, keys::STATUS_BAR_VISIBLE, d.status_bar_visible),
        track_list_view_columns: json(
            &all,
            keys::TRACK_LIST_VIEW_COLUMNS,
            d.track_list_view_columns,
        )?,
        playlist_track_columns: json(&all, keys::PLAYLIST_TRACK_COLUMNS, d.playlist_track_columns)?,
        albums_sort: text(&all, keys::ALBUMS_SORT, d.albums_sort, any),
        genres_sort: text(&all, keys::GENRES_SORT, d.genres_sort, any),
        artists_sort: text(&all, keys::ARTISTS_SORT, d.artists_sort, any),
        artist_albums_sort: text(&all, keys::ARTIST_ALBUMS_SORT, d.artist_albums_sort, any),
        recently_added_sort: text(&all, keys::RECENTLY_ADDED_SORT, d.recently_added_sort, any),
        tracks_sort: text(&all, keys::TRACKS_SORT, d.tracks_sort, any),
        last_library_sync,
        library_sync_interval: parsed(&all, keys::LIBRARY_SYNC_INTERVAL, d.library_sync_interval),
        grid_tile_sizes: json(&all, keys::GRID_TILE_SIZES, d.grid_tile_sizes)?,
        theme: text(&all, keys::THEME, d.theme, any),
        accent_colour: text(&all, keys::ACCENT_COLOUR, d.accent_colour, any),
        gapless_playback: parsed(&all, keys::GAPLESS_PLAYBACK, d.gapless_playback),
        replay_gain_preamp: parsed(&all, keys::REPLAYGAIN_PREAMP, d.replay_gain_preamp),
        scrobble_enabled: parsed(&all, keys::SCROBBLE_ENABLED, d.scrobble_enabled),
        scrobble_threshold: parsed(&all, keys::SCROBBLE_THRESHOLD, d.scrobble_threshold),
        ui_zoom_level: parsed(&all, keys::UI_ZOOM_LEVEL, d.ui_zoom_level),
        recently_added_days: parsed(&all, keys::RECENTLY_ADDED_DAYS, d.recently_added_days),
        recently_played_json: all.get(keys::RECENTLY_PLAYED_JSON).cloned(),
        song_notifications_enabled: parsed(
            &all,
            keys::SONG_NOTIFICATIONS_ENABLED,
            d.song_notifications_enabled,
        ),
        title_bar_visible: parsed(&all, keys::TITLE_BAR_VISIBLE, d.title_bar_visible),
        startup_launch_on_login: parsed(
            &all,
            keys::STARTUP_LAUNCH_ON_LOGIN,
            d.startup_launch_on_login,
        ),
        start_minimised: parsed(&all, keys::START_MINIMISED, d.start_minimised),
        close_to_tray: parsed(&all, keys::CLOSE_TO_TRAY, d.close_to_tray),
        minimise_to_tray: parsed(&all, keys::MINIMISE_TO_TRAY, d.minimise_to_tray),
        mini_player_always_on_top: parsed(
            &all,
            keys::MINI_PLAYER_ALWAYS_ON_TOP,
            d.mini_player_always_on_top,
        ),
        download_format: text(&all, keys::DOWNLOAD_FORMAT, d.download_format, |v| {
            DownloadFormat::parse(v).is_some()
        }),
        download_bitrate: parsed(&all, keys::DOWNLOAD_BITRATE, d.download_bitrate),
        play_count_source: text(&all, keys::PLAY_COUNT_SOURCE, d.play_count_source, |v| {
            matches!(v, "auto" | "jellyfin" | "earworm")
        }),
        ai_playlist_enabled: parsed(&all, keys::AI_PLAYLIST_ENABLED, d.ai_playlist_enabled),
        server_type: text(&all, keys::SERVER_TYPE, d.server_type, |v| {
            crate::mediaserver::ServerKind::parse(v).is_some()
        }),
        llm_provider,
        llm_model: text(&all, keys::LLM_MODEL, d.llm_model, any),
        llm_base_url: text(&all, keys::LLM_BASE_URL, d.llm_base_url, any),
        llm_max_tokens: parsed(&all, keys::LLM_MAX_TOKENS, d.llm_max_tokens),
        llm_system_prompt: text(&all, keys::LLM_SYSTEM_PROMPT, d.llm_system_prompt, any),
        llm_has_api_key,
    })
}

/// Update application settings.
#[tauri::command]
pub async fn update_settings(
    state: tauri::State<'_, AppState>,
    settings: SettingsUpdate,
) -> CommandResult<()> {
    let db = &state.db;
    let s = &settings;
    let mut u: Vec<(&str, String)> = Vec::new();

    // Plain values stored via `to_string`, and strings stored as given.
    macro_rules! put {
            ($($key:expr => $field:ident),* $(,)?) => {$(
                if let Some(v) = &s.$field {
                    u.push(($key, v.to_string()));
                }
            )*};
        }
    // Structured values stored as JSON.
    macro_rules! put_json {
            ($($key:expr => $field:ident),* $(,)?) => {$(
                if let Some(v) = &s.$field {
                    u.push(($key, serde_json::to_string(v).map_err(CommandError::msg)?));
                }
            )*};
        }

    put! {
        keys::JELLYFIN_SERVER_URL => jellyfin_server_url,
        keys::OFFLINE_MODE => offline_mode,
        keys::CROSSFADE_DURATION => crossfade_duration,
        keys::VOLUME => volume,
        keys::SHUFFLE => shuffle,
        keys::REPEAT_MODE => repeat_mode,
        keys::LYRICS_OFFSET_MS => lyrics_offset_ms,
        keys::SIDEBAR_COLLAPSED => sidebar_collapsed,
        keys::VIEW_MODE => view_mode,
        keys::ARTIST_VIEW_MODE => artist_view_mode,
        keys::GENRE_VIEW_MODE => genre_view_mode,
        keys::COLUMN_BROWSER_VISIBLE => column_browser_visible,
        keys::COLUMN_BROWSER_HEIGHT => column_browser_height,
        keys::QUEUE_SIDEBAR_VISIBLE => queue_sidebar_visible,
        keys::STATUS_BAR_VISIBLE => status_bar_visible,
        keys::ALBUMS_SORT => albums_sort,
        keys::ARTISTS_SORT => artists_sort,
        keys::GENRES_SORT => genres_sort,
        keys::ARTIST_ALBUMS_SORT => artist_albums_sort,
        keys::RECENTLY_ADDED_SORT => recently_added_sort,
        keys::TRACKS_SORT => tracks_sort,
        keys::LIBRARY_SYNC_INTERVAL => library_sync_interval,
        keys::THEME => theme,
        keys::ACCENT_COLOUR => accent_colour,
        keys::GAPLESS_PLAYBACK => gapless_playback,
        keys::REPLAYGAIN_PREAMP => replay_gain_preamp,
        keys::SCROBBLE_ENABLED => scrobble_enabled,
        keys::SCROBBLE_THRESHOLD => scrobble_threshold,
        keys::UI_ZOOM_LEVEL => ui_zoom_level,
        keys::RECENTLY_ADDED_DAYS => recently_added_days,
        keys::RECENTLY_PLAYED_JSON => recently_played_json,
        keys::SONG_NOTIFICATIONS_ENABLED => song_notifications_enabled,
        keys::TITLE_BAR_VISIBLE => title_bar_visible,
        keys::START_MINIMISED => start_minimised,
        keys::CLOSE_TO_TRAY => close_to_tray,
        keys::MINIMISE_TO_TRAY => minimise_to_tray,
        keys::MINI_PLAYER_ALWAYS_ON_TOP => mini_player_always_on_top,
        keys::AI_PLAYLIST_ENABLED => ai_playlist_enabled,
        keys::LLM_MODEL => llm_model,
        keys::LLM_BASE_URL => llm_base_url,
        keys::LLM_SYSTEM_PROMPT => llm_system_prompt,
    }
    put_json! {
        keys::COLUMN_BROWSER_COLUMNS => column_browser_columns,
        keys::TRACK_LIST_VIEW_COLUMNS => track_list_view_columns,
        keys::PLAYLIST_TRACK_COLUMNS => playlist_track_columns,
        keys::GRID_TILE_SIZES => grid_tile_sizes,
    }

    if let Some(mode) = s.replay_gain_mode {
        let mode_str = match mode {
            ReplayGainMode::Track => "track",
            ReplayGainMode::Album => "album",
            ReplayGainMode::Off => "off",
        };
        u.push((keys::REPLAYGAIN_MODE, mode_str.to_string()));
    }

    // Tray-behaviour toggles also update the lock-free atomic cache the
    // window-event handler reads, so changes take effect without a restart.
    if let Some(v) = s.close_to_tray {
        state.close_to_tray.store(v, Ordering::Relaxed);
    }
    if let Some(v) = s.minimise_to_tray {
        state.minimise_to_tray.store(v, Ordering::Relaxed);
    }

    // Validated values: unknown ones are ignored.
    if let Some(parsed) = s.download_format.as_deref().and_then(DownloadFormat::parse) {
        u.push((keys::DOWNLOAD_FORMAT, parsed.as_str().to_string()));
    }
    if let Some(bitrate) = s.download_bitrate {
        // Clamp to a known set of lossy bitrates.
        let clamped = match bitrate {
            0..=159 => 128,
            160..=223 => 192,
            224..=287 => 256,
            _ => 320,
        };
        u.push((keys::DOWNLOAD_BITRATE, clamped.to_string()));
    }
    if let Some(source) = s
        .play_count_source
        .as_ref()
        .filter(|v| matches!(v.as_str(), "auto" | "jellyfin" | "earworm"))
    {
        u.push((keys::PLAY_COUNT_SOURCE, source.clone()));
    }
    // Only persist a recognised backend and provider.
    if let Some(parsed) = s
        .server_type
        .as_deref()
        .and_then(crate::mediaserver::ServerKind::parse)
    {
        u.push((keys::SERVER_TYPE, parsed.as_str().to_string()));
    }
    if let Some(parsed) = s
        .llm_provider
        .as_deref()
        .and_then(crate::llm::Provider::parse)
    {
        u.push((keys::LLM_PROVIDER, parsed.as_str().to_string()));
    }
    if let Some(max_tokens) = s.llm_max_tokens {
        u.push((
            keys::LLM_MAX_TOKENS,
            max_tokens.clamp(256, 8192).to_string(),
        ));
    }

    if !u.is_empty() {
        let pairs: Vec<(&str, &str)> = u.iter().map(|(k, v)| (*k, v.as_str())).collect();
        db.set_settings(&pairs)?;
    }

    tracing::debug!("Settings updated: {:?}", settings);
    Ok(())
}

/// Quit the application unconditionally.
///
/// Routes around the close-to-tray CloseRequested interception so keyboard
/// shortcuts (Ctrl/Cmd+Q) and any explicit "quit" action always exit, even
/// when close-to-tray is enabled. `app.exit(0)` triggers `RunEvent::Exit`,
/// which performs the mpv graceful-shutdown.
#[tauri::command]
pub async fn quit_app(app: tauri::AppHandle) -> CommandResult<()> {
    tracing::info!("Quit requested (quit_app command)");
    app.exit(0);
    Ok(())
}

/// Enable or disable launch-on-login.
///
/// Performs the OS-level side effect first (via the autostart plugin, which
/// writes the XDG .desktop / macOS LaunchAgent plist / Windows registry entry)
/// then persists the DB flag so `get_settings` reflects it. Modelled on
/// `set_eq_enabled`: a dedicated command carrying an OS side effect, kept out
/// of the pure-DB `update_settings` batch.
#[tauri::command]
pub async fn set_launch_on_login(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> CommandResult<()> {
    use tauri_plugin_autostart::ManagerExt;

    tracing::info!("Setting launch-on-login: {}", enabled);

    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(CommandError::msg)?;
    } else {
        manager.disable().map_err(CommandError::msg)?;
    }

    state.db.set_setting(
        keys::STARTUP_LAUNCH_ON_LOGIN,
        if enabled { "true" } else { "false" },
    )?;

    Ok(())
}

/// Application info for About page.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub tauri_version: String,
    pub platform: String,
    pub arch: String,
    pub data_dir: String,
}

/// Get application info.
#[tauri::command]
pub async fn get_app_info(app: tauri::AppHandle) -> CommandResult<AppInfo> {
    let version = app.package_info().version.to_string();
    let tauri_version = tauri::VERSION.to_string();
    let platform = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();

    let data_dir = app
        .path()
        .app_data_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    Ok(AppInfo {
        version,
        tauri_version,
        platform,
        arch,
        data_dir,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_startup_and_tray_settings() {
        let defaults = AppSettings::default();
        // No autostart, show the window normally on launch.
        assert!(!defaults.startup_launch_on_login);
        assert!(!defaults.start_minimised);
        // close_to_tray=true preserves the historical always-hide-on-close
        // behaviour; minimise is a normal minimise unless opted in.
        assert!(defaults.close_to_tray);
        assert!(!defaults.minimise_to_tray);
    }

    /// Pins `AppSettings::default()` against the fixture the frontend
    /// preferences store's own test (`preferences.svelte.test.ts`) reads, so
    /// the two can no longer drift silently: change a default on either side
    /// without updating `src-tauri/fixtures/settings-defaults.json` and this
    /// test fails.
    #[test]
    fn defaults_match_the_shared_frontend_fixture() {
        let defaults = serde_json::to_value(AppSettings::default()).unwrap();
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../fixtures/settings-defaults.json")).unwrap();

        for (key, expected) in fixture.as_object().unwrap() {
            assert_eq!(
                defaults.get(key),
                Some(expected),
                "AppSettings::default().{key} no longer matches \
                 src-tauri/fixtures/settings-defaults.json — update whichever \
                 side is wrong (the frontend preferences store's test reads \
                 the same fixture)"
            );
        }
    }
}
