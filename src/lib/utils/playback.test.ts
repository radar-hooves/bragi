// startPlayback is the one mechanism every "play this row, queue the rest of
// the list" entry point goes through (Songs, an album, an artist, a playlist,
// search, the column browser, recently played/added — see $lib/utils/
// columnBrowserPlayback and the callers in +page.svelte). It builds and plays
// the whole context in a single backend round trip; these tests pin that it
// is exactly one call, not the old play-then-append two-step dance a
// transport action or a page reload could interrupt mid-way, and that
// shuffle rides along in that same call rather than a follow-up setShuffle
// that would re-permute the queue a second time.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { Track } from '#lib/types/models.js';

const playContext = vi.fn().mockResolvedValue(undefined);
const setRepeat = vi.fn().mockResolvedValue(undefined);
const pause = vi.fn().mockResolvedValue(undefined);
const resume = vi.fn().mockResolvedValue(undefined);

vi.mock('#lib/services/tauri.js', () => ({
  playContext: (...args: unknown[]) => playContext(...args),
  setRepeat: (...args: unknown[]) => setRepeat(...args),
  pause: (...args: unknown[]) => pause(...args),
  resume: (...args: unknown[]) => resume(...args),
  getSettings: vi.fn().mockResolvedValue({ shuffle: false, repeatMode: 'off' }),
  updateSettings: vi.fn().mockResolvedValue(undefined),
}));

const { startPlayback, playTrackFromList, shuffleAndPlay, togglePlayPause } = await import('./playback');
const { getPlaybackState, updatePlaybackState, setCurrentTrack, setPlaying } = await import('#lib/stores/player.svelte.js');

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

describe('startPlayback', () => {
  beforeEach(() => {
    playContext.mockClear();
    setRepeat.mockClear();
    updatePlaybackState({ shuffle: false, repeat: 'off' });
  });

  it('builds and plays the whole context in exactly one backend call, carrying the current shuffle flag', async () => {
    const tracks = [track('a'), track('b'), track('c')];
    await startPlayback(tracks, 1);

    expect(playContext).toHaveBeenCalledTimes(1);
    expect(playContext).toHaveBeenCalledWith(tracks, 1, false);
  });

  it('passes shuffle=true through to playContext instead of a follow-up setShuffle call', async () => {
    updatePlaybackState({ shuffle: true, repeat: 'off' });
    await startPlayback([track('a')], 0);

    expect(playContext).toHaveBeenCalledWith([track('a')], 0, true);
  });

  it('syncs repeat to the engine after the context is built', async () => {
    updatePlaybackState({ shuffle: false, repeat: 'all' });
    await startPlayback([track('a')], 0);

    expect(setRepeat).toHaveBeenCalledWith('all');
    // Order matters: the queue must exist before the engine is told about repeat.
    const playOrder = playContext.mock.invocationCallOrder[0]!;
    const repeatOrder = setRepeat.mock.invocationCallOrder[0]!;
    expect(playOrder).toBeLessThan(repeatOrder);
  });

  it('does nothing for an empty list', async () => {
    await startPlayback([], 0);
    expect(playContext).not.toHaveBeenCalled();
  });

  it('does nothing for an out-of-range start index', async () => {
    await startPlayback([track('a')], 5);
    expect(playContext).not.toHaveBeenCalled();
  });
});

describe('playTrackFromList', () => {
  beforeEach(() => playContext.mockClear());

  it('resolves the clicked track to its index in the list', async () => {
    const tracks = [track('a'), track('b'), track('c')];
    await playTrackFromList(track('b'), tracks);
    expect(playContext).toHaveBeenCalledWith(tracks, 1, false);
  });

  it('falls back to index 0 when the track is not in the list', async () => {
    const tracks = [track('a'), track('b')];
    await playTrackFromList(track('missing'), tracks);
    expect(playContext).toHaveBeenCalledWith(tracks, 0, false);
  });
});

describe('shuffleAndPlay', () => {
  beforeEach(() => playContext.mockClear());

  it('plays a permutation of the same tracks, starting at index 0', async () => {
    const tracks = [track('a'), track('b'), track('c'), track('d')];
    await shuffleAndPlay(tracks);

    expect(playContext).toHaveBeenCalledTimes(1);
    const [shuffled, startIndex] = playContext.mock.calls[0]!;
    expect(startIndex).toBe(0);
    expect((shuffled as Track[]).map((t) => t.jellyfinId).sort()).toEqual(['a', 'b', 'c', 'd']);
  });

  it('does nothing for an empty list', async () => {
    await shuffleAndPlay([]);
    expect(playContext).not.toHaveBeenCalled();
  });
});

describe('togglePlayPause', () => {
  beforeEach(() => {
    pause.mockClear();
    resume.mockClear();
    setCurrentTrack(track('now-playing'));
    setPlaying(false);
  });

  it('does nothing with no track loaded — the Space bar is a no-op, not an error toast', async () => {
    setCurrentTrack(null);
    await togglePlayPause();
    expect(pause).not.toHaveBeenCalled();
    expect(resume).not.toHaveBeenCalled();
  });

  it('resumes when paused, flipping the optimistic isPlaying flag first', async () => {
    setPlaying(false);
    await togglePlayPause();
    expect(resume).toHaveBeenCalledOnce();
    expect(pause).not.toHaveBeenCalled();
    expect(getPlaybackState().isPlaying).toBe(true);
  });

  it('pauses when playing', async () => {
    setPlaying(true);
    await togglePlayPause();
    expect(pause).toHaveBeenCalledOnce();
    expect(getPlaybackState().isPlaying).toBe(false);
  });

  it('rolls back the optimistic flip when the backend call fails', async () => {
    setPlaying(false);
    resume.mockRejectedValueOnce(new Error('engine unreachable'));
    await togglePlayPause();
    expect(getPlaybackState().isPlaying).toBe(false); // rolled back, not stuck on true
  });
});

describe('getPlaybackState sanity', () => {
  it('reflects the shuffle/repeat updates used above', () => {
    updatePlaybackState({ shuffle: true, repeat: 'one' });
    expect(getPlaybackState().shuffle).toBe(true);
    expect(getPlaybackState().repeat).toBe('one');
  });
});
