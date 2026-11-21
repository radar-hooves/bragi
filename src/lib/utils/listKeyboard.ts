/**
 * One list-keyboard handler every selectable list wires onto its scrollable
 * container: arrow-key move (Shift extends), Home/End,
 * ⌘/Ctrl+A select all, Return activates the selection, Delete/Backspace
 * removes it where that's meaningful, and type-to-find jumps to the first
 * row matching in the list's own current order — the same convention as
 * Finder, Music.app and Spotify. Space is deliberately NOT handled here: it
 * is the global play/pause shortcut (`globalShortcuts.ts`), never a
 * per-list action.
 *
 * Works with a `Selection` (`selection.svelte.ts`) over the caller's current
 * order (`getIds`), and optionally a `columns` stride for a grid, where
 * ArrowUp/Down move a full row and ArrowLeft/Right move one tile.
 */

import { fold } from './ordering';
import type { Selection } from './selection.svelte';

export interface ListKeyboardOptions {
  /** The list's current visual order — read fresh on every key press. */
  getIds: () => string[];
  selection: Selection;
  /** A row's display label for type-to-find, in the same order as `getIds()`. Omit to disable type-to-find. */
  getLabel?: (id: string) => string;
  /** Columns per row for a grid; omit for a plain single-column list (ArrowLeft/Right then do nothing). */
  columns?: () => number;
  /** Return: activate the current selection (play a track list, open a grid tile). */
  onActivate?: (ids: string[]) => void;
  /** Delete/Backspace: remove the current selection (e.g. from a playlist). Omit where there is nothing to remove. */
  onDelete?: (ids: string[]) => void;
  /** Bring a newly active index into view once the selection moves there. */
  revealIndex?: (index: number) => void;
}

export interface ListKeyboard {
  handleKeyDown: (event: KeyboardEvent) => void;
}

const TYPE_TO_FIND_RESET_MS = 500;

export function createListKeyboard(options: ListKeyboardOptions): ListKeyboard {
  const { getIds, selection, getLabel, columns, onActivate, onDelete, revealIndex } = options;
  let searchBuffer = '';
  let searchTimeout: ReturnType<typeof setTimeout> | undefined;

  function reveal(): void {
    const id = selection.activeId;
    if (id === null) return;
    const index = getIds().indexOf(id);
    if (index >= 0) revealIndex?.(index);
  }

  function moveBy(delta: number, extend: boolean): void {
    selection.moveBy(delta, extend);
    reveal();
  }

  function moveTo(index: number, extend: boolean): void {
    selection.moveTo(index, extend);
    reveal();
  }

  function typeToFind(char: string): void {
    if (!getLabel) return;
    if (searchTimeout) clearTimeout(searchTimeout);
    searchBuffer += char.toLowerCase();
    const needle = fold(searchBuffer);
    const ids = getIds();
    const index = ids.findIndex((id) => fold(getLabel(id)).startsWith(needle));
    if (index >= 0) moveTo(index, false);
    searchTimeout = setTimeout(() => {
      searchBuffer = '';
    }, TYPE_TO_FIND_RESET_MS);
  }

  function handleKeyDown(event: KeyboardEvent): void {
    if ((event.metaKey || event.ctrlKey) && (event.key === 'a' || event.key === 'A')) {
      event.preventDefault();
      selection.selectAll();
      return;
    }

    const extend = event.shiftKey;
    const stride = columns?.() ?? 1;

    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        moveBy(stride, extend);
        return;
      case 'ArrowUp':
        event.preventDefault();
        moveBy(-stride, extend);
        return;
      case 'ArrowRight':
        if (!columns) return;
        event.preventDefault();
        moveBy(1, extend);
        return;
      case 'ArrowLeft':
        if (!columns) return;
        event.preventDefault();
        moveBy(-1, extend);
        return;
      case 'Home':
        event.preventDefault();
        moveTo(0, extend);
        return;
      case 'End':
        event.preventDefault();
        moveTo(getIds().length - 1, extend);
        return;
      case 'Enter':
        if (!onActivate || selection.selectedCount === 0) return;
        event.preventDefault();
        onActivate(selection.selectedIds);
        return;
      case 'Delete':
      case 'Backspace':
        if (!onDelete || selection.selectedCount === 0) return;
        event.preventDefault();
        onDelete(selection.selectedIds);
        return;
      default:
        if (event.key.length === 1 && !event.metaKey && !event.ctrlKey && !event.altKey) {
          typeToFind(event.key);
        }
    }
  }

  return { handleKeyDown };
}
