// Menus and drag-and-drop: an artist tile had neither a context menu nor a drag payload
// test — this pins both.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent, screen } from '@testing-library/svelte';
import ArtistGridCard from './ArtistGridCard.svelte';
import type { Artist } from '#lib/types/models.js';

function makeArtist(): Artist {
  return { jellyfinId: 'artist-1', name: 'Test Artist', albumCount: 3 } as Artist;
}

function dragStartWith(setData: (type: string, data: string) => void): Event {
  const event = new Event('dragstart', { bubbles: true, cancelable: true });
  Object.defineProperty(event, 'dataTransfer', { value: { setData, effectAllowed: '' } });
  return event;
}

describe('ArtistGridCard', () => {
  afterEach(cleanup);

  it('pins the item set and order: Play, Play Next, Add to Queue, Add to Playlist', async () => {
    render(ArtistGridCard, { props: { artist: makeArtist(), onAddToPlaylist: vi.fn() } });
    await fireEvent.click(screen.getByRole('button', { name: /Artist actions/ }));

    const items = screen.getAllByRole('menuitem').map((el) => el.textContent?.trim());
    expect(items).toEqual(['Play', 'Play Next', 'Add to Queue', 'Add to Playlist']);
  });

  it('omits Add to Playlist when the caller has no destination for it', async () => {
    render(ArtistGridCard, { props: { artist: makeArtist() } });
    await fireEvent.click(screen.getByRole('button', { name: /Artist actions/ }));

    const items = screen.getAllByRole('menuitem').map((el) => el.textContent?.trim());
    expect(items).toEqual(['Play', 'Play Next', 'Add to Queue']);
  });

  it('each item calls its own callback with this artist', async () => {
    const artist = makeArtist();
    const onPlay = vi.fn();
    const onQueue = vi.fn();
    const onAddToPlaylist = vi.fn();
    render(ArtistGridCard, { props: { artist, onPlay, onQueue, onAddToPlaylist } });

    await fireEvent.click(screen.getByRole('button', { name: /Artist actions/ }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Play' }));
    expect(onPlay).toHaveBeenCalledWith(artist);

    await fireEvent.click(screen.getByRole('button', { name: /Artist actions/ }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Add to Queue' }));
    expect(onQueue).toHaveBeenCalledWith(artist, 'end');

    await fireEvent.click(screen.getByRole('button', { name: /Artist actions/ }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Add to Playlist' }));
    expect(onAddToPlaylist).toHaveBeenCalledWith(artist);
  });

  it('a right-click opens the same menu the "..." trigger opens', async () => {
    const { container } = render(ArtistGridCard, { props: { artist: makeArtist() } });
    await fireEvent.contextMenu(container.querySelector('.artist-card')!);
    expect(await screen.findByRole('menuitem', { name: 'Play' })).toBeTruthy();
  });

  it('carries the one shared drag payload: { type: "artist", artistId }', () => {
    const { container } = render(ArtistGridCard, { props: { artist: makeArtist() } });
    const setData = vi.fn();
    container.querySelector('.artist-card')!.dispatchEvent(dragStartWith(setData));

    const [, payload] = setData.mock.calls.find(([type]) => type === 'application/x-bragi-playlist')!;
    expect(JSON.parse(payload)).toEqual({ type: 'artist', artistId: 'artist-1', artistName: 'Test Artist' });
  });
});
