/**
 * One virtualised-list primitive for every long list in the app.
 *
 * Generalises the windowing math TrackList, AlbumGrid and ArtistGrid each used
 * to hand-roll separately: a fixed or measured row height, N columns (1 for a
 * plain list), overscan, a small-list bypass so nothing ever renders blank,
 * and a scroll handler coalesced to one recompute per animation frame —
 * running it synchronously on every scroll event turns a fast fling into an
 * unbounded read-write-read layout-thrash cascade that pegs the main thread.
 *
 * A caller owns its DOM shape (a `<table>`'s spacer `<tr>`s, a grid's
 * transformed spacer `<div>`) and reads back only numbers: `start`/`end` index
 * bounds, `padStart`/`padEnd` pixel offsets, and `totalSize`.
 */

import { getSavedScrollTop, saveScrollTop } from './scrollMemory.svelte';

export interface VirtualListOptions {
  /** Total item count. Read on every recompute, so a live count is fine. */
  count: () => number;
  /** Columns per row; omit for a plain single-column list. */
  columns?: (viewportWidth: number) => number;
  /** A known, constant row height in px. Mutually exclusive with measuring. */
  rowHeight?: number;
  /**
   * Seed estimate for a row height that isn't known up front, corrected by
   * measuring the real rendered stride once rows exist (`remeasure`). Ignored
   * when `rowHeight` is set.
   */
  estimateRowHeight?: (viewportWidth: number, columns: number) => number;
  /** Extra rows kept mounted above/below the viewport. Default 3. */
  overscan?: number;
  /** Below this item count, windowing is bypassed so nothing ever renders blank. Default 200. */
  bypassBelow?: number;
  /**
   * Height in px of unvirtualised content sharing the same scroll container
   * ahead of the windowed items (e.g. ColumnBrowserColumn's "All" row) — added
   * to `padStart` and accounted for in the scroll-position math, so the
   * windowed content's own coordinates don't need to know about it.
   */
  leadingOffset?: () => number;
}

export interface VirtualList {
  readonly windowing: boolean;
  readonly start: number;
  readonly end: number;
  readonly padStart: number;
  readonly padEnd: number;
  readonly totalSize: number;
  readonly columns: number;
  /** Bind to the scroll container's `onscroll`. */
  handleScroll: () => void;
  /** Recompute immediately, for a data change rather than a scroll. */
  refresh: () => void;
  /**
   * Bind from an `$effect` over the viewport element: runs the first compute
   * before paint, owns that viewport's ResizeObserver, and returns the
   * cleanup the effect should return.
   */
  attachViewport: (el: HTMLElement | undefined) => (() => void) | undefined;
  /**
   * Bind scroll-position memory to the viewport from a second `$effect`,
   * keyed on whatever makes the remembered view change (a `viewKey` prop, a
   * selected item's id). Sizes the spacer for the current data, restores the
   * remembered position on the next microtask (after that resize has reached
   * the DOM), and only then computes the window — in that order, so a deep
   * restore is never clamped by a spacer that hasn't grown yet, and the first
   * rendered window is for the restored position rather than `scrollTop` 0.
   * Self-contained regardless of whether this runs before or after
   * `attachViewport`'s own effect.
   */
  bindScrollMemory: (key: string, el: HTMLElement | undefined) => (() => void) | undefined;
  /**
   * Measured mode only: re-derive the row height from two adjacent rendered
   * rows' `offsetTop`. Call from an `$effect` keyed on whatever makes rows
   * resize (columns, a tile-size setting, the rendered slice).
   */
  remeasure: (inner: HTMLElement | undefined) => void;
  /** Scroll a given item index into view; `'center'` centres it (the ⌘L reveal). */
  scrollToIndex: (index: number, align?: 'start' | 'center') => void;
}

export function createVirtualList(options: VirtualListOptions): VirtualList {
  const getColumns = options.columns ?? (() => 1);
  const overscan = options.overscan ?? 3;
  const bypassBelow = options.bypassBelow ?? 200;
  const fixedRowHeight = options.rowHeight;
  const getEstimate = options.estimateRowHeight ?? (() => 40);
  const getLeadingOffset = options.leadingOffset ?? (() => 0);

  let viewportEl: HTMLElement | undefined;
  let windowing = $state(true);
  let cols = $state(1);
  let measuredRowHeight = $state(0);
  let start = $state(0);
  let end = $state(0);
  let padStart = $state(0);
  let totalSize = $state(0);
  let scrollRaf = 0;

  function rowHeightPx(width: number, columns: number): number {
    if (fixedRowHeight) return fixedRowHeight;
    return measuredRowHeight > 0 ? measuredRowHeight : Math.max(1, getEstimate(width, columns));
  }

  function recompute(): void {
    const el = viewportEl;
    if (!el) return;
    const n = options.count();
    const width = el.clientWidth;
    const height = el.clientHeight;

    if (n <= bypassBelow || width <= 0 || height <= 0) {
      windowing = false;
      start = 0;
      end = n;
      padStart = 0;
      totalSize = 0;
      return;
    }

    const c = Math.max(1, getColumns(width));
    const rh = rowHeightPx(width, c);
    const totalRows = Math.ceil(n / c);
    const nextTotal = totalRows * rh;

    // Scroll position relative to where the windowed content actually starts —
    // unvirtualised content sharing this scroll container (leadingOffset) isn't
    // part of the row math. Clamped to the range the browser will settle on:
    // a shrunk list (a filter, a genre reselected, a track removed) can leave
    // a stale scrollTop pointing past the new end, which would otherwise put
    // the whole window past every row that still exists.
    const maxScrollTop = Math.max(0, nextTotal - height);
    const st = Math.min(maxScrollTop, Math.max(0, el.scrollTop - getLeadingOffset()));
    const firstRow = Math.max(0, Math.floor(st / rh) - overscan);
    const lastRow = Math.min(totalRows, Math.ceil((st + height) / rh) + overscan);

    const nextStart = firstRow * c;
    const nextEnd = Math.min(n, lastRow * c);
    const nextPadStart = firstRow * rh;

    windowing = true;
    if (c !== cols) cols = c;
    if (nextStart !== start) start = nextStart;
    if (nextEnd !== end) end = nextEnd;
    if (nextPadStart !== padStart) padStart = nextPadStart;
    if (nextTotal !== totalSize) totalSize = nextTotal;
  }

  function handleScroll(): void {
    if (scrollRaf) return;
    scrollRaf = requestAnimationFrame(() => {
      scrollRaf = 0;
      recompute();
    });
  }

  /**
   * Recompute immediately — for a data change (filter, sort, section switch),
   * where the range must be right before the next paint, not coalesced to the
   * next frame the way a scroll event is.
   */
  function refresh(): void {
    recompute();
  }

  function remeasure(inner: HTMLElement | undefined): void {
    if (fixedRowHeight || !inner || !windowing || cols < 1) return;
    const kids = inner.children;
    if (kids.length <= cols) return;
    const stride = (kids[cols] as HTMLElement).offsetTop - (kids[0] as HTMLElement).offsetTop;
    if (stride > 1 && Math.abs(stride - measuredRowHeight) > 0.5) {
      measuredRowHeight = stride;
      // The spacer, scrollbar length and block offset are all sized off the
      // row height — without this, they carry the old estimate until
      // something else (a scroll, a resize) happens to trigger a recompute.
      recompute();
    }
  }

  function attachViewport(el: HTMLElement | undefined): (() => void) | undefined {
    viewportEl = el;
    if (!el) return undefined;
    recompute();
    const ro = new ResizeObserver(() => recompute());
    ro.observe(el);
    return () => {
      ro.disconnect();
      if (scrollRaf) {
        cancelAnimationFrame(scrollRaf);
        scrollRaf = 0;
      }
    };
  }

  function bindScrollMemory(key: string, el: HTMLElement | undefined): (() => void) | undefined {
    if (!el) return undefined;
    // Set directly rather than relying on attachViewport having already run
    // this flush — this must be correct regardless of which of the
    // consumer's two effects fires first.
    viewportEl = el;

    // Size the spacer for the current data now, then restore the remembered
    // position on the next microtask, once Svelte has patched the DOM for
    // that resize — restoring into a spacer that hasn't grown yet lets the
    // browser clamp the restore straight back to 0. Deferred rather
    // than forced synchronously: this runs from inside a consumer's $effect,
    // and re-entering Svelte's own scheduler mid-flush (flushSync) deadlocks
    // it. A microtask still lands well before the next paint, so nothing
    // visible ever shows the wrong window.
    recompute();
    queueMicrotask(() => {
      if (viewportEl !== el) return; // superseded by a later bind/unbind
      el.scrollTop = getSavedScrollTop(key);
      recompute();
    });

    let raf = 0;
    const save = () => saveScrollTop(key, el.scrollTop);
    const onScroll = () => {
      if (raf) return;
      raf = requestAnimationFrame(() => {
        raf = 0;
        save();
      });
    };

    el.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      el.removeEventListener('scroll', onScroll);
      if (raf) cancelAnimationFrame(raf);
      save();
    };
  }

  function scrollToIndex(index: number, align: 'start' | 'center' = 'start'): void {
    const el = viewportEl;
    if (!el) return;
    const width = el.clientWidth;
    const c = Math.max(1, cols || getColumns(width));
    const rh = rowHeightPx(width, c);
    const row = Math.floor(index / c);
    const top = getLeadingOffset() + row * rh;
    el.scrollTop = align === 'center' ? Math.max(0, top - el.clientHeight / 2 + rh / 2) : Math.max(0, top);
    recompute();
  }

  return {
    get windowing() {
      return windowing;
    },
    get start() {
      return start;
    },
    get end() {
      return end;
    },
    get padStart() {
      return padStart;
    },
    get padEnd() {
      if (!windowing) return 0;
      const c = Math.max(1, cols);
      const width = viewportEl?.clientWidth ?? 0;
      const rh = rowHeightPx(width, c);
      const visibleRows = Math.ceil((end - start) / c);
      return Math.max(0, totalSize - padStart - visibleRows * rh);
    },
    get totalSize() {
      return totalSize;
    },
    get columns() {
      return cols;
    },
    handleScroll,
    refresh,
    attachViewport,
    bindScrollMemory,
    remeasure,
    scrollToIndex,
  };
}
