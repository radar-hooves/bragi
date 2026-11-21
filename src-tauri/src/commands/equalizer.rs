//! Equalizer-related Tauri commands.
//!
//! Commands for controlling the audio equalizer with 10 and 18-band presets.
//! Settings are persisted to the database for cross-session storage, and the
//! live filter is applied by dispatching `AudioCommand::SetEq*` to the engine.

use crate::audio::eq::{extract_10_from_18, map_10_to_18_bands, EqPreset};
use crate::audio::{AudioCommand, AudioEngineHandle};
use crate::db::settings::keys;
use crate::db::Database;
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;
use serde::{Deserialize, Serialize};

/// Equalizer preset information for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqPresetInfo {
    pub name: String,
    pub display_name: String,
    pub bands_10: Vec<i32>,
    pub bands_18: Vec<i32>,
    /// `true` for the shipped presets, `false` for user-saved profiles (which
    /// the UI may rename/delete).
    pub builtin: bool,
}

impl From<&EqPreset> for EqPresetInfo {
    fn from(preset: &EqPreset) -> Self {
        Self {
            name: preset.as_str().to_lowercase().replace(' ', "_"),
            display_name: preset.as_str().to_string(),
            bands_10: preset.bands_10().to_vec(),
            bands_18: preset.bands_18().to_vec(),
            builtin: true,
        }
    }
}

/// A user-saved named EQ profile. Bands are always stored as 18 values (the
/// backend's canonical resolution); the 10-band view is derived on the fly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqUserProfile {
    pub name: String,
    pub bands_18: Vec<i32>,
}

impl From<&EqUserProfile> for EqPresetInfo {
    fn from(profile: &EqUserProfile) -> Self {
        // Derive the 10-band view from the stored 18-band curve so the profile
        // renders correctly in either resolution.
        let bands_10 = <[i32; 18]>::try_from(profile.bands_18.as_slice())
            .map(|b| extract_10_from_18(&b).to_vec())
            .unwrap_or_else(|_| vec![0i32; 10]);
        Self {
            name: profile.name.clone(),
            display_name: profile.name.clone(),
            bands_10,
            bands_18: profile.bands_18.clone(),
            builtin: false,
        }
    }
}

/// Load the user's saved EQ profiles (empty if none/unset).
fn load_user_profiles(db: &Database) -> CommandResult<Vec<EqUserProfile>> {
    Ok(db
        .get_setting_json::<Vec<EqUserProfile>>(keys::EQ_USER_PROFILES)?
        .unwrap_or_default())
}

/// The full selectable preset list: shipped presets followed by user profiles.
fn all_presets(db: &Database) -> CommandResult<Vec<EqPresetInfo>> {
    let mut presets: Vec<EqPresetInfo> = EqPreset::all().iter().map(EqPresetInfo::from).collect();
    presets.extend(load_user_profiles(db)?.iter().map(EqPresetInfo::from));
    Ok(presets)
}

/// Equalizer state for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqualizerStatus {
    pub enabled: bool,
    pub current_preset: String,
    pub bands: Vec<i32>,
}

/// Get all available equalizer presets (shipped presets plus user profiles).
#[tauri::command]
pub async fn get_eq_presets(state: tauri::State<'_, AppState>) -> CommandResult<Vec<EqPresetInfo>> {
    let presets = all_presets(&state.db)?;
    tracing::debug!("Returning {} EQ presets", presets.len());
    Ok(presets)
}

/// Get the current equalizer status from persisted settings.
#[tauri::command]
pub async fn get_eq_status(state: tauri::State<'_, AppState>) -> CommandResult<EqualizerStatus> {
    let db = &state.db;

    // Load persisted EQ settings
    let enabled = db
        .get_setting(keys::EQ_ENABLED)?
        .map(|v| v == "true")
        .unwrap_or(false);

    let current_preset = db
        .get_setting(keys::EQ_PRESET)?
        .unwrap_or_else(|| "flat".to_string());

    // Load custom bands if present, otherwise use flat
    let bands: Vec<i32> = db
        .get_setting_json::<Vec<i32>>(keys::EQ_CUSTOM_BANDS)?
        .unwrap_or_else(|| vec![0i32; 18]);

    Ok(EqualizerStatus {
        enabled,
        current_preset,
        bands,
    })
}

/// Apply the user's persisted EQ settings to a freshly-started audio engine.
///
/// Called once on startup, right after the engine handle becomes available.
/// Reads the same DB keys as [`get_eq_status`] and replays them through the same
/// `SetEq*` command path the settings UI uses, so a saved EQ takes effect on the
/// first track played instead of only after the user opens Settings > Playback.
///
/// Robustness: if the EQ is disabled or unset, this does nothing — the engine
/// already defaults to flat/off, so there is no filter to apply.
pub fn apply_persisted_eq(db: &Database, audio: &AudioEngineHandle) {
    // A disabled (or never-configured) EQ means flat: leave the engine alone.
    let enabled = match db.get_setting(keys::EQ_ENABLED) {
        Ok(value) => value.map(|v| v == "true").unwrap_or(false),
        Err(e) => {
            tracing::warn!("Failed to read persisted EQ enabled flag: {}", e);
            return;
        }
    };
    if !enabled {
        tracing::debug!("No persisted EQ to apply on startup (disabled or unset)");
        return;
    }

    let preset = db
        .get_setting(keys::EQ_PRESET)
        .ok()
        .flatten()
        .unwrap_or_else(|| "flat".to_string());

    // Push the bands/preset first so the filter string is populated, then flip
    // the enable flag — SetEqEnabled(true) triggers the actual `af` apply. A
    // built-in named preset is applied by name; a custom curve OR a user-saved
    // profile (neither is a built-in enum variant) replays its stored bands.
    let is_builtin_named = EqPreset::from_str(&preset).is_some_and(|p| p != EqPreset::Custom);
    if is_builtin_named {
        if let Err(e) = audio.send(AudioCommand::SetEqPreset(preset.clone())) {
            tracing::warn!("Failed to apply persisted EQ preset '{}': {}", preset, e);
            return;
        }
    } else {
        match db.get_setting_json::<Vec<i32>>(keys::EQ_CUSTOM_BANDS) {
            Ok(Some(bands)) => {
                if let Err(e) = audio.send(AudioCommand::SetEqBands(bands)) {
                    tracing::warn!("Failed to apply persisted custom EQ bands: {}", e);
                    return;
                }
            }
            Ok(None) => {
                tracing::warn!(
                    "EQ preset '{}' has no persisted bands; leaving flat",
                    preset
                );
            }
            Err(e) => {
                tracing::warn!("Failed to read persisted custom EQ bands: {}", e);
                return;
            }
        }
    }

    if let Err(e) = audio.send(AudioCommand::SetEqEnabled(true)) {
        tracing::warn!("Failed to enable persisted EQ on startup: {}", e);
        return;
    }

    tracing::info!(
        "Applied persisted EQ settings on startup (preset: {})",
        preset
    );
}

/// Enable or disable the equalizer.
/// Persists the setting to the database.
#[tauri::command]
pub async fn set_eq_enabled(state: tauri::State<'_, AppState>, enabled: bool) -> CommandResult<()> {
    tracing::info!("Setting EQ enabled: {}", enabled);

    // Persist to database
    state
        .db
        .set_setting(keys::EQ_ENABLED, if enabled { "true" } else { "false" })?;

    // Apply to audio engine
    state
        .get_audio()
        .await?
        .send(crate::audio::AudioCommand::SetEqEnabled(enabled))
        .map_err(CommandError::msg)
}

/// Set the equalizer preset.
/// Persists the preset name to the database.
#[tauri::command]
pub async fn set_eq_preset(
    state: tauri::State<'_, AppState>,
    preset_name: String,
) -> CommandResult<()> {
    tracing::info!("Setting EQ preset: {}", preset_name);

    // A user-saved profile isn't a built-in enum variant: replay its stored
    // bands and remember it by name, mirroring how a custom curve is persisted.
    if EqPreset::from_str(&preset_name).is_none() {
        let profiles = load_user_profiles(&state.db)?;
        let profile = profiles
            .iter()
            .find(|p| p.name == preset_name)
            .ok_or_else(|| format!("Invalid preset name: {}", preset_name))?;

        state
            .db
            .set_setting_json(keys::EQ_CUSTOM_BANDS, &profile.bands_18)?;
        state.db.set_setting(keys::EQ_PRESET, &preset_name)?;

        return state
            .get_audio()
            .await?
            .send(AudioCommand::SetEqBands(profile.bands_18.clone()))
            .map_err(CommandError::msg);
    }

    // Persist to database
    state.db.set_setting(keys::EQ_PRESET, &preset_name)?;

    // If it's a built-in preset, clear custom bands
    if EqPreset::from_str(&preset_name) != Some(EqPreset::Custom) {
        state.db.delete_setting(keys::EQ_CUSTOM_BANDS)?;
    }

    // Apply to audio engine
    state
        .get_audio()
        .await?
        .send(AudioCommand::SetEqPreset(preset_name))
        .map_err(CommandError::msg)
}

/// Set custom equalizer band values.
/// Persists the bands to the database and sets preset to "custom".
///
/// Takes an array of 10 or 18 gain values in dB. Each value should be in the range
/// -20 to +20 dB (practical limits).
/// - 10 bands: mapped to ISO octave frequencies (31, 63, 125, 250, 500, 1k, 2k, 4k, 8k, 16k Hz)
/// - 18 bands: mapped to superequalizer frequencies
#[tauri::command]
pub async fn set_eq_bands(state: tauri::State<'_, AppState>, bands: Vec<i32>) -> CommandResult<()> {
    if bands.len() != 10 && bands.len() != 18 {
        return Err(format!("Expected 10 or 18 bands, got {}", bands.len()).into());
    }

    tracing::info!("Setting custom EQ bands: {:?}", bands);

    // Convert 10-band to 18-band for storage if needed
    let bands_18 = if bands.len() == 10 {
        let bands_10: [i32; 10] = bands.try_into().map_err(|_| "Failed to convert bands")?;
        map_10_to_18_bands(&bands_10).to_vec()
    } else {
        bands
    };

    // Persist to database - always store as 18 bands
    state
        .db
        .set_setting_json(keys::EQ_CUSTOM_BANDS, &bands_18)?;
    state.db.set_setting(keys::EQ_PRESET, "custom")?;

    // Apply to audio engine
    state
        .get_audio()
        .await?
        .send(crate::audio::AudioCommand::SetEqBands(bands_18))
        .map_err(CommandError::msg)
}

/// Save the given bands as a named user profile and make it the active preset.
///
/// Bands may be 10 or 18 values (10 are mapped up to 18 for storage). Saving
/// with an existing name (case-insensitively) overwrites that profile. Built-in
/// preset names are reserved. Returns the refreshed preset list so the UI can
/// update its dropdown without a second round-trip.
#[tauri::command]
pub async fn save_eq_profile(
    state: tauri::State<'_, AppState>,
    name: String,
    bands: Vec<i32>,
) -> CommandResult<Vec<EqPresetInfo>> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Profile name cannot be empty".into());
    }
    // Reserve built-in names (and "custom") so a profile can never shadow one.
    if EqPreset::from_str(&name).is_some() {
        return Err(format!(
            "'{}' is a built-in preset name; choose a different name",
            name
        )
        .into());
    }
    if bands.len() != 10 && bands.len() != 18 {
        return Err(format!("Expected 10 or 18 bands, got {}", bands.len()).into());
    }

    // Store the canonical 18-band form.
    let bands_18 = if bands.len() == 10 {
        let bands_10: [i32; 10] = bands.try_into().map_err(|_| "Failed to convert bands")?;
        map_10_to_18_bands(&bands_10).to_vec()
    } else {
        bands
    };

    tracing::info!("Saving EQ profile '{}'", name);

    let mut profiles = load_user_profiles(&state.db)?;
    match profiles
        .iter_mut()
        .find(|p| p.name.eq_ignore_ascii_case(&name))
    {
        Some(existing) => {
            existing.name = name.clone(); // adopt the latest casing
            existing.bands_18 = bands_18.clone();
        }
        None => profiles.push(EqUserProfile {
            name: name.clone(),
            bands_18: bands_18.clone(),
        }),
    }

    state
        .db
        .set_setting_json(keys::EQ_USER_PROFILES, &profiles)?;

    // Make the saved profile the active preset.
    state
        .db
        .set_setting_json(keys::EQ_CUSTOM_BANDS, &bands_18)?;
    state.db.set_setting(keys::EQ_PRESET, &name)?;
    state
        .get_audio()
        .await?
        .send(AudioCommand::SetEqBands(bands_18))?;

    all_presets(&state.db)
}

/// Delete a user profile by name. If it was the active preset, fall back to
/// flat. Returns the refreshed preset list.
#[tauri::command]
pub async fn delete_eq_profile(
    state: tauri::State<'_, AppState>,
    name: String,
) -> CommandResult<Vec<EqPresetInfo>> {
    tracing::info!("Deleting EQ profile '{}'", name);

    let mut profiles = load_user_profiles(&state.db)?;
    profiles.retain(|p| !p.name.eq_ignore_ascii_case(&name));
    state
        .db
        .set_setting_json(keys::EQ_USER_PROFILES, &profiles)?;

    // If the deleted profile was active, reset to flat so nothing dangles.
    let active = state.db.get_setting(keys::EQ_PRESET)?.unwrap_or_default();
    if active.eq_ignore_ascii_case(&name) {
        state.db.set_setting(keys::EQ_PRESET, "flat")?;
        state.db.delete_setting(keys::EQ_CUSTOM_BANDS)?;
        state
            .get_audio()
            .await?
            .send(AudioCommand::SetEqPreset("flat".to_string()))?;
    }

    all_presets(&state.db)
}

/// Get information about a specific preset.
#[tauri::command]
pub async fn get_eq_preset_info(preset_name: String) -> CommandResult<EqPresetInfo> {
    let preset = EqPreset::from_str(&preset_name)
        .ok_or_else(|| format!("Invalid preset name: {}", preset_name))?;

    Ok(EqPresetInfo::from(&preset))
}

/// Reset the equalizer to flat (no adjustment).
/// Persists the reset to the database.
#[tauri::command]
pub async fn reset_eq(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    tracing::info!("Resetting EQ to flat");

    // Persist to database
    state.db.set_setting(keys::EQ_PRESET, "flat")?;
    state.db.delete_setting(keys::EQ_CUSTOM_BANDS)?;

    // Apply to audio engine
    state
        .get_audio()
        .await?
        .send(crate::audio::AudioCommand::SetEqPreset("flat".to_string()))
        .map_err(CommandError::msg)
}
