// Sync/offline/downloads polish: a cache eviction previously wiped every downloaded badge in
// the library, even when it evicted only one track to make room for a new
// download. handleCacheEvicted now clears only the tracks it is told about.
import { describe, it, expect, beforeEach, vi } from 'vitest';
import type { Track, Album } from '#lib/types/models.js';

// handleCacheEvicted fire-and-forgets a refreshCacheSettings() call; mocked so
// it resolves instead of hitting the (absent, in tests) Tauri IPC bridge.
vi.mock('#lib/services/tauri.js', () => ({
  getCacheSettings: vi.fn().mockResolvedValue({
    maxSizeBytes: 0,
    currentSizeBytes: 0,
    cachedTrackCount: 0,
  }),
}));

const { handleCacheEvicted } = await import('./downloads.svelte');
const { setTracks, setAlbums, getLibraryState } = await import('./library.svelte');

function makeTrack(id: string, albumId: string, downloaded: boolean): Track {
  return {
    jellyfinId: id,
    name: `Track ${id}`,
    artistName: 'Artist',
    albumName: 'Album',
    albumJellyfinId: albumId,
    trackNumber: 1,
    discNumber: 1,
    durationMs: 200_000,
    cachedFilePath: downloaded ? `/cache/${id}.flac` : undefined,
  };
}

function makeAlbum(id: string, downloadedTrackCount: number, trackCount: number): Album {
  return {
    jellyfinId: id,
    name: 'Album',
    artistName: 'Artist',
    artistJellyfinId: 'artist-1',
    trackCount,
    durationMs: 600_000,
    downloadedTrackCount,
  };
}

describe('handleCacheEvicted', () => {
  beforeEach(() => {
    setAlbums([makeAlbum('album-1', 3, 3)]);
    setTracks([
      makeTrack('t1', 'album-1', true),
      makeTrack('t2', 'album-1', true),
      makeTrack('t3', 'album-1', true),
    ]);
  });

  it('clears only the named tracks, leaving the rest downloaded', () => {
    handleCacheEvicted(['t2']);

    const tracks = getLibraryState().tracks;
    expect(tracks.find((t) => t.jellyfinId === 't1')?.cachedFilePath).toBe('/cache/t1.flac');
    expect(tracks.find((t) => t.jellyfinId === 't2')?.cachedFilePath).toBeUndefined();
    expect(tracks.find((t) => t.jellyfinId === 't3')?.cachedFilePath).toBe('/cache/t3.flac');
  });

  it('decrements the album badge by exactly the number of tracks evicted, not to zero', () => {
    handleCacheEvicted(['t2']);

    const album = getLibraryState().albums.find((a) => a.jellyfinId === 'album-1');
    expect(album?.downloadedTrackCount).toBe(2);
  });

  it('is a no-op given an empty list', () => {
    handleCacheEvicted([]);

    const album = getLibraryState().albums.find((a) => a.jellyfinId === 'album-1');
    expect(album?.downloadedTrackCount).toBe(3);
    expect(getLibraryState().tracks.every((t) => t.cachedFilePath)).toBe(true);
  });
});
