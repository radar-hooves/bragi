import { describe, it, expect, vi, afterEach } from 'vitest';
import { createColumnHeader } from './columnHeader.svelte';
import type { ColumnDef } from '#lib/components/library/trackColumns.js';

function col(id: string, overrides: Partial<ColumnDef> = {}): ColumnDef {
  return { id, label: id, align: 'left', minWidth: 60, render: () => '', ...overrides };
}

const COLUMNS: ColumnDef[] = [col('title', { minWidth: 120 }), col('year', { fixedWidth: 64, minWidth: 50 })];

/** A real `<th data-col>` in the resize hotzone — its right edge sits at
 * `right`, so a `clientX` within 6px of it reads as grabbing the edge. */
function headerCell(colId: string, right: number): HTMLElement {
  const th = document.createElement('th');
  th.dataset.col = colId;
  th.getBoundingClientRect = () => ({ right, width: right, left: 0, top: 0, bottom: 0, height: 0, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect;
  return th;
}

function mouseEvent(type: string, target: EventTarget, clientX: number): MouseEvent {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX });
  Object.defineProperty(event, 'target', { value: target });
  return event;
}

/** A real header ROW of sibling `<th data-col>` cells, side by side from
 * x=0, each `width` wide — so `handleMouseDown`'s pre-drag snapshot (read
 * from the dragged cell's own `parentElement`) has real siblings to freeze. */
function headerRow(cells: { colId: string; width: number }[]): HTMLElement[] {
  const row = document.createElement('tr');
  let left = 0;
  const ths: HTMLElement[] = [];
  for (const { colId, width } of cells) {
    const th = document.createElement('th');
    th.dataset.col = colId;
    const right = left + width;
    th.getBoundingClientRect = () => ({ left, right, width, top: 0, bottom: 0, height: 0, x: left, y: 0, toJSON: () => ({}) }) as DOMRect;
    row.appendChild(th);
    ths.push(th);
    left = right;
  }
  return ths;
}

function controller(getWidths: () => Record<string, number> = () => ({})) {
  const onWidthsChange = vi.fn();
  const onOrderChange = vi.fn();
  const measureFitWidth = vi.fn((c: ColumnDef) => c.minWidth + 100);
  const columnHeader = createColumnHeader({
    getColumns: () => COLUMNS,
    getWidths,
    onWidthsChange,
    onOrderChange,
    pinnedFirstColumn: 'title',
    measureFitWidth,
  });
  return { columnHeader, onWidthsChange, onOrderChange, measureFitWidth };
}

describe('createColumnHeader', () => {
  afterEach(() => {
    // A test that throws before mouseup would otherwise leak these onto later tests.
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
  });

  describe('resize', () => {
    it('is a no-op away from the resize edge', () => {
      const { columnHeader } = controller();
      const th = headerCell('year', 200);
      columnHeader.handleMouseDown(mouseEvent('mousedown', th, 50));
      expect(columnHeader.resizing).toBe(false);
    });

    it('drags the column edge, live, without committing until mouseup', () => {
      const { columnHeader, onWidthsChange } = controller();
      const th = headerCell('year', 200);
      columnHeader.handleMouseDown(mouseEvent('mousedown', th, 199)); // within 6px of 200
      expect(columnHeader.resizing).toBe(true);

      document.dispatchEvent(mouseEvent('mousemove', th, 249)); // +50px from the mousedown's 199
      expect(columnHeader.widthFor(COLUMNS[1]!)).toBe('250px'); // startWidth(200) + (249 - 199)
      expect(onWidthsChange).not.toHaveBeenCalled();

      document.dispatchEvent(new MouseEvent('mouseup'));
      expect(columnHeader.resizing).toBe(false);
      expect(onWidthsChange).toHaveBeenCalledWith({ year: 250 });
    });

    it('clamps a drag past the column minimum', () => {
      const { columnHeader, onWidthsChange } = controller();
      const th = headerCell('year', 200); // year's minWidth is 50
      columnHeader.handleMouseDown(mouseEvent('mousedown', th, 199));
      document.dispatchEvent(mouseEvent('mousemove', th, -1000));
      document.dispatchEvent(new MouseEvent('mouseup'));
      expect(onWidthsChange).toHaveBeenCalledWith({ year: 50 });
    });

    it('widthFor falls back to a stored width, then a fixed width, once the drag ends', () => {
      const { columnHeader } = controller(() => ({ year: 80 }));
      expect(columnHeader.widthFor(COLUMNS[1]!)).toBe('80px');
      expect(columnHeader.widthFor(col('untouched', { fixedWidth: 64, minWidth: 50 }))).toBe('64px');
      expect(columnHeader.widthFor(col('flexible', { minWidth: 100 }))).toBeUndefined();
    });

    // Fresh-eyes review: without freezing the OTHER
    // flexible columns' current width first, table-layout:fixed (and a CSS
    // grid track with no explicit width) redistributes its share as the
    // dragged column grows, visibly squeezing them instead of growing the
    // whole row until mouseup commits the final width.
    it("freezes every other flexible column's current width for the duration of the drag", () => {
      const flexColumns: ColumnDef[] = [col('title', { minWidth: 60 }), col('artist', { minWidth: 60 })];
      const onWidthsChange = vi.fn();
      const columnHeader = createColumnHeader({
        getColumns: () => flexColumns,
        getWidths: () => ({}),
        onWidthsChange,
        onOrderChange: vi.fn(),
        pinnedFirstColumn: 'title',
        measureFitWidth: () => 0,
      });
      const [, artistTh] = headerRow([
        { colId: 'title', width: 200 },
        { colId: 'artist', width: 150 },
      ]);

      columnHeader.handleMouseDown(mouseEvent('mousedown', artistTh!, 349)); // within 6px of artist's right edge (350)
      // Title had no explicit width before the drag — it must be frozen at
      // its current rendered width, not left to redistribute.
      expect(columnHeader.widthFor(flexColumns[0]!)).toBe('200px');

      document.dispatchEvent(mouseEvent('mousemove', artistTh!, 429)); // +80px
      expect(columnHeader.widthFor(flexColumns[0]!)).toBe('200px'); // still frozen mid-drag
      expect(columnHeader.widthFor(flexColumns[1]!)).toBe('230px'); // 150 (startWidth) + 80

      document.dispatchEvent(new MouseEvent('mouseup'));
      // Once the drag ends, the untouched column reverts to flexible sizing —
      // only the resized column keeps an explicit committed width.
      expect(columnHeader.widthFor(flexColumns[0]!)).toBeUndefined();
      expect(onWidthsChange).toHaveBeenCalledWith({ artist: 230 });
    });
  });

  describe('double-click-to-fit', () => {
    it('fits the column at the resize edge, via the injected measurer', () => {
      const { columnHeader, onWidthsChange, measureFitWidth } = controller();
      const th = headerCell('title', 300);
      columnHeader.handleDoubleClick(mouseEvent('dblclick', th, 299));
      expect(measureFitWidth).toHaveBeenCalledWith(COLUMNS[0]);
      expect(onWidthsChange).toHaveBeenCalledWith({ title: 220 }); // minWidth(120) + 100
    });

    it('does nothing away from the resize edge', () => {
      const { columnHeader, onWidthsChange } = controller();
      const th = headerCell('title', 300);
      columnHeader.handleDoubleClick(mouseEvent('dblclick', th, 50));
      expect(onWidthsChange).not.toHaveBeenCalled();
    });
  });

  describe('drag-to-reorder', () => {
    function dragEvent(type: string, target: EventTarget): DragEvent {
      const event = new Event(type, { bubbles: true, cancelable: true }) as DragEvent;
      Object.defineProperty(event, 'target', { value: target });
      return event;
    }

    it('the pinned first column is never draggable', () => {
      const { columnHeader } = controller();
      expect(columnHeader.isDraggable('title')).toBe(false);
      expect(columnHeader.isDraggable('year')).toBe(true);
    });

    it('reports the reordered column list on drop', () => {
      const onOrderChange = vi.fn();
      const cols: ColumnDef[] = [col('title'), col('artist'), col('album'), col('year', { fixedWidth: 64, minWidth: 50 })];
      const withMoreColumns = createColumnHeader({
        getColumns: () => cols,
        getWidths: () => ({}),
        onWidthsChange: vi.fn(),
        onOrderChange,
        pinnedFirstColumn: 'title',
        measureFitWidth: () => 0,
      });

      const artistTh = document.createElement('th');
      artistTh.dataset.col = 'artist';
      const albumTh = document.createElement('th');
      albumTh.dataset.col = 'album';

      withMoreColumns.handleDragStart(dragEvent('dragstart', artistTh), 'artist');
      withMoreColumns.handleDragOver(dragEvent('dragover', albumTh), 'album');
      withMoreColumns.handleDrop(dragEvent('drop', albumTh), 'album');

      expect(onOrderChange).toHaveBeenCalledWith(['title', 'album', 'artist', 'year']);
    });

    it('dropping a column onto the pinned first column is refused', () => {
      const { columnHeader, onOrderChange } = controller();
      const yearTh = document.createElement('th');
      yearTh.dataset.col = 'year';
      const titleTh = document.createElement('th');
      titleTh.dataset.col = 'title';

      columnHeader.handleDragStart(dragEvent('dragstart', yearTh), 'year');
      columnHeader.handleDragOver(dragEvent('dragover', titleTh), 'title');
      columnHeader.handleDrop(dragEvent('drop', titleTh), 'title');

      expect(onOrderChange).not.toHaveBeenCalled();
    });
  });
});
