import { describe, it, expect, vi, afterEach } from 'vitest';
import { createVirtualList } from './virtualList.svelte';
import { clearScrollMemory } from './scrollMemory.svelte';

/** A DOM-free stand-in for the viewport element the virtualiser reads. */
function fakeViewport(opts: { width: number; height: number; scrollTop?: number }) {
  return {
    clientWidth: opts.width,
    clientHeight: opts.height,
    scrollTop: opts.scrollTop ?? 0,
  } as unknown as HTMLElement;
}

/** A fakeViewport that also supports the listener calls bindScrollMemory needs. */
function fakeScrollableViewport(opts: { width: number; height: number; scrollTop?: number }) {
  return {
    clientWidth: opts.width,
    clientHeight: opts.height,
    scrollTop: opts.scrollTop ?? 0,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  } as unknown as HTMLElement;
}

describe('createVirtualList — single column, fixed row height', () => {
  it('bypasses windowing under the threshold so nothing renders blank', () => {
    const list = createVirtualList({ count: () => 10, rowHeight: 40, bypassBelow: 200 });
    list.attachViewport(fakeViewport({ width: 300, height: 400 }));

    expect(list.windowing).toBe(false);
    expect(list.start).toBe(0);
    expect(list.end).toBe(10);
    expect(list.padStart).toBe(0);
  });

  it('computes the first and last visible row with overscan', () => {
    // 1000 rows @ 40px, viewport 400px tall (10 rows visible), scrolled to row 50.
    const list = createVirtualList({
      count: () => 1000,
      rowHeight: 40,
      overscan: 3,
      bypassBelow: 200,
    });
    list.attachViewport(fakeViewport({ width: 300, height: 400, scrollTop: 50 * 40 }));

    expect(list.windowing).toBe(true);
    // firstRow = floor(2000/40) - 3 = 50 - 3 = 47
    expect(list.start).toBe(47);
    // lastRow = ceil((2000+400)/40) + 3 = ceil(60) + 3 = 63; end is exclusive
    expect(list.end).toBe(63);
    expect(list.padStart).toBe(47 * 40);
    expect(list.totalSize).toBe(1000 * 40);
    // Bottom pad = total - padStart - (end-start)*rowHeight
    expect(list.padEnd).toBe(1000 * 40 - 47 * 40 - (63 - 47) * 40);
  });

  it('clamps the range at the start and end of the list', () => {
    const list = createVirtualList({ count: () => 100, rowHeight: 40, overscan: 3, bypassBelow: 10 });

    const top = fakeViewport({ width: 300, height: 400, scrollTop: 0 });
    list.attachViewport(top);
    expect(list.start).toBe(0); // never negative

    const bottom = fakeViewport({ width: 300, height: 400, scrollTop: 99 * 40 });
    list.attachViewport(bottom);
    expect(list.end).toBe(100); // never past the item count
  });

  it('clamps the window when the count shrinks while scrolled deep', () => {
    // 1000 rows, scrolled almost to the bottom.
    let count = 1000;
    const list = createVirtualList({ count: () => count, rowHeight: 40, overscan: 3, bypassBelow: 10 });
    const el = fakeViewport({ width: 300, height: 400, scrollTop: 950 * 40 });
    list.attachViewport(el);
    expect(list.start).toBeGreaterThan(0);

    // A filter or a genre reselected shrinks the list without the stale
    // scrollTop having moved yet — the browser only clamps it a frame later.
    count = 20;
    list.refresh();

    expect(list.end).toBe(20); // ends at the last row that still exists
    expect(list.start).toBeLessThan(list.end); // and the window isn't empty
  });

  it('accounts for unvirtualised leading content sharing the scroll container', () => {
    // ColumnBrowserColumn's shape: a fixed "All" row + separator (60px) sit
    // above the windowed items in the same scroll container.
    const list = createVirtualList({
      count: () => 1000,
      rowHeight: 40,
      overscan: 0,
      bypassBelow: 200,
      leadingOffset: () => 60,
    });

    // Scrolled exactly past the leading content, to the very top of the items.
    list.attachViewport(fakeViewport({ width: 300, height: 400, scrollTop: 60 }));
    expect(list.start).toBe(0);

    // scrollToIndex must add the leading offset back in.
    const el = fakeViewport({ width: 300, height: 400 });
    list.attachViewport(el);
    list.scrollToIndex(10, 'start');
    expect(el.scrollTop).toBe(60 + 10 * 40);
  });

  it('recomputes the range on a resize', () => {
    const list = createVirtualList({ count: () => 1000, rowHeight: 40, overscan: 0, bypassBelow: 200 });
    const el = fakeViewport({ width: 300, height: 200, scrollTop: 0 });
    list.attachViewport(el);
    const shortEnd = list.end; // ceil(200/40) = 5 rows visible

    // Simulate the ResizeObserver firing after the window grows taller —
    // jsdom's mocked ResizeObserver never calls back, so the harness
    // re-attaches (the same code path a real callback would drive) with the
    // element's new metrics.
    (el as { clientHeight: number }).clientHeight = 800;
    list.attachViewport(el);

    expect(list.end).toBeGreaterThan(shortEnd);
  });

  it('refresh recomputes immediately for a data change, no rAF delay', () => {
    let count = 5;
    const list = createVirtualList({ count: () => count, rowHeight: 40, bypassBelow: 20 });
    list.attachViewport(fakeViewport({ width: 300, height: 400 }));
    expect(list.windowing).toBe(false); // 5 items, under the bypass threshold

    count = 500; // e.g. the library finished loading
    list.refresh();
    expect(list.windowing).toBe(true);
    expect(list.end).toBeLessThan(500); // now actually windowed
  });

  it('scrollToIndex sets scrollTop to the item row, centred when asked', () => {
    const el = fakeViewport({ width: 300, height: 400 });
    const list = createVirtualList({ count: () => 1000, rowHeight: 40, bypassBelow: 200 });
    list.attachViewport(el);

    list.scrollToIndex(100, 'start');
    expect(el.scrollTop).toBe(100 * 40);

    list.scrollToIndex(100, 'center');
    expect(el.scrollTop).toBe(Math.max(0, 100 * 40 - 400 / 2 + 40 / 2));
  });
});

describe('createVirtualList — grid mode (N columns, measured row height)', () => {
  const GAP = 12;
  const TILE = 200;
  const LABEL_OVERHEAD = 64;

  function grid(count: number) {
    return createVirtualList({
      count: () => count,
      columns: (width) => Math.max(1, Math.floor((width + GAP) / (TILE + GAP))),
      estimateRowHeight: (width, columns) => {
        const colWidth = (width - (columns - 1) * GAP) / columns;
        return Math.max(1, colWidth + LABEL_OVERHEAD);
      },
      overscan: 3,
      bypassBelow: 250,
    });
  }

  it('derives columns from the viewport width and slices whole rows', () => {
    const list = grid(1000);
    // width 848 -> cols = floor((848+12)/(200+12)) = floor(860/212) = 4
    list.attachViewport(fakeViewport({ width: 848, height: 600, scrollTop: 0 }));

    expect(list.columns).toBe(4);
    expect(list.start % 4).toBe(0);
    expect(list.end % 4).toBe(0);
  });

  it('corrects the estimated row height once rows are measured, without a second attachViewport call', () => {
    // The real consumer sequence: attach once, then remeasure from an
    // $effect keyed on the rendered slice — no consumer calls attachViewport
    // a second time, so remeasure() must itself trigger the recompute that
    // resizes the spacer, padStart and totalSize for the corrected height.
    const list = grid(1000);
    list.attachViewport(fakeViewport({ width: 848, height: 600, scrollTop: 4000 }));
    const estimatedPadStart = list.padStart;
    const estimatedTotalSize = list.totalSize;

    // Fake an inner container whose real rendered stride is smaller than the
    // seeded estimate (a denser card design), then re-measure.
    const realStride = 220;
    const children = Array.from({ length: 8 }, (_, i) => ({
      offsetTop: Math.floor(i / 4) * realStride,
    })) as unknown as HTMLCollection;
    list.remeasure({ children } as unknown as HTMLElement);

    expect(list.padStart).not.toBe(estimatedPadStart);
    expect(list.totalSize).not.toBe(estimatedTotalSize);
    expect(list.padStart % realStride).toBe(0);
  });

  it('a tile-size change (the grid-size slider, or UI zoom resizing the viewport) stays row-aligned with no blank rows', () => {
    // AlbumGrid/ArtistGrid's own columns()/estimateRowHeight() close over the
    // live per-view tile size exactly like this — a
    // plain mutable value read fresh on every recompute, not passed once at
    // construction. refresh() is the same call site AlbumGrid's tile-size
    // $effect uses.
    let tile = TILE;
    const list = createVirtualList({
      count: () => 1000,
      columns: (width) => Math.max(1, Math.floor((width + GAP) / (tile + GAP))),
      estimateRowHeight: (width, columns) => {
        const colWidth = (width - (columns - 1) * GAP) / columns;
        return Math.max(1, colWidth + LABEL_OVERHEAD);
      },
      overscan: 3,
      bypassBelow: 250,
    });
    const el = fakeViewport({ width: 848, height: 600, scrollTop: 4000 });
    list.attachViewport(el);
    const columnsBefore = list.columns;
    const totalSizeBefore = list.totalSize;

    // Bump the tile size (as if the user dragged the slider up, or zoomed
    // in) and refresh — the same sequence the tile-size $effect runs.
    tile = 280;
    list.refresh();

    expect(list.columns).not.toBe(columnsBefore); // fewer, larger columns fit
    expect(list.columns).toBeGreaterThan(0);
    expect(list.start % list.columns).toBe(0); // still whole rows
    expect(list.end % list.columns).toBe(0); // no partial trailing row
    expect(list.totalSize).not.toBe(totalSizeBefore);
    expect(list.padStart).toBeGreaterThanOrEqual(0);
    expect(list.padEnd).toBeGreaterThanOrEqual(0);
  });
});

describe('createVirtualList — bindScrollMemory', () => {
  afterEach(() => clearScrollMemory());

  it('computes the window for the restored position, not scrollTop 0, on first bind', async () => {
    // Simulate a remembered deep position saved by a previous mount of this
    // key — bindScrollMemory's cleanup saves regardless of a pending rAF
    // (same reasoning as scrollMemory.svelte's own bindScrollMemory).
    const previous = createVirtualList({ count: () => 1000, rowHeight: 40, overscan: 3, bypassBelow: 10 });
    const previousEl = fakeScrollableViewport({ width: 300, height: 400 });
    const previousCleanup = previous.bindScrollMemory('tracks', previousEl);
    previousEl.scrollTop = 500 * 40;
    previousCleanup?.();

    const list = createVirtualList({ count: () => 1000, rowHeight: 40, overscan: 3, bypassBelow: 10 });
    const el = fakeScrollableViewport({ width: 300, height: 400 });
    // No attachViewport call before this — bindScrollMemory must size the
    // spacer, restore the position and compute the window itself. The
    // restore is queued a microtask out (so it can never re-enter Svelte's
    // own scheduler mid-effect), so give it one turn.
    list.bindScrollMemory('tracks', el);
    await Promise.resolve();

    expect(el.scrollTop).toBe(500 * 40);
    expect(list.start).toBeGreaterThan(0); // NOT the top-of-list window
    expect(list.totalSize).toBe(1000 * 40); // the spacer was sized first
  });

  it('starts at 0 the first time a key is seen', async () => {
    const list = createVirtualList({ count: () => 1000, rowHeight: 40, bypassBelow: 10 });
    const el = fakeScrollableViewport({ width: 300, height: 400 });
    list.bindScrollMemory('never-seen-before', el);
    await Promise.resolve();

    expect(el.scrollTop).toBe(0);
    expect(list.start).toBe(0);
  });

  it('saves on cleanup so a later remount for the same key restores it', async () => {
    const list = createVirtualList({ count: () => 1000, rowHeight: 40, overscan: 3, bypassBelow: 10 });
    const el = fakeScrollableViewport({ width: 300, height: 400 });
    const cleanup = list.bindScrollMemory('albums', el);
    await Promise.resolve();
    el.scrollTop = 300 * 40;
    cleanup?.();

    const reopened = createVirtualList({ count: () => 1000, rowHeight: 40, overscan: 3, bypassBelow: 10 });
    const reopenedEl = fakeScrollableViewport({ width: 300, height: 400 });
    reopened.bindScrollMemory('albums', reopenedEl);
    await Promise.resolve();

    expect(reopenedEl.scrollTop).toBe(300 * 40);
  });
});

describe('createVirtualList — cleanup', () => {
  it('attachViewport returns a cleanup that disconnects the ResizeObserver', () => {
    const disconnect = vi.fn();
    const observe = vi.fn();
    const OriginalRO = globalThis.ResizeObserver;
    globalThis.ResizeObserver = function () {
      return { observe, disconnect, unobserve: vi.fn() };
    } as unknown as typeof ResizeObserver;

    try {
      const list = createVirtualList({ count: () => 10, rowHeight: 40 });
      const cleanup = list.attachViewport(fakeViewport({ width: 300, height: 400 }));
      expect(observe).toHaveBeenCalledOnce();
      cleanup?.();
      expect(disconnect).toHaveBeenCalledOnce();
    } finally {
      globalThis.ResizeObserver = OriginalRO;
    }
  });
});
