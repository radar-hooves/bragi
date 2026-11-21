import { invoke } from '@tauri-apps/api/core';
import type { SyncStatus, MusicServiceStatus, ScrobbleData, SkipData, ListenHistoryEntry } from '#lib/types/tauri.js';
import { incrementPlayCount, incrementSkipCount } from '#lib/stores/trackMetadata.svelte.js';

/** Form state for music service connection (persists across component lifecycle) */
interface MusicServiceFormState {
  url: string;
  apiToken: string;
  error: string | null;
  testResult: boolean | null;
}

/** Sync status state */
interface ScrobbleState {
  syncStatus: SyncStatus;
  serviceStatus: MusicServiceStatus;
  listeningHistory: ListenHistoryEntry[];
  isLoading: boolean;
  lastError: string | null;
  /** Whether scrobbling is enabled (user setting). */
  scrobbleEnabled: boolean;
  /** Scrobble threshold percentage (25-90, user setting). */
  scrobbleThreshold: number;
  /** Form state for connection UI (persists across tab switches). */
  form: MusicServiceFormState;
}

const initialFormState: MusicServiceFormState = {
  url: '',
  apiToken: '',
  error: null,
  testResult: null,
};

const initialState: ScrobbleState = {
  syncStatus: {
    isConfigured: false,
    isOnline: false,
    pendingCount: 0,
  },
  serviceStatus: {
    connected: false,
  },
  listeningHistory: [],
  isLoading: false,
  lastError: null,
  scrobbleEnabled: true,
  scrobbleThreshold: 50,
  form: { ...initialFormState },
};

let scrobbleState = $state<ScrobbleState>({ ...initialState });

/** Get the scrobble state */
export function getScrobbleState(): ScrobbleState {
  return scrobbleState;
}

/** Whether the music service is configured */
export function getIsServiceConfigured(): boolean {
  return scrobbleState.syncStatus.isConfigured;
}

/** Number of pending scrobbles */
export function getPendingScrobbles(): number {
  return scrobbleState.syncStatus.pendingCount;
}

/** Whether we have pending scrobbles to sync */
export function getHasPendingScrobbles(): boolean {
  return scrobbleState.syncStatus.pendingCount > 0;
}

/** Whether the service is online */
export function getIsServiceOnline(): boolean {
  return scrobbleState.syncStatus.isOnline;
}

/** Whether scrobbling is enabled */
export function getScrobbleEnabled(): boolean {
  return scrobbleState.scrobbleEnabled;
}

/** Scrobble threshold percentage (25-90) */
export function getScrobbleThreshold(): number {
  return scrobbleState.scrobbleThreshold;
}

/** Update scrobble settings (called from PlaybackPage when user changes settings) */
export function setScrobbleSettings(enabled: boolean, threshold: number): void {
  scrobbleState.scrobbleEnabled = enabled;
  scrobbleState.scrobbleThreshold = threshold;
}

// Default threshold: 50% of track or 4 minutes, whichever is less
const DEFAULT_SCROBBLE_THRESHOLD_PERCENT = 0.5;
const SCROBBLE_THRESHOLD_MS = 4 * 60 * 1000; // 4 minutes

/**
 * Check if a track should be scrobbled based on play duration.
 * Uses the user's configured threshold percentage from settings,
 * or 4 minutes, whichever is less.
 */
export function shouldScrobble(playedMs: number, durationMs: number): boolean {
  if (durationMs <= 0) return false;
  if (!getScrobbleEnabled()) return false;

  const thresholdPercent = getScrobbleThreshold() / 100;
  const percentThreshold = durationMs * thresholdPercent;
  const threshold = Math.min(percentThreshold, SCROBBLE_THRESHOLD_MS);

  return playedMs >= threshold;
}

/**
 * Scrobble a track.
 * Sends to music service if available, otherwise queues for later.
 */
export async function scrobbleTrack(data: ScrobbleData): Promise<boolean> {
  try {
    const result = await invoke<boolean>('scrobble_track', { data });
    if (result) incrementPlayCount(data.jellyfinId);
    await refreshSyncStatus();
    return result;
  } catch (e) {
    const error = e instanceof Error ? e.message : String(e);
    console.error('Failed to scrobble track:', error);
    scrobbleState.lastError = error;
    return false;
  }
}

/**
 * Record a skipped track.
 * Sends to music service if available, otherwise queues for later.
 */
export async function skipTrack(data: SkipData): Promise<boolean> {
  try {
    const result = await invoke<boolean>('skip_track', { data });
    if (result) incrementSkipCount(data.jellyfinId);
    await refreshSyncStatus();
    return result;
  } catch (e) {
    const error = e instanceof Error ? e.message : String(e);
    console.error('Failed to record skip:', error);
    scrobbleState.lastError = error;
    return false;
  }
}

/**
 * Sync pending scrobbles to the music service.
 */
export async function syncScrobbles(): Promise<number> {
  scrobbleState.isLoading = true;

  try {
    const synced = await invoke<number>('sync_scrobbles');
    await refreshSyncStatus();
    return synced;
  } catch (e) {
    const error = e instanceof Error ? e.message : String(e);
    console.error('Failed to sync scrobbles:', error);
    scrobbleState.lastError = error;
    throw e;
  } finally {
    scrobbleState.isLoading = false;
  }
}

/**
 * Refresh sync status from backend.
 */
export async function refreshSyncStatus(): Promise<void> {
  try {
    const syncStatus = await invoke<SyncStatus>('get_sync_status');
    scrobbleState.syncStatus = syncStatus;
  } catch (e) {
    console.error('Failed to get sync status:', e);
  }
}

/**
 * Refresh music service status from backend.
 */
export async function refreshServiceStatus(): Promise<void> {
  try {
    const serviceStatus = await invoke<MusicServiceStatus>('get_music_service_status');
    scrobbleState.serviceStatus = serviceStatus;
  } catch (e) {
    console.error('Failed to get service status:', e);
  }
}

/**
 * Load listening history from music service.
 */
export async function loadListeningHistory(limit: number = 50, offset: number = 0): Promise<void> {
  scrobbleState.isLoading = true;

  try {
    const history = await invoke<ListenHistoryEntry[]>('get_listening_history', {
      limit,
      offset,
    });
    scrobbleState.listeningHistory = history;
  } catch (e) {
    const error = e instanceof Error ? e.message : String(e);
    console.error('Failed to load listening history:', error);
    scrobbleState.lastError = error;
  } finally {
    scrobbleState.isLoading = false;
  }
}

/**
 * Connect to music service.
 */
export async function connectMusicService(serviceUrl: string, apiToken: string): Promise<void> {
  scrobbleState.isLoading = true;

  try {
    await invoke('connect_music_service', { serviceUrl, apiToken });
    await refreshServiceStatus();
    await refreshSyncStatus();
  } catch (e) {
    const error = e instanceof Error ? e.message : String(e);
    scrobbleState.lastError = error;
    throw e;
  } finally {
    scrobbleState.isLoading = false;
  }
}

/**
 * Disconnect from music service.
 */
export async function disconnectMusicService(): Promise<void> {
  try {
    await invoke('disconnect_music_service');
    scrobbleState.serviceStatus = { connected: false };
    scrobbleState.syncStatus = {
      isConfigured: false,
      isOnline: false,
      pendingCount: scrobbleState.syncStatus.pendingCount,
    };
  } catch (e) {
    const error = e instanceof Error ? e.message : String(e);
    scrobbleState.lastError = error;
    throw e;
  }
}

/**
 * Test connection to music service.
 */
export async function testMusicServiceConnection(): Promise<boolean> {
  try {
    return await invoke<boolean>('test_music_service_connection');
  } catch (e) {
    console.error('Connection test failed:', e);
    return false;
  }
}

/**
 * Initialise scrobble state from backend.
 */
export async function initScrobbleState(): Promise<void> {
  // Load scrobble settings from SQLite
  try {
    const settings = await invoke<Record<string, unknown>>('get_settings');
    scrobbleState.scrobbleEnabled = (settings.scrobbleEnabled as boolean) ?? true;
    scrobbleState.scrobbleThreshold = (settings.scrobbleThreshold as number) ?? 50;
  } catch {
    // Use defaults if settings can't be loaded
  }
  await Promise.all([refreshServiceStatus(), refreshSyncStatus()]);
}

/**
 * Update music service form state. Partial updates are merged with existing form state.
 */
export function updateMusicServiceForm(updates: Partial<MusicServiceFormState>): void {
  Object.assign(scrobbleState.form, updates);
}

/**
 * Reset music service form state to initial values.
 */
export function resetMusicServiceForm(): void {
  Object.assign(scrobbleState.form, initialFormState);
}
