/**
 * Store for audio output devices.
 *
 * Tracks the list of available audio output devices, updated automatically
 * when devices are connected or disconnected (via Tauri event from CoreAudio).
 */

import type { AudioDevice } from '#lib/types/tauri.js';

/** The list of available audio output devices */
let audioDevicesState = $state<AudioDevice[]>([]);

/** Get the list of audio devices */
export function getAudioDevices(): AudioDevice[] {
  return audioDevicesState;
}

/**
 * Set the list of audio devices.
 * Called by the events service when devices change.
 */
export function setAudioDevices(devices: AudioDevice[]): void {
  audioDevicesState = devices;
}
