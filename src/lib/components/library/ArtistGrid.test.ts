// Selection + keyboard wiring. The click/⌘/Shift/
// arrow/Return/type-to-find CONTRACT itself is tested once, generically, in
// gridSelection.svelte.test.ts — this file only pins that ArtistGrid wires
// its own DOM (data-artist-id, the viewport's live column-stride formula) to
// that shared controller correctly.
import { describe, it, expect, vi, beforeAll, afterAll, afterEach } from 'vitest';
import { tick } from 'svelte';
import { render, cleanup, fireEvent, screen } from '@testing-library/svelte';
import ArtistGrid from './ArtistGrid.svelte';
import { clearScrollMemory } from '#lib/utils/scrollMemory.svelte.js';
import type { Artist } from '#lib/types/models.js';

function makeArtist(i: number): Artist {
  return {
    jellyfinId: `artist-${i}`,
    name: `Artist ${i}`,
    albumCount: 3,
  };
}

// tile 200 + gap 12 (ArtistGrid's own GAP_PX): (412 + 12) / 212 = 2 columns
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

describe('ArtistGrid selection and keyboard wiring', () => {
  afterEach(() => {
    cleanup();
    clearScrollMemory();
  });

  function cardFor(container: HTMLElement, id: string): HTMLElement {
    return container.querySelector<HTMLElement>(`[data-artist-id="${id}"]`)!;
  }

  async function click(el: HTMLElement, opts: Partial<MouseEventInit> = {}): Promise<void> {
    el.dispatchEvent(new MouseEvent('click', { bubbles: true, ...opts }));
    await tick();
  }

  async function press(el: HTMLElement, key: string, opts: Partial<KeyboardEventInit> = {}): Promise<void> {
    el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...opts }));
    await tick();
  }

  it('shows the empty state with no artists', () => {
    const { getByText } = render(ArtistGrid, { props: { artists: [] } });
    expect(getByText('No artists found')).toBeTruthy();
  });

  it('a plain click opens the artist — Apple Music/Spotify, not Finder-style select-then-open', async () => {
    const artists = Array.from({ length: 3 }, (_, i) => makeArtist(i));
    const onArtistClick = vi.fn();
    const { container } = render(ArtistGrid, { props: { artists, onArtistClick } });
    await click(cardFor(container, 'artist-1'));
    expect(onArtistClick).toHaveBeenCalledWith(artists[1]);
    expect(cardFor(container, 'artist-1').classList.contains('selected')).toBe(false);
  });

  it('⌘-click selects the card WITHOUT opening it', async () => {
    const artists = Array.from({ length: 3 }, (_, i) => makeArtist(i));
    const onArtistClick = vi.fn();
    const { container } = render(ArtistGrid, { props: { artists, onArtistClick } });
    await click(cardFor(container, 'artist-1'), { metaKey: true });
    expect(cardFor(container, 'artist-1').classList.contains('selected')).toBe(true);
    expect(onArtistClick).not.toHaveBeenCalled();
  });

  it('ArrowDown moves the active card by this grid\'s own live column stride (2 columns at this width)', async () => {
    const artists = Array.from({ length: 5 }, (_, i) => makeArtist(i));
    const { container } = render(ArtistGrid, { props: { artists } });
    const viewport = container.querySelector<HTMLElement>('.artist-grid-viewport')!;
    await press(viewport, 'ArrowDown'); // nothing active yet -> lands on index 0
    expect(cardFor(container, 'artist-0').classList.contains('active')).toBe(true);
    await press(viewport, 'ArrowDown'); // +2 (one row down, 2 columns)
    expect(cardFor(container, 'artist-2').classList.contains('active')).toBe(true);
  });

  describe('the tile menu acts on the whole grid selection', () => {
    async function openMenu(container: HTMLElement, id: string): Promise<void> {
      const trigger = cardFor(container, id).querySelector<HTMLElement>('.card-menu-trigger')!;
      await fireEvent.click(trigger);
    }

    it('⌘-selecting five and choosing Add to Queue on one of them queues all five, not just that one', async () => {
      const artists = Array.from({ length: 5 }, (_, i) => makeArtist(i));
      const onArtistQueue = vi.fn();
      const { container } = render(ArtistGrid, { props: { artists, onArtistQueue } });

      await click(cardFor(container, 'artist-0'), { metaKey: true });
      await click(cardFor(container, 'artist-1'), { metaKey: true });
      await click(cardFor(container, 'artist-2'), { metaKey: true });
      await click(cardFor(container, 'artist-3'), { metaKey: true });
      await click(cardFor(container, 'artist-4'), { metaKey: true });

      await openMenu(container, 'artist-2'); // a tile INSIDE the selection
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Add to Queue' }));

      expect(onArtistQueue).toHaveBeenCalledWith(artists, 'end');
    });

    it('Play, Play Next and Add to Playlist all widen the same way', async () => {
      const artists = Array.from({ length: 3 }, (_, i) => makeArtist(i));
      const onArtistPlay = vi.fn();
      const onArtistQueue = vi.fn();
      const onArtistAddToPlaylist = vi.fn();
      const { container } = render(ArtistGrid, {
        props: { artists, onArtistPlay, onArtistQueue, onArtistAddToPlaylist },
      });

      await click(cardFor(container, 'artist-0'), { metaKey: true });
      await click(cardFor(container, 'artist-1'), { metaKey: true });
      await click(cardFor(container, 'artist-2'), { metaKey: true });

      await openMenu(container, 'artist-1');
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Play' }));
      expect(onArtistPlay).toHaveBeenCalledWith(artists);

      await openMenu(container, 'artist-1');
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Play Next' }));
      expect(onArtistQueue).toHaveBeenCalledWith(artists, 'next');

      await openMenu(container, 'artist-1');
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Add to Playlist' }));
      expect(onArtistAddToPlaylist).toHaveBeenCalledWith(artists);
    });

    it('a tile OUTSIDE the selection still acts on just itself', async () => {
      const artists = Array.from({ length: 5 }, (_, i) => makeArtist(i));
      const onArtistQueue = vi.fn();
      const { container } = render(ArtistGrid, { props: { artists, onArtistQueue } });

      await click(cardFor(container, 'artist-0'), { metaKey: true });
      await click(cardFor(container, 'artist-1'), { metaKey: true });

      await openMenu(container, 'artist-4'); // not selected
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Add to Queue' }));

      expect(onArtistQueue).toHaveBeenCalledWith([artists[4]], 'end');
    });

    it('a single-tile selection (no multi-select active) still acts on just that one tile', async () => {
      const artists = Array.from({ length: 3 }, (_, i) => makeArtist(i));
      const onArtistQueue = vi.fn();
      const { container } = render(ArtistGrid, { props: { artists, onArtistQueue } });

      await openMenu(container, 'artist-1'); // nothing selected at all
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Add to Queue' }));

      expect(onArtistQueue).toHaveBeenCalledWith([artists[1]], 'end');
    });
  });
});
