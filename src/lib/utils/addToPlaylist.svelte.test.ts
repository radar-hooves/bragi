// Playlist polish: the
// one path every add-to-playlist entry point shares for adding to an
// EXISTING playlist, asking first about a duplicate — Apple Music/Spotify
// reference: "Add anyway" / "Skip" rather than silently duplicating or
// silently refusing.
import { describe, it, expect, vi, afterEach } from 'vitest';
import type { JellyfinPlaylist, JellyfinPlaylistDetail } from '#lib/types/models.js';

const addTracksToJellyfinPlaylist = vi.fn().mockResolvedValue(true);
let storeError: string | null = null;
vi.mock('#lib/stores/jellyfinPlaylist.svelte.js', () => ({
  addTracksToJellyfinPlaylist: (...args: unknown[]) => addTracksToJellyfinPlaylist(...args),
  getJellyfinPlaylistState: () => ({ error: storeError }),
}));

const getCachedJellyfinPlaylist = vi.fn();
const getJellyfinPlaylist = vi.fn();
vi.mock('#lib/services/tauri.js', () => ({
  getCachedJellyfinPlaylist: (...args: unknown[]) => getCachedJellyfinPlaylist(...args),
  getJellyfinPlaylist: (...args: unknown[]) => getJellyfinPlaylist(...args),
}));

const { addTracksToPlaylistWithDuplicateCheck, getAddDuplicateDialogState, resolveAddDuplicateDialog } = await import('./addToPlaylist.svelte');

function makeDetail(id: string, trackIds: string[]): JellyfinPlaylistDetail {
  return {
    jellyfinId: id,
    tracks: trackIds.map((tid) => ({
      jellyfinId: tid,
      playlistItemId: `item-${tid}`,
      name: tid,
      artistName: 'Artist',
      albumName: 'Album',
      albumJellyfinId: 'album-1',
      durationMs: 200_000,
    })),
  };
}

const playlist: JellyfinPlaylist = { jellyfinId: 'pl-1', name: 'Road Trip', itemCount: 2 };

describe('addTracksToPlaylistWithDuplicateCheck', () => {
  afterEach(() => {
    vi.clearAllMocks();
    // A left-open dialog from one test must never bleed into the next.
    if (getAddDuplicateDialogState().open) resolveAddDuplicateDialog('cancel');
  });

  it('adds straight through, no dialog, when nothing incoming is already in the playlist', async () => {
    getJellyfinPlaylist.mockResolvedValue(makeDetail('pl-1', ['existing-1']));

    const result = await addTracksToPlaylistWithDuplicateCheck(playlist, ['new-1', 'new-2']);

    expect(result).toBe(true);
    expect(getAddDuplicateDialogState().open).toBe(false);
    expect(addTracksToJellyfinPlaylist).toHaveBeenCalledWith('pl-1', ['new-1', 'new-2']);
  });

  it('asks first when a track is already in the playlist, and "Add Anyway" adds every incoming track', async () => {
    getJellyfinPlaylist.mockResolvedValue(makeDetail('pl-1', ['dup-1']));

    const resultPromise = addTracksToPlaylistWithDuplicateCheck(playlist, ['dup-1', 'new-1']);
    await vi.waitFor(() => expect(getAddDuplicateDialogState().open).toBe(true));
    expect(getAddDuplicateDialogState().duplicateCount).toBe(1);
    expect(getAddDuplicateDialogState().playlistName).toBe('Road Trip');
    expect(addTracksToJellyfinPlaylist).not.toHaveBeenCalled();

    resolveAddDuplicateDialog('add');
    expect(await resultPromise).toBe(true);
    expect(addTracksToJellyfinPlaylist).toHaveBeenCalledWith('pl-1', ['dup-1', 'new-1']);
  });

  it('"Skip" adds only the tracks not already in the playlist', async () => {
    getJellyfinPlaylist.mockResolvedValue(makeDetail('pl-1', ['dup-1']));

    const resultPromise = addTracksToPlaylistWithDuplicateCheck(playlist, ['dup-1', 'new-1']);
    await vi.waitFor(() => expect(getAddDuplicateDialogState().open).toBe(true));
    resolveAddDuplicateDialog('skip');

    expect(await resultPromise).toBe(true);
    expect(addTracksToJellyfinPlaylist).toHaveBeenCalledWith('pl-1', ['new-1']);
  });

  it('"Skip" when EVERY incoming track is a duplicate adds nothing and still reports success', async () => {
    getJellyfinPlaylist.mockResolvedValue(makeDetail('pl-1', ['dup-1', 'dup-2']));

    const resultPromise = addTracksToPlaylistWithDuplicateCheck(playlist, ['dup-1', 'dup-2']);
    await vi.waitFor(() => expect(getAddDuplicateDialogState().open).toBe(true));
    resolveAddDuplicateDialog('skip');

    expect(await resultPromise).toBe(true);
    expect(addTracksToJellyfinPlaylist).not.toHaveBeenCalled();
  });

  it('"Cancel" adds nothing and resolves false', async () => {
    getJellyfinPlaylist.mockResolvedValue(makeDetail('pl-1', ['dup-1']));

    const resultPromise = addTracksToPlaylistWithDuplicateCheck(playlist, ['dup-1']);
    await vi.waitFor(() => expect(getAddDuplicateDialogState().open).toBe(true));
    resolveAddDuplicateDialog('cancel');

    expect(await resultPromise).toBe(false);
    expect(addTracksToJellyfinPlaylist).not.toHaveBeenCalled();
  });

  it('falls back to the offline cache when the server cannot be reached', async () => {
    getJellyfinPlaylist.mockRejectedValue(new Error('offline'));
    getCachedJellyfinPlaylist.mockResolvedValue(makeDetail('pl-1', ['dup-1']));

    const resultPromise = addTracksToPlaylistWithDuplicateCheck(playlist, ['dup-1']);
    await vi.waitFor(() => expect(getAddDuplicateDialogState().open).toBe(true));
    resolveAddDuplicateDialog('add');

    expect(await resultPromise).toBe(true);
    expect(getCachedJellyfinPlaylist).toHaveBeenCalledWith('pl-1');
  });

  it('throws when the server refuses the add, so a failure never reads as a cancel', async () => {
    getJellyfinPlaylist.mockResolvedValue(makeDetail('pl-1', []));
    addTracksToJellyfinPlaylist.mockResolvedValueOnce(false);
    storeError = 'Jellyfin refused the add';

    await expect(addTracksToPlaylistWithDuplicateCheck(playlist, ['new-1'])).rejects.toThrow('Jellyfin refused the add');
    storeError = null;
  });

  it('a second drop waits for the first question to be answered rather than replacing it', async () => {
    getJellyfinPlaylist.mockResolvedValue(makeDetail('pl-1', ['dup-1']));
    const gym: JellyfinPlaylist = { jellyfinId: 'pl-2', name: 'Gym', itemCount: 1 };

    const first = addTracksToPlaylistWithDuplicateCheck(playlist, ['dup-1']);
    const second = addTracksToPlaylistWithDuplicateCheck(gym, ['dup-1']);
    await vi.waitFor(() => expect(getAddDuplicateDialogState().playlistName).toBe('Road Trip'));
    resolveAddDuplicateDialog('add');
    expect(await first).toBe(true);

    await vi.waitFor(() => expect(getAddDuplicateDialogState().playlistName).toBe('Gym'));
    resolveAddDuplicateDialog('skip');
    expect(await second).toBe(true);
    expect(addTracksToJellyfinPlaylist).toHaveBeenCalledTimes(1);
  });

  it('a cache AND network failure is treated as no known duplicates — never blocks the add', async () => {
    getCachedJellyfinPlaylist.mockRejectedValue(new Error('offline'));
    getJellyfinPlaylist.mockRejectedValue(new Error('offline'));

    const result = await addTracksToPlaylistWithDuplicateCheck(playlist, ['new-1']);

    expect(result).toBe(true);
    expect(getAddDuplicateDialogState().open).toBe(false);
    expect(addTracksToJellyfinPlaylist).toHaveBeenCalledWith('pl-1', ['new-1']);
  });

  it('duplicate incoming ids are deduplicated before the check and the add', async () => {
    getJellyfinPlaylist.mockResolvedValue(makeDetail('pl-1', []));

    const result = await addTracksToPlaylistWithDuplicateCheck(playlist, ['new-1', 'new-1']);

    expect(result).toBe(true);
    expect(addTracksToJellyfinPlaylist).toHaveBeenCalledWith('pl-1', ['new-1']);
  });

  it('an empty track list is a no-op success without reading the playlist at all', async () => {
    const result = await addTracksToPlaylistWithDuplicateCheck(playlist, []);

    expect(result).toBe(true);
    expect(getJellyfinPlaylist).not.toHaveBeenCalled();
    expect(addTracksToJellyfinPlaylist).not.toHaveBeenCalled();
  });
});
