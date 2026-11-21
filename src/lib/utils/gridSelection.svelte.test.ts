import { describe, it, expect, vi } from 'vitest';
import { createGridSelection } from './gridSelection.svelte';

interface Item {
  id: string;
  name: string;
}

function makeItems(n: number): Item[] {
  return Array.from({ length: n }, (_, i) => ({ id: `item-${i}`, name: `Item ${i}` }));
}

function click(shiftKey = false, metaKey = false, ctrlKey = false): MouseEvent {
  return { shiftKey, metaKey, ctrlKey } as MouseEvent;
}

function key(k: string, opts: Partial<KeyboardEvent> = {}): KeyboardEvent {
  return new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...opts });
}

function controller(items: Item[], onActivate = vi.fn(), columns = () => 2) {
  return {
    onActivate,
    grid: createGridSelection<Item>({
      getItems: () => items,
      getId: (i) => i.id,
      getLabel: (i) => i.name,
      getViewport: () => undefined,
      idAttribute: 'data-item-id',
      columns,
      onActivate,
    }),
  };
}

describe('createGridSelection', () => {
  it('a plain click opens the item and touches no selection state', () => {
    const items = makeItems(3);
    const { grid, onActivate } = controller(items);
    grid.handleTileClick(items[1]!, click());
    expect(onActivate).toHaveBeenCalledWith(items[1]);
    expect(grid.selection.selectedCount).toBe(0);
  });

  it('⌘-click selects the item WITHOUT opening it', () => {
    const items = makeItems(3);
    const { grid, onActivate } = controller(items);
    grid.handleTileClick(items[1]!, click(false, true));
    expect(grid.selection.isSelected('item-1')).toBe(true);
    expect(onActivate).not.toHaveBeenCalled();
  });

  it('Ctrl-click behaves the same as ⌘-click', () => {
    const items = makeItems(3);
    const { grid, onActivate } = controller(items);
    grid.handleTileClick(items[1]!, click(false, false, true));
    expect(grid.selection.isSelected('item-1')).toBe(true);
    expect(onActivate).not.toHaveBeenCalled();
  });

  it('⌘-click then Shift-click extends the range WITHOUT opening anything', () => {
    const items = makeItems(5);
    const { grid, onActivate } = controller(items);
    grid.handleTileClick(items[1]!, click(false, true)); // anchor
    grid.handleTileClick(items[3]!, click(true)); // extend
    expect(grid.selection.selectedIds).toEqual(['item-1', 'item-2', 'item-3']);
    expect(onActivate).not.toHaveBeenCalled();
  });

  it('arrow keys move the active item by the caller-supplied column stride', () => {
    const items = makeItems(5);
    const { grid } = controller(items, vi.fn(), () => 2);
    grid.handleKeyDown(key('ArrowDown')); // nothing active -> index 0
    expect(grid.selection.activeId).toBe('item-0');
    grid.handleKeyDown(key('ArrowDown')); // +2 (one row down, 2 columns)
    expect(grid.selection.activeId).toBe('item-2');
    grid.handleKeyDown(key('ArrowRight'));
    expect(grid.selection.activeId).toBe('item-3');
  });

  it('⌘/Ctrl+A selects every item', () => {
    const items = makeItems(4);
    const { grid } = controller(items);
    grid.handleKeyDown(key('a', { metaKey: true }));
    expect(grid.selection.selectedIds).toEqual(items.map((i) => i.id));
  });

  it('Return opens the current selection, starting from the first selected item', () => {
    const items = makeItems(3);
    const { grid, onActivate } = controller(items);
    grid.handleTileClick(items[1]!, click(false, true)); // select without opening
    expect(onActivate).not.toHaveBeenCalled();
    grid.handleKeyDown(key('Enter'));
    expect(onActivate).toHaveBeenCalledTimes(1);
    expect(onActivate).toHaveBeenCalledWith(items[1]);
  });

  it('type-to-find jumps to the first item matching in the current order', () => {
    const items: Item[] = [
      { id: 'a', name: 'Zeta' },
      { id: 'b', name: 'Alpha' },
      { id: 'c', name: 'Mu' },
    ];
    const { grid } = controller(items);
    grid.handleKeyDown(key('m'));
    expect(grid.selection.activeId).toBe('c'); // "Mu"
  });

  it('reveal past a windowed range asks the caller-supplied virtualiser to jump there', () => {
    const items = makeItems(10);
    const scrollToIndex = vi.fn();
    const grid = createGridSelection<Item>({
      getItems: () => items,
      getId: (i) => i.id,
      getLabel: (i) => i.name,
      getViewport: () => document.createElement('div'), // never mounts the target id
      idAttribute: 'data-item-id',
      columns: () => 1,
      onActivate: vi.fn(),
      virtual: { start: 0, end: 3, scrollToIndex },
    });
    grid.handleKeyDown(key('End')); // index 9, well outside [0, 3)
    expect(scrollToIndex).toHaveBeenCalledWith(9, 'center');
  });
});
