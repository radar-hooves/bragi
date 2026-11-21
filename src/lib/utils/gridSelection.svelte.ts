/**
 * One selection + keyboard controller for every tile grid (Albums, Recently Added, Artists, Genres, each wired by hand
 * identically before this factory existed). Apple Music/Spotify's click
 * contract, not Finder's select-then-open: a plain click, a double-click and
 * Return all open the item; only ⌘/Shift-click select instead, since that's
 * the only way a multi-selection can coexist with click-to-open at all.
 *
 * ids/byId are read under `untrack()` for the same O(n^2) reactive-dependency
 * reason TrackList's own trackIds/trackById are — a large library's items are
 * deeply-reactive `$state` objects, not plain data.
 */

import { untrack } from 'svelte';
import { createSelection, type Selection } from './selection.svelte';
import { createListKeyboard } from './listKeyboard';

export interface GridSelectionOptions<T> {
  getItems: () => T[];
  getId: (item: T) => string;
  getLabel: (item: T) => string;
  getViewport: () => HTMLElement | undefined;
  /** e.g. `data-album-id` — how a revealed tile is found in the DOM. */
  idAttribute: string;
  columns: () => number;
  onActivate: (item: T) => void;
  /** A windowed grid's mounted bounds + jump, so reveal works past what's mounted. */
  virtual?: { readonly start: number; readonly end: number; scrollToIndex: (index: number, align?: 'start' | 'center') => void };
}

export interface GridSelection<T> {
  readonly selection: Selection;
  handleKeyDown: (event: KeyboardEvent) => void;
  /** Plain opens, ⌘/Shift select instead. */
  handleTileClick: (item: T, event: MouseEvent) => void;
}

export function createGridSelection<T>(options: GridSelectionOptions<T>): GridSelection<T> {
  const { getItems, getId, getLabel, getViewport, idAttribute, columns, onActivate, virtual } = options;

  // getItems() is read OUTSIDE untrack — that's what registers the dependency
  // on a new/changed array; only the per-item map/reduce over it is untracked.
  const ids = $derived.by(() => {
    const list = getItems();
    return untrack(() => list.map(getId));
  });
  const byId = $derived.by(() => {
    const list = getItems();
    return untrack(() => new Map(list.map((item) => [getId(item), item])));
  });
  const selection = createSelection(() => ids);

  function reveal(index: number): void {
    const el = getViewport();
    const id = ids[index];
    if (!el || !id) return;
    if (virtual && (index < virtual.start || index >= virtual.end)) {
      virtual.scrollToIndex(index, 'center');
    } else {
      el.querySelector(`[${idAttribute}="${CSS.escape(id)}"]`)?.scrollIntoView({ block: 'nearest' });
    }
  }

  const listKeyboard = createListKeyboard({
    getIds: () => ids,
    selection,
    getLabel: (id) => (byId.get(id) ? getLabel(byId.get(id)!) : ''),
    columns,
    onActivate: (selectedIds) => {
      const item = byId.get(selectedIds[0]!);
      if (item) onActivate(item);
    },
    revealIndex: reveal,
  });

  function handleTileClick(item: T, event: MouseEvent): void {
    if (event.shiftKey || event.metaKey || event.ctrlKey) {
      selection.click(getId(item), { shiftKey: event.shiftKey, metaKey: event.metaKey, ctrlKey: event.ctrlKey });
    } else {
      onActivate(item);
    }
  }

  return { selection, handleKeyDown: listKeyboard.handleKeyDown, handleTileClick };
}
