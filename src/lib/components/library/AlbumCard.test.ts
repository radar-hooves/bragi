// Menus and drag-and-drop: an album tile had neither a context menu nor a drag payload
// test — this pins both.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent, screen } from '@testing-library/svelte';
import AlbumCard from './AlbumCard.svelte';
import type { Album } from '#lib/types/models.js';

function makeAlbum(): Album {
  return {
    jellyfinId: 'album-1',
    name: 'Test Album',
    artistName: 'Artist',
    artistJellyfinId: 'artist-1',
    year: 2020,
    trackCount: 10,
    downloadedTrackCount: 0,
  } as Album;
}

function dragStartWith(setData: (type: string, data: string) => void): Event {
  const event = new Event('dragstart', { bubbles: true, cancelable: true });
  Object.defineProperty(event, 'dataTransfer', { value: { setData, effectAllowed: '' } });
  return event;
}

describe('AlbumCard', () => {
  afterEach(cleanup);

  it('pins the item set and order: Play, Play Next, Add to Queue, Add to Playlist', async () => {
    render(AlbumCard, { props: { album: makeAlbum(), onAddToPlaylist: vi.fn() } });
    await fireEvent.click(screen.getByRole('button', { name: /Album actions/ }));

    const items = screen.getAllByRole('menuitem').map((el) => el.textContent?.trim());
    expect(items).toEqual(['Play', 'Play Next', 'Add to Queue', 'Add to Playlist']);
  });

  it('omits Add to Playlist when the caller has no destination for it', async () => {
    render(AlbumCard, { props: { album: makeAlbum() } });
    await fireEvent.click(screen.getByRole('button', { name: /Album actions/ }));

    const items = screen.getAllByRole('menuitem').map((el) => el.textContent?.trim());
    expect(items).toEqual(['Play', 'Play Next', 'Add to Queue']);
  });

  it('each item calls its own callback with this album', async () => {
    const album = makeAlbum();
    const onPlay = vi.fn();
    const onQueue = vi.fn();
    const onAddToPlaylist = vi.fn();
    render(AlbumCard, { props: { album, onPlay, onQueue, onAddToPlaylist } });

    await fireEvent.click(screen.getByRole('button', { name: /Album actions/ }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Play' }));
    expect(onPlay).toHaveBeenCalledWith(album);

    await fireEvent.click(screen.getByRole('button', { name: /Album actions/ }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Play Next' }));
    expect(onQueue).toHaveBeenCalledWith(album, 'next');

    await fireEvent.click(screen.getByRole('button', { name: /Album actions/ }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Add to Queue' }));
    expect(onQueue).toHaveBeenCalledWith(album, 'end');

    await fireEvent.click(screen.getByRole('button', { name: /Album actions/ }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Add to Playlist' }));
    expect(onAddToPlaylist).toHaveBeenCalledWith(album);
  });

  it('a right-click opens the same menu the "..." trigger opens', async () => {
    const { container } = render(AlbumCard, { props: { album: makeAlbum() } });
    await fireEvent.contextMenu(container.querySelector('.album-card')!);
    expect(await screen.findByRole('menuitem', { name: 'Play' })).toBeTruthy();
  });

  it('carries the one shared drag payload: { type: "album", albumId }', () => {
    const { container } = render(AlbumCard, { props: { album: makeAlbum() } });
    const setData = vi.fn();
    container.querySelector('.album-card')!.dispatchEvent(dragStartWith(setData));

    const [, payload] = setData.mock.calls.find(([type]) => type === 'application/x-bragi-playlist')!;
    expect(JSON.parse(payload)).toEqual({ type: 'album', albumId: 'album-1', albumName: 'Test Album' });
  });
});
