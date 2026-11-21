// Playlist polish:
// "New Playlist from a selection" (Apple Music/Spotify reference) and
// adding to an existing playlist through the one shared duplicate-check
// path every add-to-playlist entry point uses.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent, screen } from '@testing-library/svelte';
import AddToJellyfinPlaylistDialog from './AddToJellyfinPlaylistDialog.svelte';
import type { JellyfinPlaylist, Track } from '#lib/types/models.js';

vi.mock('#lib/services/tauri.js', () => ({
  getJellyfinPlaylists: vi.fn().mockResolvedValue([]),
  createJellyfinPlaylist: vi.fn(),
  addTracksToJellyfinPlaylist: vi.fn(),
}));

const addTracksToPlaylistWithDuplicateCheck = vi.fn().mockResolvedValue(true);
vi.mock('#lib/utils/addToPlaylist.svelte.js', () => ({
  addTracksToPlaylistWithDuplicateCheck: (...args: unknown[]) => addTracksToPlaylistWithDuplicateCheck(...args),
}));

const { getJellyfinPlaylists, createJellyfinPlaylist, addTracksToJellyfinPlaylist } = await import('#lib/services/tauri.js');
const { loadJellyfinPlaylists } = await import('#lib/stores/jellyfinPlaylist.svelte.js');

const tracks: Track[] = [
  {
    jellyfinId: 't1',
    name: 'Song One',
    artistName: 'Artist',
    albumName: 'Album',
    albumJellyfinId: 'album-1',
    trackNumber: 1,
    discNumber: 1,
    durationMs: 200_000,
  },
  {
    jellyfinId: 't2',
    name: 'Song Two',
    artistName: 'Artist',
    albumName: 'Album',
    albumJellyfinId: 'album-1',
    trackNumber: 2,
    discNumber: 1,
    durationMs: 200_000,
  },
];

describe('AddToJellyfinPlaylistDialog', () => {
  afterEach(() => {
    cleanup();
    vi.mocked(createJellyfinPlaylist).mockReset();
    vi.mocked(addTracksToJellyfinPlaylist).mockReset();
    addTracksToPlaylistWithDuplicateCheck.mockClear();
  });

  it('creates a new playlist from the selection and adds the selection directly (no duplicate check — the playlist is brand new)', async () => {
    const created: JellyfinPlaylist = { jellyfinId: 'new-pl', name: 'My Mix', itemCount: 0 };
    vi.mocked(createJellyfinPlaylist).mockResolvedValue(created);
    vi.mocked(addTracksToJellyfinPlaylist).mockResolvedValue({ jellyfinId: 'new-pl', tracks: [] });
    const onSuccess = vi.fn();

    render(AddToJellyfinPlaylistDialog, { props: { tracks, onClose: vi.fn(), onSuccess } });

    await fireEvent.click(screen.getByText('Create new playlist'));
    const input = screen.getByPlaceholderText('Playlist name');
    await fireEvent.input(input, { target: { value: 'My Mix' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Add to Playlist' }));

    // Two sequential store awaits (create, then add) — wait for the whole
    // chain rather than assuming one tick flushes both.
    await vi.waitFor(() => expect(onSuccess).toHaveBeenCalled());

    expect(createJellyfinPlaylist).toHaveBeenCalledWith('My Mix');
    expect(addTracksToJellyfinPlaylist).toHaveBeenCalledWith('new-pl', ['t1', 't2']);
    expect(addTracksToPlaylistWithDuplicateCheck).not.toHaveBeenCalled();
    expect(onSuccess).toHaveBeenCalledWith(created);
  });

  it('adding to an EXISTING playlist goes through the shared duplicate-check path', async () => {
    const existing: JellyfinPlaylist = { jellyfinId: 'pl-1', name: 'Road Trip', itemCount: 5 };
    vi.mocked(getJellyfinPlaylists).mockResolvedValue([existing]);
    await loadJellyfinPlaylists();
    const onSuccess = vi.fn();

    render(AddToJellyfinPlaylistDialog, { props: { tracks, onClose: vi.fn(), onSuccess } });

    await fireEvent.click(await screen.findByText('Road Trip'));
    await fireEvent.click(screen.getByRole('button', { name: 'Add to Playlist' }));

    expect(addTracksToPlaylistWithDuplicateCheck).toHaveBeenCalledWith(existing, ['t1', 't2']);
    expect(addTracksToJellyfinPlaylist).not.toHaveBeenCalled();
    expect(onSuccess).toHaveBeenCalledWith(existing);
  });

  it('cancelling the duplicate question leaves the dialog open with no success callback', async () => {
    const existing: JellyfinPlaylist = { jellyfinId: 'pl-2', name: 'Chill', itemCount: 5 };
    vi.mocked(getJellyfinPlaylists).mockResolvedValue([existing]);
    await loadJellyfinPlaylists();
    addTracksToPlaylistWithDuplicateCheck.mockResolvedValueOnce(false); // user cancelled
    const onSuccess = vi.fn();
    const onClose = vi.fn();

    render(AddToJellyfinPlaylistDialog, { props: { tracks, onClose, onSuccess } });

    await fireEvent.click(await screen.findByText('Chill'));
    await fireEvent.click(screen.getByRole('button', { name: 'Add to Playlist' }));

    expect(onSuccess).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled(); // dialog stays open for another try
  });
});
