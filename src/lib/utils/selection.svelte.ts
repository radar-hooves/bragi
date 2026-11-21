/**
 * One ordered multi-selection model for every selectable list in the app:
 * the Songs table, a Jellyfin playlist, an album/artist
 * grid. Mirrors the Finder/Music.app/Spotify contract — plain click selects
 * one, ⌘-click toggles one, Shift-click selects the range from the last
 * anchor, ⌘A selects all, and an arrow-key move (`moveBy`/`moveTo`) either
 * replaces the selection or extends it from the anchor, exactly like a click.
 *
 * A factory returning reactive getters, not a class or a store — the same
 * shape `createVirtualList` uses, since selection is per-list UI state, not
 * app-wide shared state.
 */

export interface SelectionModifiers {
  shiftKey: boolean;
  metaKey: boolean;
  ctrlKey: boolean;
}

export interface Selection {
  /** Selected ids, in the current list order (`getIds()`), not insertion order. */
  readonly selectedIds: string[];
  readonly selectedCount: number;
  /** The last id moved or clicked to — the arrow-key cursor and the Shift-click anchor when unmodified. */
  readonly activeId: string | null;
  isSelected: (id: string) => boolean;
  /** A row's own click handler: plain replaces, ⌘/Ctrl toggles, Shift extends from the anchor. */
  click: (id: string, modifiers: SelectionModifiers) => void;
  /** Select exactly one id, resetting the anchor there. */
  selectOnly: (id: string) => void;
  selectAll: () => void;
  clear: () => void;
  /** Move the active id to an absolute index in `getIds()`; extend keeps the anchor and grows the range. */
  moveTo: (index: number, extend: boolean) => void;
  /** Move the active id by a relative offset (row count, or a grid's column stride); see `moveTo`. */
  moveBy: (delta: number, extend: boolean) => void;
  /**
   * An action (right-click, drag, a menu item) on `id` acts on the WHOLE
   * selection when `id` is part of a multi-selection, otherwise on `id`
   * alone — the Finder/Music.app/Spotify contract every selectable list in
   * the app follows (previously written out by hand
   * as `tracksForAction`/`albumsForAction`/`artistsForAction` in four
   * places). Returns ids in the list's current order, `id` itself included.
   */
  targetIds: (id: string) => string[];
}

function rangeSet(ids: string[], anchorId: string, targetId: string): Set<string> {
  const anchorIndex = ids.indexOf(anchorId);
  const targetIndex = ids.indexOf(targetId);
  if (anchorIndex === -1 || targetIndex === -1) return new Set([targetId]);
  const [lo, hi] = anchorIndex < targetIndex ? [anchorIndex, targetIndex] : [targetIndex, anchorIndex];
  return new Set(ids.slice(lo, hi + 1));
}

/** @param getIds The list's current visual order — read fresh on every call, so a live sort/filter is fine. */
export function createSelection(getIds: () => string[]): Selection {
  let selected = $state<Set<string>>(new Set());
  let anchor = $state<string | null>(null);
  let active = $state<string | null>(null);

  function selectOnly(id: string): void {
    selected = new Set([id]);
    anchor = id;
    active = id;
  }

  function click(id: string, modifiers: SelectionModifiers): void {
    if (modifiers.shiftKey && anchor !== null) {
      active = id;
      selected = rangeSet(getIds(), anchor, id);
      return;
    }
    if (modifiers.metaKey || modifiers.ctrlKey) {
      const next = new Set(selected);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      selected = next;
      anchor = id;
      active = id;
      return;
    }
    selectOnly(id);
  }

  function selectAll(): void {
    const ids = getIds();
    selected = new Set(ids);
    active = ids[ids.length - 1] ?? null;
    anchor = active;
  }

  function clear(): void {
    selected = new Set();
    anchor = null;
    active = null;
  }

  function moveTo(index: number, extend: boolean): void {
    const ids = getIds();
    if (index < 0 || index >= ids.length) return;
    const id = ids[index]!;
    active = id;
    if (extend && anchor !== null) {
      selected = rangeSet(ids, anchor, id);
    } else {
      anchor = id;
      selected = new Set([id]);
    }
  }

  function moveBy(delta: number, extend: boolean): void {
    const ids = getIds();
    if (ids.length === 0) return;
    const currentIndex = active !== null ? ids.indexOf(active) : -1;
    if (currentIndex === -1) {
      // Nothing active (or it scrolled out of the current list): land on the
      // first row for a forward move, the last for a backward one — the same
      // "first press selects something sensible" behaviour as every reference
      // app, regardless of the step size (a grid's column stride included).
      moveTo(delta > 0 ? 0 : ids.length - 1, extend);
      return;
    }
    const next = Math.min(Math.max(currentIndex + delta, 0), ids.length - 1);
    moveTo(next, extend);
  }

  function targetIds(id: string): string[] {
    if (selected.size > 1 && selected.has(id)) {
      return getIds().filter((i) => selected.has(i));
    }
    return [id];
  }

  return {
    get selectedIds() {
      return getIds().filter((id) => selected.has(id));
    },
    get selectedCount() {
      return selected.size;
    },
    get activeId() {
      return active;
    },
    isSelected: (id: string) => selected.has(id),
    click,
    selectOnly,
    selectAll,
    clear,
    moveTo,
    moveBy,
    targetIds,
  };
}
