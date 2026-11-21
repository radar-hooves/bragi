import { describe, it, expect } from 'vitest';
import { createSelection } from './selection.svelte';

describe('createSelection', () => {
  const ids = ['a', 'b', 'c', 'd', 'e'];
  const getIds = () => ids;

  it('a plain click selects exactly that id', () => {
    const s = createSelection(getIds);
    s.click('b', { shiftKey: false, metaKey: false, ctrlKey: false });
    expect(s.selectedIds).toEqual(['b']);
    expect(s.activeId).toBe('b');
  });

  it('a plain click on a new id replaces the previous selection', () => {
    const s = createSelection(getIds);
    s.click('b', { shiftKey: false, metaKey: false, ctrlKey: false });
    s.click('d', { shiftKey: false, metaKey: false, ctrlKey: false });
    expect(s.selectedIds).toEqual(['d']);
  });

  it('⌘/Ctrl-click toggles one id without touching the rest', () => {
    const s = createSelection(getIds);
    s.click('b', { shiftKey: false, metaKey: false, ctrlKey: false });
    s.click('d', { shiftKey: false, metaKey: true, ctrlKey: false });
    expect(s.selectedIds).toEqual(['b', 'd']); // in list order, not click order

    s.click('b', { shiftKey: false, metaKey: true, ctrlKey: false });
    expect(s.selectedIds).toEqual(['d']); // toggled back off
  });

  it('Ctrl-click behaves the same as ⌘-click', () => {
    const s = createSelection(getIds);
    s.click('a', { shiftKey: false, metaKey: false, ctrlKey: false });
    s.click('c', { shiftKey: false, metaKey: false, ctrlKey: true });
    expect(s.selectedIds).toEqual(['a', 'c']);
  });

  it('Shift-click selects the range from the last anchor, in list order', () => {
    const s = createSelection(getIds);
    s.click('b', { shiftKey: false, metaKey: false, ctrlKey: false });
    s.click('d', { shiftKey: true, metaKey: false, ctrlKey: false });
    expect(s.selectedIds).toEqual(['b', 'c', 'd']);
  });

  it('Shift-click works backwards from the anchor too', () => {
    const s = createSelection(getIds);
    s.click('d', { shiftKey: false, metaKey: false, ctrlKey: false });
    s.click('b', { shiftKey: true, metaKey: false, ctrlKey: false });
    expect(s.selectedIds).toEqual(['b', 'c', 'd']);
  });

  it('a second Shift-click re-ranges from the same anchor rather than extending the last range', () => {
    const s = createSelection(getIds);
    s.click('b', { shiftKey: false, metaKey: false, ctrlKey: false });
    s.click('d', { shiftKey: true, metaKey: false, ctrlKey: false });
    s.click('c', { shiftKey: true, metaKey: false, ctrlKey: false });
    expect(s.selectedIds).toEqual(['b', 'c']);
  });

  it('selectAll selects every id in list order', () => {
    const s = createSelection(getIds);
    s.selectAll();
    expect(s.selectedIds).toEqual(ids);
    expect(s.selectedCount).toBe(5);
  });

  it('clear empties the selection and the anchor', () => {
    const s = createSelection(getIds);
    s.selectAll();
    s.clear();
    expect(s.selectedIds).toEqual([]);
    expect(s.activeId).toBeNull();
    // A Shift-click after clear() has no anchor to range from — replaces, like a plain click.
    s.click('c', { shiftKey: true, metaKey: false, ctrlKey: false });
    expect(s.selectedIds).toEqual(['c']);
  });

  it('isSelected reflects membership', () => {
    const s = createSelection(getIds);
    s.click('b', { shiftKey: false, metaKey: false, ctrlKey: false });
    expect(s.isSelected('b')).toBe(true);
    expect(s.isSelected('a')).toBe(false);
  });

  describe('moveBy / moveTo (arrow-key nav)', () => {
    it('the first move with nothing active lands on the first row going forward', () => {
      const s = createSelection(getIds);
      s.moveBy(1, false);
      expect(s.selectedIds).toEqual(['a']);
    });

    it('the first move with nothing active lands on the last row going backward', () => {
      const s = createSelection(getIds);
      s.moveBy(-1, false);
      expect(s.selectedIds).toEqual(['e']);
    });

    it('moves the active id by the given delta, replacing the selection', () => {
      const s = createSelection(getIds);
      s.moveTo(1, false); // b
      s.moveBy(1, false); // c
      expect(s.selectedIds).toEqual(['c']);
      expect(s.activeId).toBe('c');
    });

    it('clamps at the ends of the list', () => {
      const s = createSelection(getIds);
      s.moveTo(4, false); // e, the last row
      s.moveBy(1, false);
      expect(s.activeId).toBe('e');
      expect(s.selectedIds).toEqual(['e']);

      s.moveTo(0, false); // a, the first row
      s.moveBy(-1, false);
      expect(s.activeId).toBe('a');
    });

    it('extend grows the range from the anchor instead of replacing it', () => {
      const s = createSelection(getIds);
      s.moveTo(1, false); // b — sets the anchor
      s.moveBy(1, true); // extend to c
      s.moveBy(1, true); // extend to d
      expect(s.selectedIds).toEqual(['b', 'c', 'd']);
    });

    it('moveBy honours a grid stride (a column count) for Up/Down', () => {
      const s = createSelection(getIds);
      s.moveTo(0, false); // a, index 0
      s.moveBy(2, false); // a grid with 2 columns: down one row = +2
      expect(s.activeId).toBe('c');
    });

    it('moveTo home/end reach the first and last row', () => {
      const s = createSelection(getIds);
      s.moveTo(0, false);
      expect(s.activeId).toBe('a');
      s.moveTo(ids.length - 1, false);
      expect(s.activeId).toBe('e');
    });

    it('an out-of-range moveTo is ignored', () => {
      const s = createSelection(getIds);
      s.moveTo(1, false);
      s.moveTo(99, false);
      expect(s.activeId).toBe('b'); // unchanged
    });
  });

  it('selectedIds re-derives against a live, changing list order', () => {
    let live = ['x', 'y', 'z'];
    const s = createSelection(() => live);
    s.selectAll();
    live = ['y', 'z']; // 'x' removed from the underlying list
    expect(s.selectedIds).toEqual(['y', 'z']);
  });

  // The "act on the whole selection, or just this
  // one" rule every action list (TrackList, PlaylistView, AlbumGrid,
  // ArtistGrid) shares, in one place.
  describe('targetIds', () => {
    it('a single selection acts on just that item, whatever id is passed', () => {
      const s = createSelection(getIds);
      s.click('b', { shiftKey: false, metaKey: false, ctrlKey: false });
      expect(s.targetIds('b')).toEqual(['b']);
    });

    it('an id acted on that is part of a multi-selection returns the whole selection, in list order', () => {
      const s = createSelection(getIds);
      s.click('b', { shiftKey: false, metaKey: false, ctrlKey: false });
      s.click('d', { shiftKey: false, metaKey: true, ctrlKey: false });
      expect(s.targetIds('d')).toEqual(['b', 'd']);
      expect(s.targetIds('b')).toEqual(['b', 'd']);
    });

    it('an id outside the current multi-selection acts on just itself', () => {
      const s = createSelection(getIds);
      s.click('b', { shiftKey: false, metaKey: false, ctrlKey: false });
      s.click('d', { shiftKey: false, metaKey: true, ctrlKey: false });
      expect(s.targetIds('a')).toEqual(['a']);
    });

    it('nothing selected still acts on just the passed id', () => {
      const s = createSelection(getIds);
      expect(s.targetIds('c')).toEqual(['c']);
    });
  });
});
