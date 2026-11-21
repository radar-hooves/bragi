// Playlist polish:
// dragging tracks onto a playlist in the sidebar is one of the reference
// add-to-playlist entry points (Apple Music, Spotify), and must go through
// the same duplicate-checked path as every other one.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, screen, waitFor, fireEvent } from '@testing-library/svelte';
import PlaylistList from './PlaylistList.svelte';
import type { JellyfinPlaylist } from '#lib/types/models.js';

vi.mock('#lib/stores/jellyfin.svelte.js', () => ({
  getJellyfinState: () => ({ status: { connected: true } }),
}));

vi.mock('#lib/stores/preferences.svelte.js', () => ({
  getPreferences: () => ({ aiPlaylistEnabled: false }),
}));

vi.mock('#lib/services/tauri.js', () => ({
  getJellyfinPlaylists: vi.fn(),
  getJellyfinPlaylist: vi.fn(),
  getCachedJellyfinPlaylist: vi.fn().mockResolvedValue(null),
  getAlbumTracks: vi.fn(),
  getArtistAlbums: vi.fn(),
}));

const addTracksToPlaylistWithDuplicateCheck = vi.fn().mockResolvedValue(true);
vi.mock('#lib/utils/addToPlaylist.svelte.js', () => ({
  addTracksToPlaylistWithDuplicateCheck: (...args: unknown[]) => addTracksToPlaylistWithDuplicateCheck(...args),
}));

const startPlayback = vi.fn().mockResolvedValue(undefined);
vi.mock('#lib/utils/playback.js', () => ({
  startPlayback: (...args: unknown[]) => startPlayback(...args),
}));

const { getJellyfinPlaylists, getJellyfinPlaylist } = await import('#lib/services/tauri.js');
const {
  loadJellyfinPlaylists,
  getJellyfinPlaylistState,
  getPendingPlaylistIntent,
  clearPendingPlaylistIntent,
} = await import('#lib/stores/jellyfinPlaylist.svelte.js');
const { getUiState } = await import('#lib/stores/ui.svelte.js');

async function openRowMenu(name: string): Promise<void> {
  const row = (await screen.findByText(name)).closest('.playlist-item')!;
  const menuBtn = row.parentElement!.querySelector<HTMLElement>('.playlist-menu-trigger')!;
  await fireEvent.click(menuBtn);
}

function makePlaylist(id: string, name: string): JellyfinPlaylist {
  return { jellyfinId: id, name, itemCount: 0 };
}

/** A drop event carrying the app's own drag payload (see TrackList's handleDragStart). */
function dropEventWith(payload: object): Event {
  return dropEventWithRaw(JSON.stringify(payload));
}

function dropEventWithRaw(raw: string): Event {
  const event = new Event('drop', { bubbles: true, cancelable: true });
  Object.defineProperty(event, 'dataTransfer', {
    value: {
      getData: (type: string) => (type === 'application/x-bragi-playlist' ? raw : ''),
    },
  });
  return event;
}

describe('PlaylistList', () => {
  afterEach(() => {
    cleanup();
    addTracksToPlaylistWithDuplicateCheck.mockClear();
  });

  it('dropping tracks onto a sidebar playlist routes through the shared duplicate-check path', async () => {
    const playlist = makePlaylist('pl-1', 'Road Trip');
    vi.mocked(getJellyfinPlaylists).mockResolvedValue([playlist]);
    await loadJellyfinPlaylists();

    render(PlaylistList, { props: {} });
    const row = (await screen.findByText('Road Trip')).closest('.playlist-item')!;

    row.dispatchEvent(dropEventWith({ type: 'tracks', trackIds: ['t1', 't2'] }));

    await waitFor(() => expect(addTracksToPlaylistWithDuplicateCheck).toHaveBeenCalledWith(playlist, ['t1', 't2']));
  });

  it('a drop with no drag payload does nothing', async () => {
    const playlist = makePlaylist('pl-2', 'Empty Drop Target');
    vi.mocked(getJellyfinPlaylists).mockResolvedValue([playlist]);
    await loadJellyfinPlaylists();

    render(PlaylistList, { props: {} });
    const row = (await screen.findByText('Empty Drop Target')).closest('.playlist-item')!;

    row.dispatchEvent(dropEventWithRaw(''));
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(addTracksToPlaylistWithDuplicateCheck).not.toHaveBeenCalled();
  });

  describe('row context menu — Play / Rename / Delete without opening the playlist', () => {
    afterEach(() => {
      startPlayback.mockClear();
      clearPendingPlaylistIntent();
    });

    it('pins the item set and order: Play, Rename, Delete', async () => {
      const playlist = makePlaylist('pl-menu-1', 'Menu Pins');
      vi.mocked(getJellyfinPlaylists).mockResolvedValue([playlist]);
      await loadJellyfinPlaylists();
      render(PlaylistList, { props: {} });

      await openRowMenu('Menu Pins');
      const items = screen.getAllByRole('menuitem').map((el) => el.textContent?.trim());
      expect(items).toEqual(['Play', 'Rename', 'Delete']);
    });

    it('Play fetches the playlist and starts playback from the top', async () => {
      const playlist = makePlaylist('pl-menu-2', 'Play Me');
      vi.mocked(getJellyfinPlaylists).mockResolvedValue([playlist]);
      await loadJellyfinPlaylists();
      vi.mocked(getJellyfinPlaylist).mockResolvedValue({
        jellyfinId: 'pl-menu-2',
        tracks: [
          { jellyfinId: 't1', playlistItemId: 'i1', name: 'Song', artistName: 'Artist', albumName: 'Album', albumJellyfinId: 'a1', durationMs: 1000 },
        ],
      });
      render(PlaylistList, { props: {} });

      await openRowMenu('Play Me');
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Play' }));

      await waitFor(() => expect(startPlayback).toHaveBeenCalled());
      const [tracks, startIndex] = startPlayback.mock.calls[0]!;
      expect(tracks).toHaveLength(1);
      expect(startIndex).toBe(0);
    });

    // Rename and Delete do NOT reimplement PlaylistView's own rename field or
    // delete-confirmation dialog — they select the
    // playlist, switch to the Playlists section, and leave a pending intent
    // for PlaylistView to pick up once it mounts with that playlist loaded
    // (verified end-to-end in PlaylistView.test.ts).
    it('Rename selects the playlist, opens the Playlists section, and requests a rename', async () => {
      const playlist = makePlaylist('pl-menu-3', 'Old Name');
      vi.mocked(getJellyfinPlaylists).mockResolvedValue([playlist]);
      await loadJellyfinPlaylists();
      render(PlaylistList, { props: {} });

      await openRowMenu('Old Name');
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Rename' }));

      expect(getJellyfinPlaylistState().selectedPlaylistId).toBe('pl-menu-3');
      expect(getUiState().currentSection).toBe('playlists');
      expect(getPendingPlaylistIntent()).toEqual({ type: 'rename', playlistId: 'pl-menu-3' });
    });

    it('Delete selects the playlist, opens the Playlists section, and requests a delete — no dialog of its own', async () => {
      const playlist = makePlaylist('pl-menu-4', 'Doomed');
      vi.mocked(getJellyfinPlaylists).mockResolvedValue([playlist]);
      await loadJellyfinPlaylists();
      render(PlaylistList, { props: {} });

      await openRowMenu('Doomed');
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Delete' }));

      expect(getJellyfinPlaylistState().selectedPlaylistId).toBe('pl-menu-4');
      expect(getUiState().currentSection).toBe('playlists');
      expect(getPendingPlaylistIntent()).toEqual({ type: 'delete', playlistId: 'pl-menu-4' });
      expect(screen.queryByText('Delete playlist?')).toBeNull();
    });
  });
});
