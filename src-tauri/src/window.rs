//! Bragi's windows (`main`, the MiniPlayer) and which one is in front. Frame
//! persistence, single instance, hide-on-close and the display fit are
//! `desktop_shell::window`'s, wired in `lib.rs`.

use tauri::{AppHandle, Manager, Runtime};

use crate::commands::MINI_LABEL;

/// Window label for the main window.
pub const MAIN_LABEL: &str = "main";

/// Main window's declared minimum size, matching `tauri.conf.json` (pinned
/// against drift by a test below).
pub const MAIN_MIN_SIZE: (u32, u32) = (520, 400);

/// Which window currently represents Bragi: the MiniPlayer when it is open
/// (opening it hides `main`), the main window otherwise.
pub fn active_label<R: Runtime>(app: &AppHandle<R>) -> &'static str {
    resolve_active_label(app.get_webview_window(MINI_LABEL).is_some())
}

fn resolve_active_label(mini_is_open: bool) -> &'static str {
    if mini_is_open {
        MINI_LABEL
    } else {
        MAIN_LABEL
    }
}

/// Bring whichever window represents Bragi to the front: the tray, a second
/// launch and the Dock icon all use it.
pub fn show_active<R: Runtime>(app: &AppHandle<R>) {
    desktop_shell::window::show(app, active_label(app));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_label_prefers_the_miniplayer_when_it_is_open() {
        assert_eq!(resolve_active_label(true), MINI_LABEL);
        assert_eq!(resolve_active_label(false), MAIN_LABEL);
    }

    /// `MAIN_MIN_SIZE` and `tauri.conf.json`'s `minWidth`/`minHeight` are
    /// declared in different languages; only a test keeps them in step.
    #[test]
    fn main_min_size_matches_tauri_conf() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("valid JSON");
        let window_conf = &conf["app"]["windows"][0];
        assert_eq!(
            window_conf["minWidth"].as_u64().expect("minWidth") as u32,
            MAIN_MIN_SIZE.0
        );
        assert_eq!(
            window_conf["minHeight"].as_u64().expect("minHeight") as u32,
            MAIN_MIN_SIZE.1
        );
    }
}
