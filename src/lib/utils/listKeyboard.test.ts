import { describe, it, expect, vi } from 'vitest';
import { createSelection } from './selection.svelte';
import { createListKeyboard } from './listKeyboard';

function key(type: string, opts: Partial<KeyboardEvent> = {}): KeyboardEvent {
  return new KeyboardEvent('keydown', { key: type, bubbles: true, cancelable: true, ...opts });
}

describe('createListKeyboard', () => {
  const labels: Record<string, string> = {
    a: 'Alpha',
    b: 'Bravo',
    c: 'Charlie',
    d: 'Delta',
    e: 'Echo',
  };
  const ids = Object.keys(labels);

  it('ArrowDown moves the selection forward and reveals the new index', () => {
    const selection = createSelection(() => ids);
    const revealIndex = vi.fn();
    const lk = createListKeyboard({ getIds: () => ids, selection, revealIndex });

    lk.handleKeyDown(key('ArrowDown'));
    expect(selection.selectedIds).toEqual(['a']);
    expect(revealIndex).toHaveBeenCalledWith(0);

    lk.handleKeyDown(key('ArrowDown'));
    expect(selection.selectedIds).toEqual(['b']);
    expect(revealIndex).toHaveBeenCalledWith(1);
  });

  it('ArrowUp moves the selection backward', () => {
    const selection = createSelection(() => ids);
    const lk = createListKeyboard({ getIds: () => ids, selection });
    selection.moveTo(2, false); // c
    lk.handleKeyDown(key('ArrowUp'));
    expect(selection.selectedIds).toEqual(['b']);
  });

  it('Shift+ArrowDown extends the selection instead of replacing it', () => {
    const selection = createSelection(() => ids);
    const lk = createListKeyboard({ getIds: () => ids, selection });
    selection.moveTo(1, false); // b
    lk.handleKeyDown(key('ArrowDown', { shiftKey: true }));
    lk.handleKeyDown(key('ArrowDown', { shiftKey: true }));
    expect(selection.selectedIds).toEqual(['b', 'c', 'd']);
  });

  it('Home/End jump to the first/last row', () => {
    const selection = createSelection(() => ids);
    const lk = createListKeyboard({ getIds: () => ids, selection });
    selection.moveTo(2, false);
    lk.handleKeyDown(key('Home'));
    expect(selection.activeId).toBe('a');
    lk.handleKeyDown(key('End'));
    expect(selection.activeId).toBe('e');
  });

  it('⌘/Ctrl+A selects every row', () => {
    const selection = createSelection(() => ids);
    const lk = createListKeyboard({ getIds: () => ids, selection });
    lk.handleKeyDown(key('a', { metaKey: true }));
    expect(selection.selectedIds).toEqual(ids);
  });

  it('a bare "a" keystroke (no modifier) does NOT select all — it is type-to-find', () => {
    const selection = createSelection(() => ids);
    const lk = createListKeyboard({ getIds: () => ids, selection, getLabel: (id) => labels[id]! });
    lk.handleKeyDown(key('a'));
    expect(selection.selectedCount).toBe(1); // jumped to "Alpha", not select-all
    expect(selection.activeId).toBe('a');
  });

  it('ArrowLeft/Right do nothing without a columns stride (a plain list has no horizontal axis)', () => {
    const selection = createSelection(() => ids);
    const lk = createListKeyboard({ getIds: () => ids, selection });
    selection.moveTo(1, false);
    const event = key('ArrowRight');
    lk.handleKeyDown(event);
    expect(selection.activeId).toBe('b'); // unchanged
    expect(event.defaultPrevented).toBe(false);
  });

  it('ArrowDown moves by the grid column stride, ArrowRight/Left move by one tile', () => {
    const selection = createSelection(() => ids);
    const lk = createListKeyboard({ getIds: () => ids, selection, columns: () => 2 });
    selection.moveTo(0, false); // a, index 0
    lk.handleKeyDown(key('ArrowDown')); // index 0 + 2
    expect(selection.activeId).toBe('c');
    lk.handleKeyDown(key('ArrowRight')); // +1
    expect(selection.activeId).toBe('d');
    lk.handleKeyDown(key('ArrowLeft')); // -1
    expect(selection.activeId).toBe('c');
  });

  it('type-to-find jumps to the first row whose label matches, case/accent-insensitively', () => {
    const selection = createSelection(() => ids);
    const lk = createListKeyboard({ getIds: () => ids, selection, getLabel: (id) => labels[id]! });
    lk.handleKeyDown(key('d'));
    expect(selection.activeId).toBe('d'); // "Delta"
  });

  it('Enter activates the current selection', () => {
    const selection = createSelection(() => ids);
    const onActivate = vi.fn();
    const lk = createListKeyboard({ getIds: () => ids, selection, onActivate });
    selection.click('b', { shiftKey: false, metaKey: false, ctrlKey: false });
    lk.handleKeyDown(key('Enter'));
    expect(onActivate).toHaveBeenCalledWith(['b']);
  });

  it('Enter does nothing with an empty selection', () => {
    const selection = createSelection(() => ids);
    const onActivate = vi.fn();
    const lk = createListKeyboard({ getIds: () => ids, selection, onActivate });
    lk.handleKeyDown(key('Enter'));
    expect(onActivate).not.toHaveBeenCalled();
  });

  it('Delete removes the current selection when onDelete is wired', () => {
    const selection = createSelection(() => ids);
    const onDelete = vi.fn();
    const lk = createListKeyboard({ getIds: () => ids, selection, onDelete });
    selection.click('c', { shiftKey: false, metaKey: false, ctrlKey: false });
    lk.handleKeyDown(key('Delete'));
    expect(onDelete).toHaveBeenCalledWith(['c']);
  });

  it('Delete is a no-op where the list has nothing to remove (no onDelete wired)', () => {
    const selection = createSelection(() => ids);
    const lk = createListKeyboard({ getIds: () => ids, selection });
    selection.click('c', { shiftKey: false, metaKey: false, ctrlKey: false });
    const event = key('Delete');
    lk.handleKeyDown(event);
    expect(event.defaultPrevented).toBe(false); // never intercepted — the browser default (if any) still applies
  });
});
