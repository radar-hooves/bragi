//! Developer-related Tauri commands.
//!
//! Commands for debugging, diagnostics, and developer tools.

use tauri_plugin_notification::NotificationExt;

use crate::error::CommandResult;
use crate::state::AppState;
use serde::Serialize;
use std::fs;
use tauri::Manager;

/// Developer/diagnostic information.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeveloperInfo {
    /// Path to the SQLite database file.
    pub database_path: String,
    /// Size of the database file in bytes.
    pub database_size_bytes: u64,
    /// Path to the audio cache directory.
    pub cache_dir: String,
    /// Whether running in debug mode.
    pub debug_mode: bool,
    /// Device ID used for scrobbling.
    pub device_id: Option<String>,
}

/// Get developer/diagnostic information.
#[tauri::command]
pub async fn get_developer_info(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CommandResult<DeveloperInfo> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;

    let database_path = data_dir.join("bragi.db");

    let database_size_bytes = fs::metadata(&database_path).map(|m| m.len()).unwrap_or(0);

    let cache_dir = app
        .path()
        .app_cache_dir()
        .map(|p| p.join("audio"))
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    // Get device ID if available
    let device_id = state.db.get_setting("device_id").ok().flatten();

    Ok(DeveloperInfo {
        database_path: database_path.to_string_lossy().to_string(),
        database_size_bytes,
        cache_dir,
        debug_mode: cfg!(debug_assertions),
        device_id,
    })
}

/// Open the data directory in the system file manager.
#[tauri::command]
pub async fn open_data_directory(app: tauri::AppHandle) -> CommandResult<()> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;

    open_in_file_manager(&data_dir)
}

/// Open the cache directory in the system file manager.
#[tauri::command]
pub async fn open_cache_directory(app: tauri::AppHandle) -> CommandResult<()> {
    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("Failed to get app cache dir: {}", e))?
        .join("audio");

    // Create cache dir if it doesn't exist
    if !cache_dir.exists() {
        fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create cache directory: {}", e))?;
    }

    open_in_file_manager(&cache_dir)
}

/// Open a path in the system file manager.
fn open_in_file_manager(path: &std::path::Path) -> CommandResult<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| format!("Failed to open Finder: {}", e))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| format!("Failed to open Explorer: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| format!("Failed to open file manager: {}", e))?;
    }

    Ok(())
}

/// Clear all failed sync operations from the queue.
///
/// Operations that have exceeded the maximum retry attempts can be
/// cleared to prevent them from clogging the queue.
#[tauri::command]
pub async fn clear_failed_sync_operations(state: tauri::State<'_, AppState>) -> CommandResult<u32> {
    const MAX_ATTEMPTS: u32 = 5;

    let cleared = state
        .db
        .cleanup_failed(MAX_ATTEMPTS)
        .map_err(|e| format!("Failed to clear failed operations: {}", e))?;

    tracing::info!("Cleared {} failed sync operations", cleared);
    Ok(cleared)
}

/// Send a test notification to verify notifications are working.
#[tauri::command]
pub async fn send_test_notification(app: tauri::AppHandle) -> CommandResult<()> {
    let title = "Bragi Test Notification";
    let body = "If you see this, notifications are working!";

    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| format!("Failed to show test notification: {}", e))?;

    tracing::info!("Sent test notification");
    Ok(())
}
