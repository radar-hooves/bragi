// Sync/offline/downloads polish: the one shared sync-progress model every trigger (toolbar,
// Settings, the background scheduler) now updates through, driven entirely
// by the backend's library-sync-* events.
import { describe, it, expect, beforeEach, vi } from 'vitest';
import type { SyncMetrics } from '#lib/types/tauri.js';

const getCachedAlbums = vi.fn();
const getCachedArtists = vi.fn();
const getLibraryTracks = vi.fn();
const getSettings = vi.fn();
const getJellyfinPlaylists = vi.fn();
vi.mock('./jellyfin.svelte', () => ({ getActiveBackend: () => 'jellyfin' }));

vi.mock('#lib/services/tauri.js', () => ({
  getCachedAlbums: (...args: unknown[]) => getCachedAlbums(...args),
  getCachedArtists: (...args: unknown[]) => getCachedArtists(...args),
  getLibraryTracks: (...args: unknown[]) => getLibraryTracks(...args),
  getSettings: (...args: unknown[]) => getSettings(...args),
  getJellyfinPlaylists: (...args: unknown[]) => getJellyfinPlaylists(...args),
}));

const {
  getIsSyncing,
  getLastSyncResult,
  getLastSyncMetrics,
  getLastSyncError,
  getLastSyncedAt,
  handleSyncStarted,
  handleSyncCompleted,
  handleSyncFailed,
} = await import('./syncStatus.svelte');
const { getLibraryState } = await import('./library.svelte');
const { getJellyfinPlaylistState } = await import('./jellyfinPlaylist.svelte');

function metrics(overrides: Partial<SyncMetrics> = {}): SyncMetrics {
  return {
    durationMs: 1000,
    fetchTimeMs: 600,
    dbTimeMs: 400,
    artistCount: 1,
    albumCount: 1,
    trackCount: 10,
    apiRequests: 5,
    staleArtists: 0,
    staleAlbums: 0,
    staleTracks: 0,
    failedFetches: 0,
    skipped: false,
    ...overrides,
  };
}

// The store is a module-level singleton (like every other .svelte.ts store in
// this codebase), so each test fully finishes whatever backend it starts
// rather than relying on a reset between tests.
describe('syncStatus store', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getCachedAlbums.mockResolvedValue([]);
    getCachedArtists.mockResolvedValue([]);
    getLibraryTracks.mockResolvedValue([]);
    getSettings.mockResolvedValue({ lastLibrarySync: 12345 });
    getJellyfinPlaylists.mockResolvedValue([]);
  });

  it('is syncing once started, and stops once completed', async () => {
    handleSyncStarted('jellyfin');
    expect(getIsSyncing()).toBe(true);

    await handleSyncCompleted('jellyfin', metrics());
    expect(getIsSyncing()).toBe(false);
    expect(getLastSyncResult()).toBe('success');
  });

  it('tracks multiple backends independently: one finishing does not clear the other', async () => {
    handleSyncStarted('jellyfin');
    handleSyncStarted('opensubsonic');
    expect(getIsSyncing()).toBe(true);

    await handleSyncCompleted('jellyfin', metrics());
    expect(getIsSyncing()).toBe(true); // opensubsonic is still running

    await handleSyncCompleted('opensubsonic', metrics());
    expect(getIsSyncing()).toBe(false);
  });

  it('refreshes the library store from the cache on a real completion — the fix for a scheduled sync never reaching the UI', async () => {
    const track = {
      jellyfinId: 't1',
      name: 'Song',
      artistName: 'Artist',
      albumName: 'Album',
      albumJellyfinId: 'album-1',
      trackNumber: 1,
      discNumber: 1,
      durationMs: 200_000,
    };
    getLibraryTracks.mockResolvedValue([track]);

    handleSyncStarted('jellyfin');
    await handleSyncCompleted('jellyfin', metrics());

    expect(getCachedAlbums).toHaveBeenCalled();
    expect(getCachedArtists).toHaveBeenCalled();
    expect(getLibraryTracks).toHaveBeenCalled();
    expect(getLibraryState().tracks).toEqual([track]);
    expect(getLastSyncedAt()).toBe(12345);
  });

  it('refreshes the visible playlist list after syncing without restarting', async () => {
    const playlist = { jellyfinId: 'concert', name: 'Iron Maiden concert', trackCount: 3 };
    getJellyfinPlaylists.mockResolvedValue([playlist]);
    await handleSyncCompleted('jellyfin', metrics());
    expect(getJellyfinPlaylistState().playlists).toEqual([playlist]);
    expect(getJellyfinPlaylists).toHaveBeenCalledOnce();
  });

  it('does not refresh playlists when the inactive backend syncs', async () => {
    await handleSyncCompleted('opensubsonic', metrics());
    expect(getJellyfinPlaylists).not.toHaveBeenCalled();
  });

  it('a coalesced (skipped) completion never refreshes the library', async () => {
    handleSyncStarted('jellyfin');
    await handleSyncCompleted('jellyfin', metrics({ skipped: true }));

    expect(getIsSyncing()).toBe(false);
    expect(getLastSyncResult()).toBe('skipped');
    expect(getCachedAlbums).not.toHaveBeenCalled();
  });

  it('a failure clears syncing, records the error, and leaves the last metrics untouched', async () => {
    // Establish a known "last successful metrics" first.
    handleSyncStarted('jellyfin');
    await handleSyncCompleted('jellyfin', metrics({ trackCount: 42 }));
    const metricsBeforeFailure = getLastSyncMetrics();

    handleSyncStarted('jellyfin');
    handleSyncFailed('jellyfin', 'Network error: timed out');

    expect(getIsSyncing()).toBe(false);
    expect(getLastSyncResult()).toBe('error');
    expect(getLastSyncError()).toBe('Network error: timed out');
    expect(getLastSyncMetrics()).toBe(metricsBeforeFailure);
  });
});
