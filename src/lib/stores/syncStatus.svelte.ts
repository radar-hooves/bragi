/**
 * Library sync progress — one shared model (sync/offline/downloads).
 *
 * Previously "is a sync running" lived as three independent local booleans:
 * the toolbar sync button, the Settings "Sync Now" button, and the library
 * store's first-load `isSyncing` flag — none aware of the others. A sync
 * triggered from Settings left the toolbar icon looking idle, and a
 * background scheduled sync had NO frontend representation at all: it
 * updated the SQLite cache but the in-memory library state (and every UI
 * reading it) never refreshed until the user happened to trigger their own
 * manual sync afterwards.
 *
 * This store is driven entirely by the backend's `library-sync-started` /
 * `-completed` / `-failed` events, emitted from the one path every trigger
 * (manual command, scheduler) funnels through — so every trigger site reads
 * the same `getIsSyncing()`, and a completed sync (whoever started it)
 * refreshes the library store here, once.
 */

import { getCachedAlbums, getCachedArtists, getLibraryTracks, getSettings } from '#lib/services/tauri.js';
import { setAlbums, setArtists, setTracks } from '#lib/stores/library.svelte.js';
import { getActiveBackend } from '#lib/stores/jellyfin.svelte.js';
import { loadJellyfinPlaylists } from '#lib/stores/jellyfinPlaylist.svelte.js';
import { SvelteSet } from 'svelte/reactivity';
import type { SyncMetrics } from '#lib/types/tauri.js';

interface SyncStatusState {
  /** Backends with a sync currently in flight. */
  syncingBackends: SvelteSet<string>;
  lastResult: 'success' | 'error' | 'skipped' | null;
  lastMetrics: SyncMetrics | null;
  lastError: string | null;
  /** Unix seconds, refreshed after a successful, non-skipped sync. */
  lastSyncedAt: number | null;
}

let state = $state<SyncStatusState>({
  syncingBackends: new SvelteSet(),
  lastResult: null,
  lastMetrics: null,
  lastError: null,
  lastSyncedAt: null,
});

/** Whether ANY backend has a sync in flight right now. */
export function getIsSyncing(): boolean {
  return state.syncingBackends.size > 0;
}

export function getLastSyncResult(): 'success' | 'error' | 'skipped' | null {
  return state.lastResult;
}

export function getLastSyncMetrics(): SyncMetrics | null {
  return state.lastMetrics;
}

export function getLastSyncError(): string | null {
  return state.lastError;
}

export function getLastSyncedAt(): number | null {
  return state.lastSyncedAt;
}

export function handleSyncStarted(backend: string): void {
  state.syncingBackends.add(backend);
}

/**
 * A completed sync (manual or scheduled — the event does not distinguish)
 * refreshes the library store from the now-current cache, so a scheduled
 * sync's result actually reaches the UI instead of sitting unseen in SQLite
 * until the user's next manual trigger. A coalesced no-op (`metrics.skipped`)
 * changes nothing and refreshes nothing.
 */
export async function handleSyncCompleted(backend: string, metrics: SyncMetrics): Promise<void> {
  state.syncingBackends.delete(backend);
  state.lastMetrics = metrics;
  state.lastError = null;
  state.lastResult = metrics.skipped ? 'skipped' : 'success';

  if (metrics.skipped) return;

  try {
    const [albums, artists, tracks, settings] = await Promise.all([
      getCachedAlbums(),
      getCachedArtists(),
      getLibraryTracks(),
      getSettings(),
      backend === getActiveBackend() ? loadJellyfinPlaylists() : Promise.resolve(),
    ]);
    setAlbums(albums);
    setArtists(artists);
    setTracks(tracks);
    state.lastSyncedAt = settings.lastLibrarySync;
  } catch (e) {
    console.error('Failed to refresh library after sync:', e);
  }
}

export function handleSyncFailed(backend: string, error: string): void {
  state.syncingBackends.delete(backend);
  state.lastResult = 'error';
  state.lastError = error;
}
