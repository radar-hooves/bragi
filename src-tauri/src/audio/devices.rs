//! Audio device enumeration.
//!
//! Queries mpv's `audio-device-list` property via IPC to get device
//! identifiers that match what `--audio-device=` expects (e.g.
//! `coreaudio/BuiltInSpeakerDevice`).

use std::path::Path;

use super::mpv_ipc::MpvIpc;
use super::types::AudioDevice;

/// Query the running mpv instance for available audio output devices.
///
/// Connects to the mpv IPC socket, reads `audio-device-list`, and returns
/// devices with mpv-compatible names (suitable for `--audio-device=`).
///
/// Filters out the `auto` pseudo-device (exposed as "System Default" in the UI)
/// and `null` (no audio output).
pub fn query_mpv_devices(socket_path: &Path) -> Result<Vec<AudioDevice>, String> {
    let ipc = MpvIpc::connect(&socket_path.to_path_buf())?;

    let device_list = ipc.get_property("audio-device-list")?;

    let arr = device_list
        .as_array()
        .ok_or_else(|| "audio-device-list is not an array".to_string())?;

    tracing::debug!("mpv audio-device-list: {:?}", arr);

    let mut devices = Vec::new();

    for item in arr {
        let Some(name) = item.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let description = item
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or(name);

        // Skip pseudo-devices and non-CoreAudio backends.
        // mpv lists devices for every compiled audio backend (coreaudio, openal,
        // etc.) with the same human-readable descriptions, causing duplicates.
        // On macOS, CoreAudio is the real output path.
        if name == "auto" || name == "null" || !name.starts_with("coreaudio/") {
            continue;
        }

        devices.push(AudioDevice {
            name: name.to_string(),
            description: description.to_string(),
            is_default: false,
        });
    }

    // Deduplicate by description. Multi-output monitors (e.g. HDMI + DisplayPort)
    // appear as separate CoreAudio sub-devices with the same name. Keep the first
    // occurrence, which is typically the active output.
    let mut seen_descriptions = std::collections::HashSet::new();
    devices.retain(|d| seen_descriptions.insert(d.description.clone()));

    // Mark the current default device.
    // mpv's "audio-device" property tells us which device is active;
    // when set to "auto", the first real device in the list is the OS default.
    if let Ok(current) = ipc.get_property("audio-device") {
        let current_name = current.as_str().unwrap_or("auto");
        if current_name == "auto" {
            // First device in the list is the OS default
            if let Some(first) = devices.first_mut() {
                first.is_default = true;
            }
        } else {
            for d in &mut devices {
                if d.name == current_name {
                    d.is_default = true;
                    break;
                }
            }
        }
    }

    // Sort with default device first
    devices.sort_by_key(|d| !d.is_default);

    Ok(devices)
}
