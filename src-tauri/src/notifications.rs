//! Song change notifications.
//!
//! Handles native OS notifications when the currently playing track changes.

use std::sync::Mutex;

use tauri::Manager;
use tauri_plugin_notification::NotificationExt;

use crate::db::settings::keys;
use crate::models::Track;
use crate::state::AppState;

/// State for tracking the last notified track.
///
/// We track the Jellyfin ID of the last track we sent a notification for
/// to avoid duplicate notifications for pause/resume events.
pub struct NotificationState {
    last_track_id: Mutex<Option<String>>,
}

impl NotificationState {
    pub fn new() -> Self {
        Self {
            last_track_id: Mutex::new(None),
        }
    }
}

/// Check if song notifications are enabled.
fn is_enabled(app: &tauri::AppHandle) -> bool {
    let state = match app.try_state::<AppState>() {
        Some(s) => s,
        None => return false,
    };

    state
        .db
        .get_setting(keys::SONG_NOTIFICATIONS_ENABLED)
        .ok()
        .flatten()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false)
}

/// Check if the main window is currently focused.
fn is_window_focused(app: &tauri::AppHandle) -> bool {
    app.get_webview_window("main")
        .map(|w| w.is_focused().unwrap_or(false))
        .unwrap_or(false)
}

/// Handle a potential track change event.
///
/// Sends a notification if:
/// - Song notifications are enabled
/// - The track is different from the last notified track
/// - The main window is not focused
/// - Playback is active (not paused)
pub fn on_track_change(app: &tauri::AppHandle, track: Option<&Track>, is_playing: bool) {
    // No notification if no track or not playing
    let track = match track {
        Some(t) if is_playing => t,
        _ => return,
    };

    // Get or create notification state
    let notification_state = match app.try_state::<NotificationState>() {
        Some(s) => s,
        None => return,
    };

    // Check if this is a new track
    let mut last_id = match notification_state.last_track_id.lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };

    if last_id.as_ref() == Some(&track.jellyfin_id) {
        // Same track, no notification (e.g., resume after pause)
        return;
    }

    // Update the last track ID
    *last_id = Some(track.jellyfin_id.clone());
    drop(last_id);

    // Check if notifications are enabled
    if !is_enabled(app) {
        return;
    }

    // Check if window is focused (no notification if user is looking at the app)
    if is_window_focused(app) {
        return;
    }

    // Send the notification
    let title = &track.name;
    let body = format!("{} — {}", track.artist_name, track.album_name);

    let mut builder = app.notification().builder().title(title).body(&body);

    // Attach the album artwork if we have it cached locally. The OS notification
    // system renders this as the standard media layout (art on the left, text on
    // the right). Missing cache files fall back to a text-only notification.
    if let Some(icon_path) = artwork_icon_path(app, track) {
        builder = builder.icon(icon_path);
    }

    if let Err(e) = builder.show() {
        tracing::warn!("Failed to show song notification: {}", e);
    } else {
        tracing::debug!(
            "Sent song notification for: {} - {}",
            track.artist_name,
            track.name
        );
    }
}

/// Resolve the locally cached album artwork path for a track's notification icon.
///
/// Returns the absolute filesystem path (as a string) of the cached album art
/// for the track's album, or `None` if the app state is unavailable, the album
/// has no ID, or the artwork has not been cached yet.
fn artwork_icon_path(app: &tauri::AppHandle, track: &Track) -> Option<String> {
    if track.album_jellyfin_id.is_empty() {
        return None;
    }

    let state = app.try_state::<AppState>()?;
    let path =
        crate::artwork::cached_artwork_path(&state.artwork_cache_dir, &track.album_jellyfin_id)?;

    Some(path.to_string_lossy().into_owned())
}
