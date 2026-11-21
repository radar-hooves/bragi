// The queue store is a pure reflection of the backend engine's own queue —
// the single mechanism that builds and advances it. `setQueueState` (called
// from the `queue-changed` event handler) is the only writer; there is no
// local mutator a UI handler could call instead and drift from the engine.
// These tests pin that shape, plus the repeat-all wraparound (Next/Previous
// must stay enabled at the queue boundary when repeat is on, since the engine
// wraps rather than stopping there), and the Up Next panel's grouping over
// `getUpcomingQueueEntries`.
import { describe, it, expect, beforeEach, vi } from 'vitest';
import type { QueueEntrySource, Track } from '#lib/types/models.js';

vi.mock('#lib/services/tauri.js', () => ({
  getSettings: vi.fn().mockResolvedValue({ shuffle: false, repeatMode: 'off' }),
  updateSettings: vi.fn().mockResolvedValue(undefined),
  setShuffle: vi.fn(),
  setRepeat: vi.fn(),
}));

const {
  getQueueState,
  getCurrentQueueTrack,
  getUpcomingTracks,
  getUpcomingQueueEntries,
  getPreviousTracks,
  getHasNext,
  getHasPrevious,
  getQueueLength,
  setQueueState,
} = await import('./queue.svelte');

const { updatePlaybackState } = await import('./player.svelte');

function track(id: string): Track {
  return {
    jellyfinId: id,
    mbid: null,
    artistMbid: null,
    releaseMbid: null,
    name: id,
    artistName: 'Artist',
    albumName: 'Album',
    albumJellyfinId: 'album-1',
    trackNumber: 1,
    discNumber: 1,
    durationMs: 180_000,
    genre: null,
    year: null,
    artworkUrl: null,
    cachedFilePath: null,
    fileSizeBytes: null,
    normalizationGainDb: null,
  } as unknown as Track;
}

/** All-`context` sources for a track list — the common case (a played list, no manual queueing). */
function contextSources(tracks: Track[]): QueueEntrySource[] {
  return tracks.map(() => 'context');
}

describe('queue store', () => {
  beforeEach(() => {
    setQueueState({ tracks: [], currentIndex: -1, sources: [] });
    updatePlaybackState({ repeat: 'off' });
  });

  it('exposes no local mutator — the backend is the only writer', async () => {
    const mod = await import('./queue.svelte');
    for (const absent of [
      'setQueue',
      'addToQueue',
      'removeFromQueue',
      'clearQueue',
      'skipToIndex',
      'skipNext',
      'skipPrevious',
    ]) {
      expect(mod).not.toHaveProperty(absent);
    }
  });

  it('reflects exactly what setQueueState (the queue-changed handler) sets', () => {
    const tracks = [track('a'), track('b'), track('c')];
    const sources = contextSources(tracks);
    setQueueState({ tracks, currentIndex: 1, sources });

    expect(getQueueState()).toEqual({ tracks, currentIndex: 1, sources });
    expect(getCurrentQueueTrack()?.jellyfinId).toBe('b');
    expect(getUpcomingTracks().map((t) => t.jellyfinId)).toEqual(['c']);
    expect(getPreviousTracks().map((t) => t.jellyfinId)).toEqual(['a']);
    expect(getQueueLength()).toBe(3);
  });

  it('getCurrentQueueTrack is null on an empty or unset queue', () => {
    setQueueState({ tracks: [], currentIndex: -1, sources: [] });
    expect(getCurrentQueueTrack()).toBeNull();
  });

  describe('getHasNext / getHasPrevious', () => {
    it('is false on an empty queue', () => {
      setQueueState({ tracks: [], currentIndex: -1, sources: [] });
      expect(getHasNext()).toBe(false);
      expect(getHasPrevious()).toBe(false);
    });

    it('bounds normally with repeat off', () => {
      const tracks = [track('a'), track('b'), track('c')];
      setQueueState({ tracks, currentIndex: 0, sources: contextSources(tracks) });
      expect(getHasPrevious()).toBe(false);
      expect(getHasNext()).toBe(true);

      setQueueState({ tracks, currentIndex: 2, sources: contextSources(tracks) });
      expect(getHasPrevious()).toBe(true);
      expect(getHasNext()).toBe(false);
    });

    it('stays enabled at both boundaries under repeat-all (the engine wraps)', () => {
      updatePlaybackState({ repeat: 'all' });
      const tracks = [track('a'), track('b'), track('c')];
      setQueueState({ tracks, currentIndex: 2, sources: contextSources(tracks) });
      expect(getHasNext()).toBe(true);

      setQueueState({ tracks, currentIndex: 0, sources: contextSources(tracks) });
      expect(getHasPrevious()).toBe(true);
    });

    it('repeat-one does not change the boundary bounds (matches Apple Music: manual skip ignores repeat-one)', () => {
      updatePlaybackState({ repeat: 'one' });
      const tracks = [track('a'), track('b')];
      setQueueState({ tracks, currentIndex: 1, sources: contextSources(tracks) });
      expect(getHasNext()).toBe(false);
    });

    it('repeat-all with a single track still reports both as reachable (wraps to itself)', () => {
      updatePlaybackState({ repeat: 'all' });
      const tracks = [track('a')];
      setQueueState({ tracks, currentIndex: 0, sources: contextSources(tracks) });
      expect(getHasNext()).toBe(true);
      expect(getHasPrevious()).toBe(true);
    });
  });

  describe('getUpcomingQueueEntries', () => {
    it('is empty on an empty or unset queue', () => {
      setQueueState({ tracks: [], currentIndex: -1, sources: [] });
      expect(getUpcomingQueueEntries()).toEqual([]);
    });

    it('is empty when the current track is the last one', () => {
      const tracks = [track('a'), track('b')];
      setQueueState({ tracks, currentIndex: 1, sources: contextSources(tracks) });
      expect(getUpcomingQueueEntries()).toEqual([]);
    });

    it('pairs each upcoming track with its own source and absolute queue index', () => {
      const tracks = [track('a'), track('b'), track('c'), track('d')];
      setQueueState({
        tracks,
        currentIndex: 0,
        sources: ['context', 'userQueued', 'userQueued', 'context'],
      });

      expect(getUpcomingQueueEntries()).toEqual([
        { track: track('b'), source: 'userQueued', index: 1 },
        { track: track('c'), source: 'userQueued', index: 2 },
        { track: track('d'), source: 'context', index: 3 },
      ]);
    });

    it('groups Playing Next (user-queued) ahead of the context as contiguous runs, in queue order', () => {
      // A Play Next click inserts right after the current track — a Playing
      // Next run at the front, then the rest of the context.
      const tracks = [track('now'), track('next-1'), track('next-2'), track('ctx-1'), track('ctx-2')];
      setQueueState({
        tracks,
        currentIndex: 0,
        sources: ['context', 'userQueued', 'userQueued', 'context', 'context'],
      });

      const entries = getUpcomingQueueEntries();
      const playingNext = entries.filter((e) => e.source === 'userQueued');
      const context = entries.filter((e) => e.source === 'context');

      expect(playingNext.map((e) => e.track.jellyfinId)).toEqual(['next-1', 'next-2']);
      expect(context.map((e) => e.track.jellyfinId)).toEqual(['ctx-1', 'ctx-2']);
      // The engine's own order, not a re-sort: Playing Next entries precede
      // every context entry in the returned list.
      expect(entries.map((e) => e.track.jellyfinId)).toEqual(['next-1', 'next-2', 'ctx-1', 'ctx-2']);
    });

    it('follows setQueueState (queue-changed) rather than holding its own copy', () => {
      const first = [track('a'), track('b')];
      setQueueState({ tracks: first, currentIndex: 0, sources: contextSources(first) });
      expect(getUpcomingQueueEntries().map((e) => e.track.jellyfinId)).toEqual(['b']);

      const second = [track('a'), track('x'), track('y')];
      setQueueState({ tracks: second, currentIndex: 0, sources: ['context', 'userQueued', 'context'] });
      expect(getUpcomingQueueEntries()).toEqual([
        { track: track('x'), source: 'userQueued', index: 1 },
        { track: track('y'), source: 'context', index: 2 },
      ]);
    });
  });
});
