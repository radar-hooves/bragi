//! MiniPlayer window management.
//!
//! The MiniPlayer is a compact, always-on-top **second Tauri window** (label
//! `mini`) — not a CSS resize of the main window. It loads the same SPA
//! (`index.html`); `+layout.svelte` branches on the window label to render the
//! `MiniPlayer` component and run a trimmed init. Playback state stays in sync
//! for free because the audio engine broadcasts events to every window
//! (`app.emit`), so this module only manages the window lifecycle.
//!
//! Replace model: opening the MiniPlayer hides the main window; closing it (via
//! this command, the native close control, or the macOS red traffic light)
//! restores the main window. The "restore main on mini close" half lives in the
//! `on_window_event` handler in `lib.rs` so it covers every close path and we
//! never leave zero visible windows. Bringing a window forward
//! (unminimize/show/focus) and deciding which window is the active one are
//! `desktop_shell::window::show`/`window::active_label` — the one mechanism this
//! module shares with `tray.rs`, not a copy of its own.

use crate::db::settings::keys;
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;
use crate::window;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Window label for the MiniPlayer.
pub const MINI_LABEL: &str = "mini";

/// MiniPlayer size bounds, in logical pixels.
pub const MIN_SIZE: (f64, f64) = (280.0, 84.0);
pub const MAX_SIZE: (f64, f64) = (480.0, 160.0);

/// Toggle the MiniPlayer: open it (hiding the main window) if it is closed, or
/// close it (restoring the main window) if it is open.
#[tauri::command]
pub async fn toggle_mini_player(app: tauri::AppHandle) -> CommandResult<()> {
    if app.get_webview_window(MINI_LABEL).is_some() {
        close_mini_player_inner(&app);
        Ok(())
    } else {
        open_mini_player_inner(&app).map_err(CommandError::msg)
    }
}

/// Open the MiniPlayer (idempotent — focuses it if already open) and hide the
/// main window.
#[tauri::command]
pub async fn open_mini_player(app: tauri::AppHandle) -> CommandResult<()> {
    open_mini_player_inner(&app).map_err(CommandError::msg)
}

/// Close the MiniPlayer and restore the main window.
#[tauri::command]
pub async fn close_mini_player(app: tauri::AppHandle) -> CommandResult<()> {
    close_mini_player_inner(&app);
    Ok(())
}

/// Set whether the MiniPlayer floats above other windows, and persist the
/// preference so the next MiniPlayer opens the same way.
#[tauri::command]
pub async fn set_mini_always_on_top(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> CommandResult<()> {
    if let Some(window) = app.get_webview_window(MINI_LABEL) {
        window
            .set_always_on_top(enabled)
            .map_err(CommandError::msg)?;
        // Tie the taskbar entry to the floating state: when NOT floating, expose
        // the mini in the taskbar/switcher so it can be raised after losing
        // focus (otherwise it could be stranded with no way to bring it forward).
        let _ = window.set_skip_taskbar(enabled);
    }
    state
        .db
        .set_setting(
            keys::MINI_PLAYER_ALWAYS_ON_TOP,
            if enabled { "true" } else { "false" },
        )
        .map_err(CommandError::msg)
}

/// Build (or focus) the MiniPlayer window and hide the main window.
fn open_mini_player_inner(app: &tauri::AppHandle) -> tauri::Result<()> {
    // Already open — just show and focus it (never duplicate the window).
    if let Some(window) = app.get_webview_window(MINI_LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }

    // Honour the persisted always-on-top preference (default: float).
    let always_on_top = app
        .state::<AppState>()
        .db
        .get_parsed(keys::MINI_PLAYER_ALWAYS_ON_TOP)
        .ok()
        .flatten()
        .unwrap_or(true);

    #[allow(unused_mut)]
    let mut builder =
        WebviewWindowBuilder::new(app, MINI_LABEL, WebviewUrl::App("index.html".into()))
            .title("Bragi MiniPlayer")
            .inner_size(320.0, 96.0)
            .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
            .max_inner_size(MAX_SIZE.0, MAX_SIZE.1)
            .resizable(true)
            // Not minimisable: on macOS the native yellow traffic light would
            // otherwise send the mini to the Dock while the main window is
            // hidden, leaving zero visible windows (the replace-model invariant).
            .minimizable(false)
            .always_on_top(always_on_top)
            // Skip the taskbar only while floating. When always-on-top is off the
            // mini is an ordinary window that could be occluded, so it needs a
            // taskbar/switcher entry to be raised again (see set_mini_always_on_top).
            .skip_taskbar(always_on_top)
            // macOS keeps native decorations (traffic lights); Linux runs
            // decorations-off with in-app custom controls, matching the main
            // window's per-OS chrome.
            .decorations(cfg!(target_os = "macos"));

    // macOS: overlay title-bar style so the native traffic lights float over
    // the content where AppKit puts them; MiniPlayer.svelte's top strip
    // clears them.
    #[cfg(target_os = "macos")]
    {
        builder = builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true);
    }

    let mini = builder.build()?;

    // Replace model: hide the main window while the MiniPlayer is up. Native
    // macOS fullscreen and a floating MiniPlayer don't mix (a fullscreen
    // Space hidden behind the MiniPlayer is not somewhere "close" can return
    // to), so drop out of fullscreen first if the main window is in it.
    if let Some(main) = app.get_webview_window(window::MAIN_LABEL) {
        if main.is_fullscreen().unwrap_or(false) {
            let _ = main.set_fullscreen(false);
        }
        let _ = main.hide();
    }

    let _ = mini.set_focus();
    tracing::info!("MiniPlayer opened");
    Ok(())
}

/// Close the MiniPlayer window. The main window is restored by the
/// `on_window_event` handler in `lib.rs` (fires for every close path). If the
/// MiniPlayer is not open, ensure the main window is visible regardless.
fn close_mini_player_inner(app: &tauri::AppHandle) {
    if let Some(mini) = app.get_webview_window(MINI_LABEL) {
        let _ = mini.close();
    } else {
        desktop_shell::window::show(app, window::MAIN_LABEL);
    }
}
