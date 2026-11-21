// The one library search entry point: cache vs. live
// selection, the empty-cache live fallback, and the offline degrade all
// collapse into `performLibrarySearch` so no caller branches on them.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { Track } from '#lib/types/models.js';

const searchLibrary = vi.fn();
const searchLibraryLive = vi.fn();
const getOfflineMode = vi.fn();

vi.mock('#lib/services/tauri.js', () => ({
  searchLibrary: (...args: unknown[]) => searchLibrary(...args),
  searchLibraryLive: (...args: unknown[]) => searchLibraryLive(...args),
}));

vi.mock('#lib/stores/offlineMode.svelte.js', () => ({
  getOfflineMode: (...args: unknown[]) => getOfflineMode(...args),
}));

const { performLibrarySearch } = await import('./search');

const EMPTY = { tracks: [], albums: [], artists: [] };
function withOneTrack(name: string) {
  return { tracks: [{ jellyfinId: name, name, artistName: 'Someone' } as unknown as Track], albums: [], artists: [] };
}

describe('performLibrarySearch', () => {
  beforeEach(() => {
    searchLibrary.mockReset();
    searchLibraryLive.mockReset();
    getOfflineMode.mockReset().mockReturnValue(false);
  });

  it('returns an empty result for a blank query without calling either backend', async () => {
    const r = await performLibrarySearch('   ', { isSyncing: false });
    expect(r).toEqual({ tracks: [], albums: [], artists: [], topResult: null });
    expect(searchLibrary).not.toHaveBeenCalled();
    expect(searchLibraryLive).not.toHaveBeenCalled();
  });

  it('searches the cache when not syncing and the cache has a match', async () => {
    searchLibrary.mockResolvedValue(withOneTrack('Rock'));
    const r = await performLibrarySearch('rock', { isSyncing: false });
    expect(searchLibrary).toHaveBeenCalledWith('rock');
    expect(searchLibraryLive).not.toHaveBeenCalled();
    expect(r.tracks.map((t) => t.name)).toEqual(['Rock']);
  });

  it('goes straight to a live search while the backend is still syncing', async () => {
    searchLibraryLive.mockResolvedValue(withOneTrack('Rock'));
    await performLibrarySearch('rock', { isSyncing: true });
    expect(searchLibrary).not.toHaveBeenCalled();
    expect(searchLibraryLive).toHaveBeenCalledWith('rock');
  });

  it('answers from the cache when the live search fails mid-sync', async () => {
    searchLibraryLive.mockRejectedValue(new Error('server dropped mid-sync'));
    searchLibrary.mockResolvedValue(withOneTrack('Rock'));
    const r = await performLibrarySearch('rock', { isSyncing: true });
    expect(r.tracks.map((t) => t.name)).toEqual(['Rock']);
  });

  it('falls back to a live search when the cache comes back fully empty', async () => {
    searchLibrary.mockResolvedValue(EMPTY);
    searchLibraryLive.mockResolvedValue(withOneTrack('Rock'));
    const r = await performLibrarySearch('rock', { isSyncing: false });
    expect(searchLibrary).toHaveBeenCalledWith('rock');
    expect(searchLibraryLive).toHaveBeenCalledWith('rock');
    expect(r.tracks.map((t) => t.name)).toEqual(['Rock']);
  });

  it('never attempts a live search while offline, even on an empty cache', async () => {
    getOfflineMode.mockReturnValue(true);
    searchLibrary.mockResolvedValue(EMPTY);
    const r = await performLibrarySearch('rock', { isSyncing: false });
    expect(searchLibraryLive).not.toHaveBeenCalled();
    expect(r).toEqual({ tracks: [], albums: [], artists: [], topResult: null });
  });

  it('keeps the (empty) cache result when the live fallback itself fails, rather than surfacing an error', async () => {
    searchLibrary.mockResolvedValue(EMPTY);
    searchLibraryLive.mockRejectedValue(new Error('offline mid-search'));
    const r = await performLibrarySearch('rock', { isSyncing: false });
    expect(r).toEqual({ tracks: [], albums: [], artists: [], topResult: null });
  });

  it("doesn't retry live when already syncing, even if the live result is itself empty", async () => {
    searchLibraryLive.mockResolvedValue(EMPTY);
    await performLibrarySearch('rock', { isSyncing: true });
    expect(searchLibraryLive).toHaveBeenCalledTimes(1);
    expect(searchLibrary).not.toHaveBeenCalled();
  });
});
