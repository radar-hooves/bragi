// Selection + keyboard wiring. The click/⌘/Shift/
// arrow/Return/type-to-find CONTRACT itself is tested once, generically, in
// gridSelection.svelte.test.ts — this file only pins that AlbumGrid wires its
// own DOM (data-album-id, the viewport's live column-stride formula) to that
// shared controller correctly.
import { describe, it, expect, vi, beforeAll, afterAll, afterEach } from 'vitest';
import { tick } from 'svelte';
import { render, cleanup, fireEvent, screen } from '@testing-library/svelte';
import AlbumGrid from './AlbumGrid.svelte';
import { clearScrollMemory } from '#lib/utils/scrollMemory.svelte.js';
import type { Album } from '#lib/types/models.js';

function makeAlbum(i: number): Album {
  return {
    jellyfinId: `album-${i}`,
    name: `Album ${i}`,
    artistName: 'Artist',
    artistJellyfinId: 'artist-1',
    trackCount: 10,
    durationMs: 2_000_000,
    downloadedTrackCount: 0,
  };
}

// tile 200 + gap 12 (AlbumGrid's own GAP_PX): (412 + 12) / 212 = 2 columns
// exactly, so arrow-key stride is predictable without touching tile-size state.
const VIEWPORT_WIDTH = 412;
const VIEWPORT_HEIGHT = 400;

/** See TrackList.test.ts for why clientWidth/clientHeight need faking under jsdom. */
let widthDescriptor: PropertyDescriptor | undefined;
let heightDescriptor: PropertyDescriptor | undefined;

beforeAll(() => {
  widthDescriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientWidth');
  heightDescriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientHeight');
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
    configurable: true,
    get: () => VIEWPORT_WIDTH,
  });
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
    configurable: true,
    get: () => VIEWPORT_HEIGHT,
  });
});

afterAll(() => {
  if (widthDescriptor) Object.defineProperty(HTMLElement.prototype, 'clientWidth', widthDescriptor);
  if (heightDescriptor) Object.defineProperty(HTMLElement.prototype, 'clientHeight', heightDescriptor);
});

describe('AlbumGrid selection and keyboard wiring', () => {
  afterEach(() => {
    cleanup();
    clearScrollMemory();
  });

  function cardFor(container: HTMLElement, id: string): HTMLElement {
    return container.querySelector<HTMLElement>(`[data-album-id="${id}"]`)!;
  }

  async function click(el: HTMLElement, opts: Partial<MouseEventInit> = {}): Promise<void> {
    el.dispatchEvent(new MouseEvent('click', { bubbles: true, ...opts }));
    await tick();
  }

  async function press(el: HTMLElement, key: string, opts: Partial<KeyboardEventInit> = {}): Promise<void> {
    el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...opts }));
    await tick();
  }

  it('shows the empty state with no albums', () => {
    const { getByText } = render(AlbumGrid, { props: { albums: [] } });
    expect(getByText('No albums found')).toBeTruthy();
  });

  it('a plain click opens the album — Apple Music/Spotify, not Finder-style select-then-open', async () => {
    const albums = Array.from({ length: 3 }, (_, i) => makeAlbum(i));
    const onAlbumClick = vi.fn();
    const { container } = render(AlbumGrid, { props: { albums, onAlbumClick } });
    await click(cardFor(container, 'album-1'));
    expect(onAlbumClick).toHaveBeenCalledWith(albums[1]);
    expect(cardFor(container, 'album-1').classList.contains('selected')).toBe(false);
  });

  it('⌘-click selects the card WITHOUT opening it', async () => {
    const albums = Array.from({ length: 3 }, (_, i) => makeAlbum(i));
    const onAlbumClick = vi.fn();
    const { container } = render(AlbumGrid, { props: { albums, onAlbumClick } });
    await click(cardFor(container, 'album-1'), { metaKey: true });
    expect(cardFor(container, 'album-1').classList.contains('selected')).toBe(true);
    expect(onAlbumClick).not.toHaveBeenCalled();
  });

  it('ArrowDown moves the active card by this grid\'s own live column stride (2 columns at this width)', async () => {
    const albums = Array.from({ length: 5 }, (_, i) => makeAlbum(i));
    const { container } = render(AlbumGrid, { props: { albums } });
    const viewport = container.querySelector<HTMLElement>('.album-grid-viewport')!;
    await press(viewport, 'ArrowDown'); // nothing active yet -> lands on index 0
    expect(cardFor(container, 'album-0').classList.contains('active')).toBe(true);
    await press(viewport, 'ArrowDown'); // +2 (one row down, 2 columns)
    expect(cardFor(container, 'album-2').classList.contains('active')).toBe(true);
  });

  it('picks up a new albums prop rather than staying stuck on the first one it saw', async () => {
    const first = Array.from({ length: 2 }, (_, i) => makeAlbum(i));
    const { container, rerender } = render(AlbumGrid, { props: { albums: first } });
    const replaced: Album[] = [{ ...makeAlbum(9), name: 'Replaced' }];
    await rerender({ albums: replaced });
    const viewport = container.querySelector<HTMLElement>('.album-grid-viewport')!;
    await press(viewport, 'r'); // type-to-find against the NEW list
    expect(cardFor(container, 'album-9').classList.contains('active')).toBe(true);
  });

  describe('the tile menu acts on the whole grid selection', () => {
    async function openMenu(container: HTMLElement, id: string): Promise<void> {
      const trigger = cardFor(container, id).querySelector<HTMLElement>('.card-menu-trigger')!;
      await fireEvent.click(trigger);
    }

    it('⌘-selecting five and choosing Add to Queue on one of them queues all five, not just that one', async () => {
      const albums = Array.from({ length: 5 }, (_, i) => makeAlbum(i));
      const onAlbumQueue = vi.fn();
      const { container } = render(AlbumGrid, { props: { albums, onAlbumQueue } });

      await click(cardFor(container, 'album-0'), { metaKey: true });
      await click(cardFor(container, 'album-1'), { metaKey: true });
      await click(cardFor(container, 'album-2'), { metaKey: true });
      await click(cardFor(container, 'album-3'), { metaKey: true });
      await click(cardFor(container, 'album-4'), { metaKey: true });

      await openMenu(container, 'album-2'); // a tile INSIDE the selection
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Add to Queue' }));

      expect(onAlbumQueue).toHaveBeenCalledWith(albums, 'end');
    });

    it('Play, Play Next and Add to Playlist all widen the same way', async () => {
      const albums = Array.from({ length: 3 }, (_, i) => makeAlbum(i));
      const onAlbumsPlay = vi.fn();
      const onAlbumQueue = vi.fn();
      const onAlbumAddToPlaylist = vi.fn();
      const { container } = render(AlbumGrid, {
        props: { albums, onAlbumsPlay, onAlbumQueue, onAlbumAddToPlaylist },
      });

      await click(cardFor(container, 'album-0'), { metaKey: true });
      await click(cardFor(container, 'album-1'), { metaKey: true });
      await click(cardFor(container, 'album-2'), { metaKey: true });

      await openMenu(container, 'album-1');
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Play' }));
      expect(onAlbumsPlay).toHaveBeenCalledWith(albums);

      await openMenu(container, 'album-1');
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Play Next' }));
      expect(onAlbumQueue).toHaveBeenCalledWith(albums, 'next');

      await openMenu(container, 'album-1');
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Add to Playlist' }));
      expect(onAlbumAddToPlaylist).toHaveBeenCalledWith(albums);
    });

    it('a tile OUTSIDE the selection still acts on just itself', async () => {
      const albums = Array.from({ length: 5 }, (_, i) => makeAlbum(i));
      const onAlbumQueue = vi.fn();
      const { container } = render(AlbumGrid, { props: { albums, onAlbumQueue } });

      await click(cardFor(container, 'album-0'), { metaKey: true });
      await click(cardFor(container, 'album-1'), { metaKey: true });

      await openMenu(container, 'album-4'); // not selected
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Add to Queue' }));

      expect(onAlbumQueue).toHaveBeenCalledWith([albums[4]], 'end');
    });

    it('a single-tile selection (no multi-select active) still acts on just that one tile', async () => {
      const albums = Array.from({ length: 3 }, (_, i) => makeAlbum(i));
      const onAlbumQueue = vi.fn();
      const { container } = render(AlbumGrid, { props: { albums, onAlbumQueue } });

      await openMenu(container, 'album-1'); // nothing selected at all
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Add to Queue' }));

      expect(onAlbumQueue).toHaveBeenCalledWith([albums[1]], 'end');
    });
  });
});
