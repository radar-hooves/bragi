/**
 * One scroll-position memory for every view, keyed by what is on screen
 * — the Apple Music / Spotify reference: coming back to a view (back,
 * switching sections, closing a detail view) keeps where you left it, and a
 * genuinely different item (a different album, artist or playlist id) starts
 * fresh because it has a different key.
 *
 * In-memory only, for the session: nothing here needs to survive a relaunch,
 * and a key that is never revisited is a few bytes, never cleaned up.
 */

const positions = new Map<string, number>();

/** The remembered scroll position for a key, or 0 if none is saved yet. */
export function getSavedScrollTop(key: string): number {
  return positions.get(key) ?? 0;
}

/** Save a key's current scroll position. */
export function saveScrollTop(key: string, top: number): void {
  positions.set(key, top);
}

/**
 * Bind a scroll container to its memory from an `$effect`:
 *
 * ```ts
 * $effect(() => bindScrollMemory(viewKey, viewportElement));
 * ```
 *
 * Restores the saved position (or 0) as soon as the element exists, saves on
 * every scroll (rAF-coalesced, same reasoning as the virtualiser's own scroll
 * handler), and does a final save on cleanup — which fires before the effect
 * re-runs, so a key change always saves the outgoing view first. With
 * `restoreReady=false`, keep saving user scroll but defer restoration until
 * the content is tall enough; an untouched loading view cannot erase memory.
 *
 * A virtualised list does NOT use this directly — its spacer needs sizing
 * before the restore, or a deep restore gets clamped back to 0, so it
 * goes through `VirtualList.bindScrollMemory` instead, which orders that
 * itself and then defers to the same `getSavedScrollTop`/`saveScrollTop` pair.
 */
export function bindScrollMemory(
  key: string,
  el: HTMLElement | undefined,
  restoreReady = true
): (() => void) | undefined {
  if (!el) return undefined;

  if (restoreReady) el.scrollTop = getSavedScrollTop(key);

  let raf = 0;
  let scrolled = false;
  let lastTop = el.scrollTop;
  const save = () => saveScrollTop(key, el.isConnected ? el.scrollTop : lastTop);
  const onScroll = () => {
    scrolled = true;
    lastTop = el.scrollTop;
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
    if (restoreReady || scrolled) save();
  };
}

/** Test-only: clear all remembered positions between cases. */
export function clearScrollMemory(): void {
  positions.clear();
}
