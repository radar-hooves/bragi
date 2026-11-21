/**
 * Offline mode store for controlling online/offline state.
 *
 * Two independent signals collapse into one "effective offline" state
 * (sync/offline/downloads), mirroring Spotify: a manual "Offline Mode" toggle the user
 * sets deliberately (`offlineModeState`, persisted), and an automatic
 * "lost the connection" signal the backend detects on its own
 * (`networkReachable`, driven by the `connectivity-changed` event).
 * `getEffectiveOffline()` collapses the two for network-only features
 * (lyrics). Only the user's own toggle (`getOfflineMode()`) hides or greys
 * library content, in lists, search and rows: a server out of reach never
 * empties a browsable album or blocks a search, and cached results stay usable.
 */

import { getSettings, updateSettings } from '#lib/services/tauri.js';

/** Whether the user has manually switched offline mode on. */
let offlineModeState = $state(false);

/**
 * Whether the backend's automatic connectivity probe last found a
 * credentialed backend reachable. Optimistic default (`true`), matching the
 * backend's own default, so a cold start never shows a false "offline" state
 * before the first probe lands.
 */
let networkReachableState = $state(true);

/** Get whether offline mode is enabled */
export function getOfflineMode(): boolean {
  return offlineModeState;
}

/** Get the backend's last-known automatic network reachability. */
export function getNetworkReachable(): boolean {
  return networkReachableState;
}

/**
 * Whether network-only features should stand down right now: the user chose
 * Offline Mode, or the backend can't reach any configured server. Library
 * content never reads this; it reads `getOfflineMode()`.
 */
export function getEffectiveOffline(): boolean {
  return offlineModeState || !networkReachableState;
}

/** Update from the backend's `connectivity-changed` event. */
export function setNetworkReachable(reachable: boolean): void {
  networkReachableState = reachable;
}

/** Initialise offline mode from persisted settings. Call once at app startup. */
export async function initOfflineMode(): Promise<void> {
  try {
    const settings = await getSettings();
    offlineModeState = settings.offlineMode;
  } catch (e) {
    console.error('Failed to load offline mode setting:', e);
  }
}

/** Toggle offline mode on or off and persist to settings. */
export async function setOfflineMode(enabled: boolean): Promise<void> {
  offlineModeState = enabled;
  try {
    await updateSettings({ offlineMode: enabled });
  } catch (e) {
    console.error('Failed to save offline mode setting:', e);
  }
}
