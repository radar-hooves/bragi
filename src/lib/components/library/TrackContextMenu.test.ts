// Menus and drag-and-drop: the ONE track context menu definition, reused by every list
// (Songs, an album, an artist, search, Recently Played, a playlist, the
// now-playing bar). These tests pin its item set, order and wording so a
// future edit can't silently drift a second copy back into existence.
//
// Play Next / Add to Queue are caller-supplied callbacks (onQueueNext/
// onQueueEnd), never a scalar queue call hardwired to `track` — the caller
// (TrackList, PlaylistView, ...) is what knows whether the right-clicked row
// is part of a wider selection.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent, screen } from '@testing-library/svelte';
import TrackContextMenu from './TrackContextMenu.svelte';
import type { Track } from '#lib/types/models.js';

const revealTrackInLibrary = vi.fn();
vi.mock('#lib/utils/revealInLibrary.svelte.js', () => ({
  revealTrackInLibrary: (...args: unknown[]) => revealTrackInLibrary(...args),
}));

function makeTrack(): Track {
  return {
    jellyfinId: 'track-1',
    name: 'Song',
    artistName: 'Artist',
    albumName: 'Album',
    albumJellyfinId: 'album-1',
    trackNumber: 1,
    discNumber: 1,
    durationMs: 200_000,
  };
}

function baseProps(overrides: Record<string, unknown> = {}) {
  return {
    track: makeTrack(),
    showAddToPlaylist: true,
    onPlay: vi.fn(),
    onQueueNext: vi.fn(),
    onQueueEnd: vi.fn(),
    onAddToPlaylist: vi.fn(),
    onRemoveFromPlaylist: undefined as (() => void) | undefined,
    ...overrides,
  };
}

async function openMenu(): Promise<void> {
  const trigger = screen.getByRole('button', { name: /Track actions/ });
  await fireEvent.click(trigger);
}

describe('TrackContextMenu', () => {
  afterEach(() => {
    cleanup();
    revealTrackInLibrary.mockClear();
  });

  it('pins the item set and order outside a playlist: Play, Play Next, Add to Queue, Add to Playlist, Show in Library', async () => {
    render(TrackContextMenu, { props: baseProps() });
    await openMenu();

    const items = screen.getAllByRole('menuitem').map((el) => el.textContent?.trim());
    expect(items).toEqual(['Play', 'Play Next', 'Add to Queue', 'Add to Playlist', 'Show in Library']);
  });

  it('omits Add to Playlist when the caller has no add-to-playlist destination', async () => {
    render(TrackContextMenu, { props: baseProps({ showAddToPlaylist: false }) });
    await openMenu();

    const items = screen.getAllByRole('menuitem').map((el) => el.textContent?.trim());
    expect(items).toEqual(['Play', 'Play Next', 'Add to Queue', 'Show in Library']);
  });

  it('appends Remove from Playlist only inside a playlist', async () => {
    render(TrackContextMenu, { props: baseProps({ onRemoveFromPlaylist: vi.fn() }) });
    await openMenu();

    const items = screen.getAllByRole('menuitem').map((el) => el.textContent?.trim());
    expect(items).toEqual(['Play', 'Play Next', 'Add to Queue', 'Add to Playlist', 'Show in Library', 'Remove from Playlist']);
  });

  it('Play, Play Next, Add to Queue, Add to Playlist and Remove from Playlist each call their own caller-supplied callback — never an internal queue call of their own', async () => {
    const props = baseProps({ onRemoveFromPlaylist: vi.fn() });
    render(TrackContextMenu, { props });

    await openMenu();
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Play' }));
    expect(props.onPlay).toHaveBeenCalledTimes(1);

    await openMenu();
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Play Next' }));
    expect(props.onQueueNext).toHaveBeenCalledTimes(1);

    await openMenu();
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Add to Queue' }));
    expect(props.onQueueEnd).toHaveBeenCalledTimes(1);

    await openMenu();
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Add to Playlist' }));
    expect(props.onAddToPlaylist).toHaveBeenCalledTimes(1);

    await openMenu();
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Remove from Playlist' }));
    expect(props.onRemoveFromPlaylist).toHaveBeenCalledTimes(1);
  });

  it('Show in Library reveals this track, the one shared reveal mechanism', async () => {
    const track = makeTrack();
    render(TrackContextMenu, { props: baseProps({ track }) });
    await openMenu();
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Show in Library' }));
    expect(revealTrackInLibrary).toHaveBeenCalledWith(track);
  });

  it('disables the playback-shaped items when offline with no local copy', async () => {
    render(TrackContextMenu, { props: baseProps({ playbackDisabled: true }) });
    await openMenu();

    for (const name of ['Play', 'Play Next', 'Add to Queue']) {
      expect(screen.getByRole('menuitem', { name }).getAttribute('data-disabled')).not.toBeNull();
    }
    expect(screen.getByRole('menuitem', { name: 'Add to Playlist' }).getAttribute('data-disabled')).toBeNull();
  });

  it('a right-click on the row it belongs to opens it via the bindable open prop', async () => {
    const { rerender } = render(TrackContextMenu, { props: baseProps({ open: false }) });
    expect(screen.queryByRole('menuitem', { name: 'Play' })).toBeNull();

    await rerender(baseProps({ open: true }));
    expect(await screen.findByRole('menuitem', { name: 'Play' })).toBeTruthy();
  });
});
