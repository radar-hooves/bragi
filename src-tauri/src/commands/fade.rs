//! Fade-on-pause/stop Tauri commands.
//!
//! Controls the stepped mpv-IPC volume ramp that fades out on pause/stop and
//! fades in on resume. Settings are persisted to the database for cross-session
//! storage, and the live behaviour is applied by dispatching
//! `AudioCommand::SetFade` to the engine — mirroring `commands::equalizer`.

use crate::audio::{AudioCommand, AudioEngineHandle};
use crate::db::settings::keys;
use crate::db::Database;
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

/// Product default fade ramp duration in milliseconds.
const DEFAULT_FADE_DURATION_MS: u32 = 400;

/// Enable/disable the pause/stop volume fade and set its ramp duration.
///
/// Persists both settings and applies them to the running engine. Duration is
/// clamped engine-side to a sane maximum.
#[tauri::command]
pub async fn set_fade_settings(
    state: tauri::State<'_, AppState>,
    enabled: bool,
    duration_ms: u32,
) -> CommandResult<()> {
    tracing::info!(
        "Setting fade: enabled={}, duration_ms={}",
        enabled,
        duration_ms
    );

    // Persist to database
    state
        .db
        .set_setting(keys::FADE_ENABLED, if enabled { "true" } else { "false" })?;
    state
        .db
        .set_setting(keys::FADE_DURATION_MS, &duration_ms.to_string())?;

    // Apply to audio engine
    state
        .get_audio()
        .await?
        .send(AudioCommand::SetFade {
            enabled,
            duration_ms: duration_ms as u64,
        })
        .map_err(CommandError::msg)
}

/// Replay the user's persisted fade settings into a freshly-started engine.
///
/// Called once on startup, right after the engine handle becomes available
/// (mirroring [`crate::commands::apply_persisted_eq`]). Always sends the
/// command: the engine default already matches the product default (ON, 400ms),
/// so this is idempotent when nothing was ever configured, and it faithfully
/// restores an explicit "off" or a custom duration.
pub fn apply_persisted_fade(db: &Database, audio: &AudioEngineHandle) {
    let enabled = match db.get_setting(keys::FADE_ENABLED) {
        Ok(value) => value.map(|v| v == "true").unwrap_or(true),
        Err(e) => {
            tracing::warn!("Failed to read persisted fade enabled flag: {}", e);
            return;
        }
    };

    let duration_ms = db
        .get_parsed(keys::FADE_DURATION_MS)
        .ok()
        .flatten()
        .unwrap_or(DEFAULT_FADE_DURATION_MS);

    if let Err(e) = audio.send(AudioCommand::SetFade {
        enabled,
        duration_ms: duration_ms as u64,
    }) {
        tracing::warn!("Failed to apply persisted fade settings on startup: {}", e);
        return;
    }

    tracing::info!(
        "Applied persisted fade settings on startup (enabled: {}, duration_ms: {})",
        enabled,
        duration_ms
    );
}
