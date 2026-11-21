import { describe, it, expect, vi, beforeAll, afterAll, afterEach } from 'vitest';
import { tick } from 'svelte';
import { render, cleanup, fireEvent, screen } from '@testing-library/svelte';
import TrackList from './TrackList.svelte';
import { clearScrollMemory } from '#lib/utils/scrollMemory.svelte.js';
import type { Track } from '#lib/types/models.js';

const queueTrackNext = vi.fn();
const queueTrackAtEnd = vi.fn();
vi.mock('#lib/utils/queueActions.js', () => ({
  queueTrackNext: (...args: unknown[]) => queueTrackNext(...args),
  queueTrackAtEnd: (...args: unknown[]) => queueTrackAtEnd(...args),
}));

function makeTrack(i: number): Track {
  return {
    jellyfinId: `track-${i}`,
    name: `Song ${i}`,
    artistName: 'Artist',
    albumName: 'Album',
    albumJellyfinId: 'album-1',
    trackNumber: i + 1,
    discNumber: 1,
    durationMs: 200_000,
  };
}

const VIEWPORT_WIDTH = 800;
const VIEWPORT_HEIGHT = 400;

/**
 * jsdom never lays anything out (every element reports 0x0), so the
 * virtualiser would always bypass windowing and mount every row. Fixing
 * `clientWidth`/`clientHeight` on the element prototype gives every element a
 * real size for this file's tests, the same way a real window does, so
 * windowing actually engages and a 300-row test doesn't mount 300 real
 * TrackRow trees (each carrying a download button and a context menu).
 */
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

/** Waits for the virtualiser's rAF-coalesced scroll handler to have run. */
async function flushScroll(): Promise<void> {
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  await tick();
}

describe('TrackList', () => {
  afterEach(() => {
    cleanup();
    clearScrollMemory();
  });

  it('shows the empty state with no tracks', () => {
    const { getByText } = render(TrackList, {
      props: { tracks: [], visibleColumns: ['title', 'artist'], columnWidths: {} },
    });
    expect(getByText('No songs found')).toBeTruthy();
  });

  it('renders every row when the list is small (bypass path)', () => {
    const tracks = Array.from({ length: 5 }, (_, i) => makeTrack(i));
    const { container } = render(TrackList, {
      props: { tracks, visibleColumns: ['title', 'artist'], columnWidths: {} },
    });
    const rows = container.querySelectorAll('tbody tr:not(.spacer-row)');
    expect(rows.length).toBe(5);
  });

  it('windows a long list: fewer rows mount than tracks exist, with a top spacer once scrolled', async () => {
    const tracks = Array.from({ length: 300 }, (_, i) => makeTrack(i));
    const { container } = render(TrackList, {
      props: { tracks, visibleColumns: ['title', 'artist'], columnWidths: {} },
    });

    // Mounted at scrollTop 0: only the viewport's own rows + overscan exist.
    const initialRows = container.querySelectorAll('tbody tr:not(.spacer-row)');
    expect(initialRows.length).toBeLessThan(300);
    expect(initialRows.length).toBeGreaterThan(0);

    const viewport = container.querySelector<HTMLElement>('.track-table-viewport')!;
    viewport.scrollTop = 200 * 40; // scroll well down the list
    viewport.dispatchEvent(new Event('scroll'));
    await flushScroll();

    const scrolledRows = container.querySelectorAll('tbody tr:not(.spacer-row)');
    expect(scrolledRows.length).toBeLessThan(300);
    expect(scrolledRows.length).toBeGreaterThan(0);

    const topSpacer = container.querySelector<HTMLElement>('tbody tr.spacer-row td');
    expect(topSpacer).toBeTruthy();
    expect(parseInt(topSpacer!.style.height, 10)).toBeGreaterThan(0);
  });

  it('keeps the header and body in the same column order (colgroup drives both)', () => {
    const tracks = [makeTrack(0), makeTrack(1)];
    const columns = ['title', 'artist', 'album', 'time'];
    const { container } = render(TrackList, {
      props: { tracks, visibleColumns: columns, columnWidths: {} },
    });

    const headerCols = Array.from(container.querySelectorAll('thead th[data-col]')).map((th) => (th as HTMLElement).dataset.col);
    // The <colgroup> is what actually sizes both thead and tbody cells —
    // its column count (minus the trailing download column) must match the
    // header's, or the two would drift out of alignment under a resize.
    const colCount = container.querySelectorAll('colgroup col').length;
    expect(colCount).toBe(headerCols.length + 1);

    const firstBodyRowCellCount = container.querySelectorAll('tbody tr:not(.spacer-row)')[0]!.children.length;
    expect(firstBodyRowCellCount).toBe(colCount);
  });

  /** Reads rendered row order via the title cell text, which is stable
   * regardless of which other columns are shown. */
  function renderedTitles(container: HTMLElement): string[] {
    return Array.from(container.querySelectorAll('tbody tr:not(.spacer-row) .title-text')).map((el) => el.textContent);
  }

  describe('no-column state', () => {
    it('Songs: shows tracks in library (SQL) order, not grouped by disc/track/title', () => {
      // Interleaved album tracks: a disc/track/title re-sort would put every
      // album's track 1 together instead of preserving this library order.
      const tracks: Track[] = [
        { ...makeTrack(0), name: 'Zeta', discNumber: 2, trackNumber: 5 },
        { ...makeTrack(1), name: 'Alpha', discNumber: 1, trackNumber: 1 },
        { ...makeTrack(2), name: 'Mu', discNumber: 1, trackNumber: 9 },
      ];
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {} },
      });
      expect(renderedTitles(container)).toEqual(['Zeta', 'Alpha', 'Mu']);
    });

    it('Recently Played: shows tracks in recency order, not re-sorted', () => {
      // Most-recently-played first — track numbers deliberately don't align
      // with that order, so a disc/track re-sort would visibly scramble it.
      const tracks: Track[] = [
        { ...makeTrack(0), name: 'Just Played', discNumber: 1, trackNumber: 9 },
        { ...makeTrack(1), name: 'Played Earlier', discNumber: 1, trackNumber: 1 },
        { ...makeTrack(2), name: 'Played Yesterday', discNumber: 1, trackNumber: 5 },
      ];
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {}, viewKey: 'recently-played' },
      });
      expect(renderedTitles(container)).toEqual(['Just Played', 'Played Earlier', 'Played Yesterday']);
    });

    it('Search: shows tracks in relevance order, not re-sorted', () => {
      const tracks: Track[] = [
        { ...makeTrack(0), name: 'Best Match', discNumber: 3, trackNumber: 1 },
        { ...makeTrack(1), name: 'Runner Up', discNumber: 1, trackNumber: 1 },
        { ...makeTrack(2), name: 'Weakest Match', discNumber: 2, trackNumber: 1 },
      ];
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {}, viewKey: 'search' },
      });
      expect(renderedTitles(container)).toEqual(['Best Match', 'Runner Up', 'Weakest Match']);
    });

    it('the third header click (back to no column) returns to the supplied order, not disc/track/title', async () => {
      const tracks: Track[] = [
        { ...makeTrack(0), name: 'Zeta', discNumber: 2, trackNumber: 5 },
        { ...makeTrack(1), name: 'Alpha', discNumber: 1, trackNumber: 1 },
      ];
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {} },
      });
      // jsdom never lays anything out, so getBoundingClientRect() is all
      // zeros; a plain .click() (clientX 0) reads as within the header's
      // resize-edge zone and is ignored. A clientX far from that zero rect
      // reads as a genuine header click instead.
      const header = container.querySelector<HTMLElement>('th[data-col="title"]')!;
      const clickHeader = () => header.dispatchEvent(new MouseEvent('click', { bubbles: true, clientX: -1000 }));
      clickHeader(); // asc
      clickHeader(); // desc
      clickHeader(); // back to no column
      await tick();
      expect(renderedTitles(container)).toEqual(['Zeta', 'Alpha']);
    });
  });

  describe('column resize', () => {
    it("clamps a drag past the minimum at the column's own minWidth", async () => {
      const tracks = [makeTrack(0)];
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title', 'year'], columnWidths: {} },
      });
      const header = container.querySelector<HTMLElement>('th[data-col="year"]')!;
      // jsdom's getBoundingClientRect is all zeros, so a clientX within
      // RESIZE_EDGE_PX (6px) of 0 reads as grabbing the resize edge (see the
      // no-column-state test above for the same trick in reverse).
      header.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, clientX: -3 }));
      document.dispatchEvent(new MouseEvent('mousemove', { clientX: -1000 }));
      await tick(); // the live-drag width is reactive state, not a direct DOM write

      const yearCol = container.querySelectorAll('colgroup col')[1] as HTMLElement; // title, year, download
      expect(yearCol.style.width).toBe('50px'); // year's minWidth (trackColumns.ts)

      document.dispatchEvent(new MouseEvent('mouseup'));
    });

    it('does not clamp a drag that stays above the minimum', async () => {
      const tracks = [makeTrack(0)];
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title', 'year'], columnWidths: {} },
      });
      const header = container.querySelector<HTMLElement>('th[data-col="year"]')!;
      header.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, clientX: -3 }));
      document.dispatchEvent(new MouseEvent('mousemove', { clientX: 100 }));
      await tick();

      const yearCol = container.querySelectorAll('colgroup col')[1] as HTMLElement;
      expect(yearCol.style.width).toBe('103px'); // 0 (jsdom rect width) + (100 - -3)

      document.dispatchEvent(new MouseEvent('mouseup'));
    });

    it('commits the live-drag width to onColumnWidthsChange on mouseup, not before', () => {
      const onColumnWidthsChange = vi.fn();
      const tracks = [makeTrack(0)];
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title', 'year'], columnWidths: {}, onColumnWidthsChange },
      });
      const header = container.querySelector<HTMLElement>('th[data-col="year"]')!;
      header.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, clientX: -3 }));
      document.dispatchEvent(new MouseEvent('mousemove', { clientX: 100 }));
      expect(onColumnWidthsChange).not.toHaveBeenCalled();

      document.dispatchEvent(new MouseEvent('mouseup'));
      expect(onColumnWidthsChange).toHaveBeenCalledWith({ year: 103 });
    });

    // Double-click fits the column to its widest
    // content (Finder/Music.app), rather than resetting to the default.
    it("double-clicking a column's resize edge fits it, falling back to the column's minWidth where no canvas text metrics are available", () => {
      const onColumnWidthsChange = vi.fn();
      const tracks = [makeTrack(0)];
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title', 'year'], columnWidths: { year: 120 }, onColumnWidthsChange },
      });
      const header = container.querySelector<HTMLElement>('th[data-col="year"]')!;
      header.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, clientX: -3 }));
      // jsdom implements no canvas backend, so `getContext('2d')` returns
      // null and the fit falls back to year's own minWidth (50) — the same
      // null-guard Waveform.svelte already relies on for the same reason.
      expect(onColumnWidthsChange).toHaveBeenCalledWith({ year: 50 });
    });

    it('double-clicking away from the resize edge leaves widths untouched', () => {
      const onColumnWidthsChange = vi.fn();
      const tracks = [makeTrack(0)];
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title', 'year'], columnWidths: { year: 120 }, onColumnWidthsChange },
      });
      const header = container.querySelector<HTMLElement>('th[data-col="year"]')!;
      header.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, clientX: -1000 }));
      expect(onColumnWidthsChange).not.toHaveBeenCalled();
    });
  });

  describe('column reorder', () => {
    // jsdom has no DragEvent constructor; a plain Event carries everything
    // our handlers actually read (bubbling, preventDefault/defaultPrevented)
    // since dataTransfer access is always null-guarded.
    function dragEvent(type: string): Event {
      return new Event(type, { bubbles: true, cancelable: true });
    }

    it('drags a header to a new position and reports the new order', () => {
      const onColumnsChange = vi.fn();
      const tracks = [makeTrack(0)];
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title', 'artist', 'album', 'time'], columnWidths: {}, onColumnsChange },
      });
      const artistHeader = container.querySelector<HTMLElement>('th[data-col="artist"]')!;
      const albumHeader = container.querySelector<HTMLElement>('th[data-col="album"]')!;

      artistHeader.dispatchEvent(dragEvent('dragstart'));
      albumHeader.dispatchEvent(dragEvent('dragover'));
      albumHeader.dispatchEvent(dragEvent('drop'));

      expect(onColumnsChange).toHaveBeenCalledWith(['title', 'album', 'artist', 'time']);
    });

    it('never lets Title be dragged: it is pinned first', () => {
      const tracks = [makeTrack(0)];
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title', 'artist', 'album', 'time'], columnWidths: {} },
      });
      const titleHeader = container.querySelector<HTMLElement>('th[data-col="title"]')!;
      expect(titleHeader.draggable).toBe(false);

      const dragStart = dragEvent('dragstart');
      titleHeader.dispatchEvent(dragStart);
      expect(dragStart.defaultPrevented).toBe(true);
    });

    it('never lets another column be dropped onto Title (it never moves off first)', () => {
      const onColumnsChange = vi.fn();
      const tracks = [makeTrack(0)];
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title', 'artist', 'album', 'time'], columnWidths: {}, onColumnsChange },
      });
      const artistHeader = container.querySelector<HTMLElement>('th[data-col="artist"]')!;
      const titleHeader = container.querySelector<HTMLElement>('th[data-col="title"]')!;

      artistHeader.dispatchEvent(dragEvent('dragstart'));
      titleHeader.dispatchEvent(dragEvent('dragover'));
      titleHeader.dispatchEvent(dragEvent('drop'));

      expect(onColumnsChange).not.toHaveBeenCalled();
    });

    it('a plain header click (no drag) still sorts, unaffected by the drag wiring', async () => {
      const tracks: Track[] = [
        { ...makeTrack(0), name: 'Zeta' },
        { ...makeTrack(1), name: 'Alpha' },
      ];
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {} },
      });
      const header = container.querySelector<HTMLElement>('th[data-col="title"]')!;
      header.dispatchEvent(new MouseEvent('click', { bubbles: true, clientX: -1000 }));
      await tick();
      const titles = Array.from(container.querySelectorAll('tbody tr:not(.spacer-row) .title-text')).map((el) => el.textContent);
      expect(titles).toEqual(['Alpha', 'Zeta']);
    });
  });

  it('keeps header and body aligned after the caller re-supplies a reordered/resized column set', () => {
    const track: Track = { ...makeTrack(0), name: 'TitleZ', artistName: 'ArtistQ', albumName: 'AlbumR' };
    const initial = ['title', 'artist', 'album', 'time'];
    const { container, rerender } = render(TrackList, {
      props: { tracks: [track], visibleColumns: initial, columnWidths: {} },
    });

    // Simulate what happens after a drag-reorder and a resize round-trip
    // through the caller's onColumnsChange/onColumnWidthsChange: the caller
    // re-supplies both props together, same as every real call site.
    const reordered = ['title', 'album', 'artist', 'time'];
    rerender({ tracks: [track], visibleColumns: reordered, columnWidths: { artist: 220 } });

    const headerOrder = Array.from(container.querySelectorAll('thead th[data-col]')).map((th) => (th as HTMLElement).dataset.col);
    expect(headerOrder).toEqual(reordered);

    const colgroupCols = container.querySelectorAll('colgroup col');
    expect(colgroupCols.length).toBe(reordered.length + 1); // + the trailing download column
    // The <colgroup> is what actually sizes both thead and tbody — the
    // resized column's <col> must carry the width at its OWN reordered index.
    expect((colgroupCols[reordered.indexOf('artist')] as HTMLElement).style.width).toBe('220px');

    const bodyCells = container.querySelectorAll('tbody tr:not(.spacer-row) td.col-cell');
    expect(bodyCells.length).toBe(reordered.length);
    // Cell order follows the REORDERED list, not the original prop order —
    // album (index 1) before artist (index 2).
    expect(bodyCells[1]!.textContent).toContain('AlbumR');
    expect(bodyCells[2]!.textContent).toContain('ArtistQ');
  });

  it('remembers scroll position per viewKey and resets for a different key', async () => {
    // Three full renders of a 300-row table in one test — comfortably under
    // the default 5s on a quiet machine, but tight under real load.
    const tracks = Array.from({ length: 300 }, (_, i) => makeTrack(i));
    const first = render(TrackList, {
      props: { tracks, visibleColumns: ['title'], columnWidths: {}, viewKey: 'tracks' },
    });
    const viewport1 = first.container.querySelector<HTMLElement>('.track-table-viewport')!;
    viewport1.scrollTop = 200 * 40;
    // bindScrollMemory saves on cleanup regardless of a pending rAF, so
    // unmounting immediately after setting scrollTop is enough — no need to
    // wait for the scroll event's own coalesced handler here.
    first.unmount();

    const second = render(TrackList, {
      props: { tracks, visibleColumns: ['title'], columnWidths: {}, viewKey: 'tracks' },
    });
    // The restore is queued a microtask out, so it can never re-enter
    // Svelte's own scheduler mid-effect — give it one turn.
    await Promise.resolve();
    const viewport2 = second.container.querySelector<HTMLElement>('.track-table-viewport')!;
    expect(viewport2.scrollTop).toBe(200 * 40);
    second.unmount();

    const third = render(TrackList, {
      props: { tracks, visibleColumns: ['title'], columnWidths: {}, viewKey: 'recently-played' },
    });
    await Promise.resolve();
    const viewport3 = third.container.querySelector<HTMLElement>('.track-table-viewport')!;
    expect(viewport3.scrollTop).toBe(0); // a different view starts fresh
  }, 15000);

  it('computes the first window for the restored position, not scrollTop 0, on remount', async () => {
    // attachViewport's $effect and the scroll-memory $effect both run on
    // mount; the primitive must get their order right on its own, since a
    // consumer only ever declares them, it doesn't control which runs first.
    // Two full renders of a 300-row table — tight under real load.
    const tracks = Array.from({ length: 300 }, (_, i) => makeTrack(i));
    const first = render(TrackList, {
      props: { tracks, visibleColumns: ['title'], columnWidths: {}, viewKey: 'deep-restore' },
    });
    const viewport1 = first.container.querySelector<HTMLElement>('.track-table-viewport')!;
    viewport1.scrollTop = 200 * 40;
    first.unmount(); // saves on cleanup

    const second = render(TrackList, {
      props: { tracks, visibleColumns: ['title'], columnWidths: {}, viewKey: 'deep-restore' },
    });
    // No scroll event dispatched — only the microtask the restore is queued
    // on, well before anything ever paints.
    await Promise.resolve();
    await tick();

    const topSpacer = second.container.querySelector<HTMLElement>('tbody tr.spacer-row td');
    expect(topSpacer).toBeTruthy();
    expect(parseInt(topSpacer!.style.height, 10)).toBeGreaterThan(0);
    expect(second.queryByText('Song 200')).toBeTruthy();
    expect(second.queryByText('Song 0')).toBeFalsy();
  }, 15000);

  describe('selection and keyboard', () => {
    function rowFor(container: HTMLElement, id: string): HTMLElement {
      return container.querySelector<HTMLElement>(`[data-track-id="${id}"]`)!;
    }

    async function click(el: HTMLElement, opts: Partial<MouseEventInit> = {}): Promise<void> {
      el.dispatchEvent(new MouseEvent('click', { bubbles: true, ...opts }));
      await tick();
    }

    async function press(el: HTMLElement, key: string, opts: Partial<KeyboardEventInit> = {}): Promise<void> {
      el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...opts }));
      await tick();
    }

    it('a plain click selects exactly that row', async () => {
      const tracks = [makeTrack(0), makeTrack(1), makeTrack(2)];
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {} },
      });
      await click(rowFor(container, 'track-1'));
      expect(rowFor(container, 'track-1').classList.contains('selected')).toBe(true);
      expect(rowFor(container, 'track-0').classList.contains('selected')).toBe(false);
    });

    it('Shift-click selects the range from the last click', async () => {
      const tracks = Array.from({ length: 5 }, (_, i) => makeTrack(i));
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {} },
      });
      await click(rowFor(container, 'track-1'));
      await click(rowFor(container, 'track-3'), { shiftKey: true });
      for (const id of ['track-1', 'track-2', 'track-3']) {
        expect(rowFor(container, id).classList.contains('selected')).toBe(true);
      }
      expect(rowFor(container, 'track-0').classList.contains('selected')).toBe(false);
      expect(rowFor(container, 'track-4').classList.contains('selected')).toBe(false);
    });

    it('⌘-click toggles one row without clearing the rest', async () => {
      const tracks = Array.from({ length: 4 }, (_, i) => makeTrack(i));
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {} },
      });
      await click(rowFor(container, 'track-0'));
      await click(rowFor(container, 'track-2'), { metaKey: true });
      expect(rowFor(container, 'track-0').classList.contains('selected')).toBe(true);
      expect(rowFor(container, 'track-2').classList.contains('selected')).toBe(true);
      expect(rowFor(container, 'track-1').classList.contains('selected')).toBe(false);
    });

    it('⌘/Ctrl+A selects every track', async () => {
      const tracks = Array.from({ length: 4 }, (_, i) => makeTrack(i));
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {} },
      });
      const viewport = container.querySelector<HTMLElement>('.track-table-viewport')!;
      await press(viewport, 'a', { metaKey: true });
      for (let i = 0; i < 4; i++) {
        expect(rowFor(container, `track-${i}`).classList.contains('selected')).toBe(true);
      }
    });

    it('ArrowDown moves the active row forward', async () => {
      const tracks = Array.from({ length: 3 }, (_, i) => makeTrack(i));
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {} },
      });
      const viewport = container.querySelector<HTMLElement>('.track-table-viewport')!;
      await press(viewport, 'ArrowDown');
      expect(rowFor(container, 'track-0').classList.contains('active')).toBe(true);
      await press(viewport, 'ArrowDown');
      expect(rowFor(container, 'track-1').classList.contains('active')).toBe(true);
      expect(rowFor(container, 'track-0').classList.contains('active')).toBe(false);
    });

    it('Return plays the selection, starting from the first selected track', async () => {
      const tracks = Array.from({ length: 3 }, (_, i) => makeTrack(i));
      const onTrackPlay = vi.fn();
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {}, onTrackPlay },
      });
      const viewport = container.querySelector<HTMLElement>('.track-table-viewport')!;
      await click(rowFor(container, 'track-1'));
      await press(viewport, 'Enter');
      expect(onTrackPlay).toHaveBeenCalledWith(tracks[1]);
    });

    it('type-to-find jumps to the first row matching in the current (sorted) order', async () => {
      const tracks: Track[] = [
        { ...makeTrack(0), name: 'Zeta' },
        { ...makeTrack(1), name: 'Alpha' },
        { ...makeTrack(2), name: 'Mu' },
      ];
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {} },
      });
      const viewport = container.querySelector<HTMLElement>('.track-table-viewport')!;
      await press(viewport, 'm');
      expect(rowFor(container, 'track-2').classList.contains('active')).toBe(true); // "Mu"
    });

    it('selection survives a row scrolling out of the virtualised window and back', async () => {
      const tracks = Array.from({ length: 300 }, (_, i) => makeTrack(i));
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {} },
      });
      const viewport = container.querySelector<HTMLElement>('.track-table-viewport')!;

      async function scrollTo(top: number): Promise<void> {
        viewport.scrollTop = top;
        viewport.dispatchEvent(new Event('scroll'));
        await flushScroll();
      }

      // The initial mount restores a remembered scroll position on its own
      // deferred microtask; scroll once first so that one-shot restore
      // is spent before this test starts asserting on scroll state of its own.
      await scrollTo(1);
      await scrollTo(250 * 40);

      const deepRow = container.querySelector<HTMLElement>('tbody tr[data-track-id]:not(.spacer-row)')!;
      const deepId = deepRow.dataset.trackId!;
      // A row well past the always-mounted top rows — otherwise scrolling back
      // to 0 below wouldn't actually unmount it, and the test would prove nothing.
      expect(Number(deepId!.replace('track-', ''))).toBeGreaterThan(30);

      await click(deepRow);
      expect(deepRow.classList.contains('selected')).toBe(true);

      await scrollTo(0);
      expect(container.querySelector(`[data-track-id="${deepId}"]`)).toBeFalsy(); // unmounted

      await scrollTo(250 * 40);
      expect(rowFor(container, deepId).classList.contains('selected')).toBe(true); // still selected on remount
    }, 15000);
  });

  describe('add-to-playlist / drag act on the whole selection', () => {
    function rowFor(container: HTMLElement, id: string): HTMLElement {
      return container.querySelector<HTMLElement>(`[data-track-id="${id}"]`)!;
    }

    async function click(el: HTMLElement, opts: Partial<MouseEventInit> = {}): Promise<void> {
      el.dispatchEvent(new MouseEvent('click', { bubbles: true, ...opts }));
      await tick();
    }

    // jsdom has no DragEvent constructor (see the column-reorder tests above);
    // a plain Event with a stubbed dataTransfer carries everything
    // handleDragStart actually reads.
    function dragStartWith(setData: (type: string, data: string) => void): Event {
      const event = new Event('dragstart', { bubbles: true, cancelable: true });
      Object.defineProperty(event, 'dataTransfer', { value: { setData, effectAllowed: '' } });
      return event;
    }

    it('dragging a row that is part of a multi-selection carries the whole selection, not just that row', async () => {
      const tracks = Array.from({ length: 4 }, (_, i) => makeTrack(i));
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {} },
      });
      await click(rowFor(container, 'track-0'));
      await click(rowFor(container, 'track-2'), { shiftKey: true }); // selects track-0..2

      const setData = vi.fn();
      // track-1 is INSIDE the selection, even though it isn't the anchor.
      rowFor(container, 'track-1').dispatchEvent(dragStartWith(setData));

      const [, payload] = setData.mock.calls.find(([type]) => type === 'application/x-bragi-playlist')!;
      expect(JSON.parse(payload)).toEqual({ type: 'tracks', trackIds: ['track-0', 'track-1', 'track-2'] });
    });

    it('dragging a row OUTSIDE the current selection carries only that row', async () => {
      const tracks = Array.from({ length: 4 }, (_, i) => makeTrack(i));
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {} },
      });
      await click(rowFor(container, 'track-0'));
      await click(rowFor(container, 'track-1'), { shiftKey: true }); // selects track-0..1

      const setData = vi.fn();
      rowFor(container, 'track-3').dispatchEvent(dragStartWith(setData)); // not selected

      const [, payload] = setData.mock.calls.find(([type]) => type === 'application/x-bragi-playlist')!;
      expect(JSON.parse(payload)).toEqual({ type: 'tracks', trackIds: ['track-3'] });
    });

    it('"Add to Playlist" on a row inside a multi-selection adds the whole selection', async () => {
      const tracks = Array.from({ length: 3 }, (_, i) => makeTrack(i));
      const onAddToPlaylist = vi.fn();
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {}, onAddToPlaylist },
      });
      await click(rowFor(container, 'track-0'));
      await click(rowFor(container, 'track-1'), { shiftKey: true }); // selects track-0..1

      const trigger = rowFor(container, 'track-0').querySelector<HTMLElement>('.track-menu-trigger')!;
      await fireEvent.click(trigger);
      const item = await screen.findByText('Add to Playlist');
      await fireEvent.click(item);

      expect(onAddToPlaylist).toHaveBeenCalledWith([tracks[0], tracks[1]]);
    });

    it('"Add to Playlist" on a row outside the selection adds only that row', async () => {
      const tracks = Array.from({ length: 3 }, (_, i) => makeTrack(i));
      const onAddToPlaylist = vi.fn();
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {}, onAddToPlaylist },
      });
      await click(rowFor(container, 'track-0'));
      await click(rowFor(container, 'track-1'), { shiftKey: true }); // selects track-0..1

      const trigger = rowFor(container, 'track-2').querySelector<HTMLElement>('.track-menu-trigger')!;
      await fireEvent.click(trigger);
      const item = await screen.findByText('Add to Playlist');
      await fireEvent.click(item);

      expect(onAddToPlaylist).toHaveBeenCalledWith([tracks[2]]);
    });

    it('selecting five and right-clicking one queues all five with Add to Queue — not just the clicked row', async () => {
      const tracks = Array.from({ length: 5 }, (_, i) => makeTrack(i));
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {} },
      });
      await click(rowFor(container, 'track-0'));
      await click(rowFor(container, 'track-4'), { shiftKey: true }); // selects track-0..4

      await fireEvent.contextMenu(rowFor(container, 'track-2')); // right-click a row INSIDE the selection
      await fireEvent.click(await screen.findByRole('menuitem', { name: 'Add to Queue' }));

      expect(queueTrackAtEnd).toHaveBeenCalledWith(tracks);
    });

    it('selecting five and right-clicking one queues all five with Play Next too', async () => {
      const tracks = Array.from({ length: 5 }, (_, i) => makeTrack(i));
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {} },
      });
      await click(rowFor(container, 'track-0'));
      await click(rowFor(container, 'track-4'), { shiftKey: true });

      await fireEvent.contextMenu(rowFor(container, 'track-2'));
      await fireEvent.click(await screen.findByRole('menuitem', { name: 'Play Next' }));

      expect(queueTrackNext).toHaveBeenCalledWith(tracks);
    });

    it('right-clicking a row OUTSIDE the selection queues only that row', async () => {
      const tracks = Array.from({ length: 5 }, (_, i) => makeTrack(i));
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {} },
      });
      await click(rowFor(container, 'track-0'));
      await click(rowFor(container, 'track-1'), { shiftKey: true }); // selects track-0..1

      await fireEvent.contextMenu(rowFor(container, 'track-4')); // not selected
      await fireEvent.click(await screen.findByRole('menuitem', { name: 'Add to Queue' }));

      expect(queueTrackAtEnd).toHaveBeenCalledWith([tracks[4]]);
    });
  });

  describe('right-click routes to the correct menu', () => {
    function rowFor(container: HTMLElement, id: string): HTMLElement {
      return container.querySelector<HTMLElement>(`[data-track-id="${id}"]`)!;
    }

    it('on the column header opens the column-visibility menu', async () => {
      const tracks = [makeTrack(0)];
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {} },
      });
      const header = container.querySelector<HTMLElement>('th[data-col="title"]')!;

      await fireEvent.contextMenu(header);
      expect(await screen.findByText('Columns')).toBeTruthy();
    });

    it('on a track row opens that track\'s own context menu, never the column-visibility menu', async () => {
      const tracks = [makeTrack(0)];
      const { container } = render(TrackList, {
        props: { tracks, visibleColumns: ['title'], columnWidths: {} },
      });
      const row = rowFor(container, 'track-0');

      await fireEvent.contextMenu(row);
      expect(await screen.findByRole('menuitem', { name: 'Play' })).toBeTruthy();
      expect(screen.queryByText('Columns')).toBeNull();
    });
  });
});
