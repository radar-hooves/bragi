/**
 * One column-header controller for every column-driven list: drag-to-resize, double-click-to-fit and drag-to-reorder —
 * the same mechanism `selection.svelte.ts` and `gridSelection.svelte.ts`
 * already give row/tile selection. TrackList (a `<table>`) and PlaylistView
 * (a CSS grid) keep their own row markup and their own column-visibility
 * menu; this is only what happens ON the header row.
 *
 * Resize drives a live-width override reactively (`widthFor`) rather than
 * poking a DOM element directly, so it works the same way whether the caller
 * reads it for a `<col>` style or a grid-template-columns track.
 */

import type { ColumnDef } from '#lib/components/library/trackColumns.js';

const RESIZE_EDGE_PX = 6;
const MIN_COL_WIDTH = 40;

export interface ColumnHeaderOptions {
  getColumns: () => ColumnDef[];
  getWidths: () => Record<string, number>;
  onWidthsChange: (widths: Record<string, number>) => void;
  onOrderChange: (order: string[]) => void;
  /** The one column that's never draggable, and never a valid drop target — Title, everywhere. */
  pinnedFirstColumn: string;
  /** The final (already padded/clamped) fit-to-content width for a column,
   * measured over the caller's OWN current rows and font. */
  measureFitWidth: (col: ColumnDef) => number;
}

export interface ColumnHeader {
  readonly resizing: boolean;
  readonly dragOverColId: string | null;
  /** Every column's live-drag width override, keyed by column id — only
   * populated during a resize (see `widthFor`). Exposed for a caller (like
   * PlaylistView) whose own width lookup (`colGridTrack`) needs to merge
   * this over its committed widths, rather than reading `widthFor` cell by cell. */
  readonly liveWidths: Record<string, number>;
  /** This column's live-drag width while its edge is being dragged, else its stored/default width. */
  widthFor: (col: ColumnDef) => string | undefined;
  isDraggable: (colId: string) => boolean;
  /** True when `event` lands within the resize hotzone of a header cell — the
   * same pixel-thin edge a caller with its own header click (TrackList's
   * sort) must never treat as that click. */
  isResizeEdge: (event: MouseEvent) => boolean;
  handleMouseDown: (event: MouseEvent) => void;
  handleMouseMove: (event: MouseEvent) => void;
  handleDoubleClick: (event: MouseEvent) => void;
  handleDragStart: (event: DragEvent, colId: string) => void;
  handleDragOver: (event: DragEvent, colId: string) => void;
  handleDrop: (event: DragEvent, colId: string) => void;
  handleDragEnd: () => void;
}

export function createColumnHeader(options: ColumnHeaderOptions): ColumnHeader {
  const { getColumns, getWidths, onWidthsChange, onOrderChange, pinnedFirstColumn, measureFitWidth } = options;

  let resizing = $state(false);
  let resizeColId: string | null = null;
  let resizeStartX = 0;
  let resizeStartWidth = 0;
  let liveWidths = $state<Record<string, number>>({});

  let dragColId: string | null = null;
  let dragOverColId = $state<string | null>(null);

  function findCol(colId: string): ColumnDef | undefined {
    return getColumns().find((c) => c.id === colId);
  }

  function widthFor(col: ColumnDef): string | undefined {
    const live = liveWidths[col.id];
    if (live) return `${live}px`;
    const px = getWidths()[col.id];
    if (px) return `${px}px`;
    if (col.fixedWidth) return `${col.fixedWidth}px`;
    return undefined;
  }

  function isDraggable(colId: string): boolean {
    return colId !== pinnedFirstColumn;
  }

  function thFromEvent(event: MouseEvent): HTMLElement | null {
    return (event.target as HTMLElement).closest<HTMLElement>('[data-col]');
  }

  function atResizeEdge(th: HTMLElement, clientX: number): boolean {
    return th.getBoundingClientRect().right - clientX <= RESIZE_EDGE_PX;
  }

  function isResizeEdge(event: MouseEvent): boolean {
    const th = thFromEvent(event);
    return !!th && atResizeEdge(th, event.clientX);
  }

  function handleMouseDown(event: MouseEvent): void {
    const th = thFromEvent(event);
    if (!th || !atResizeEdge(th, event.clientX)) return;
    const colId = th.dataset.col!;
    event.preventDefault();
    event.stopPropagation();

    // Freeze every sibling column's CURRENT rendered width first — a
    // table-layout:fixed <col> (or a CSS grid track) with no explicit width
    // redistributes its flexible share as the dragged column's width grows,
    // squeezing the others instead of growing the whole row/table. Read from
    // the header row/container itself (the dragged cell's own parent), so
    // this works the same for TrackList's <tr> and PlaylistView's header div.
    const snapshot: Record<string, number> = {};
    const siblings = th.parentElement?.querySelectorAll<HTMLElement>('[data-col]');
    siblings?.forEach((cell) => {
      const id = cell.dataset.col;
      if (id) snapshot[id] = cell.getBoundingClientRect().width;
    });
    liveWidths = snapshot;

    resizing = true;
    resizeColId = colId;
    resizeStartX = event.clientX;
    resizeStartWidth = snapshot[colId] ?? th.getBoundingClientRect().width;
    document.addEventListener('mousemove', handleResizeMove);
    document.addEventListener('mouseup', handleResizeEnd);
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
  }

  function handleResizeMove(event: MouseEvent): void {
    if (!resizeColId) return;
    const minWidth = findCol(resizeColId)?.minWidth ?? MIN_COL_WIDTH;
    const width = Math.max(minWidth, resizeStartWidth + event.clientX - resizeStartX);
    liveWidths = { ...liveWidths, [resizeColId]: width };
  }

  function handleResizeEnd(): void {
    document.removeEventListener('mousemove', handleResizeMove);
    document.removeEventListener('mouseup', handleResizeEnd);
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
    if (resizeColId) {
      const width = liveWidths[resizeColId];
      if (width) onWidthsChange({ ...getWidths(), [resizeColId]: width });
    }
    resizing = false;
    resizeColId = null;
    liveWidths = {};
  }

  function handleMouseMove(event: MouseEvent): void {
    if (resizing) return;
    const th = thFromEvent(event);
    if (!th) return;
    th.style.cursor = atResizeEdge(th, event.clientX) ? 'col-resize' : '';
  }

  function handleDoubleClick(event: MouseEvent): void {
    const th = thFromEvent(event);
    if (!th || !atResizeEdge(th, event.clientX)) return;
    const col = findCol(th.dataset.col!);
    if (!col) return;
    onWidthsChange({ ...getWidths(), [col.id]: measureFitWidth(col) });
  }

  function handleDragStart(event: DragEvent, colId: string): void {
    if (!isDraggable(colId)) {
      event.preventDefault();
      return;
    }
    dragColId = colId;
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = 'move';
      event.dataTransfer.setData('text/plain', colId);
    }
  }

  function handleDragOver(event: DragEvent, colId: string): void {
    if (!dragColId || !isDraggable(colId)) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
    dragOverColId = colId;
  }

  function handleDrop(event: DragEvent, targetColId: string): void {
    event.preventDefault();
    const fromColId = dragColId;
    dragColId = null;
    dragOverColId = null;
    if (!fromColId || !isDraggable(targetColId) || fromColId === targetColId) return;
    const order = getColumns().map((c) => c.id);
    const fromIdx = order.indexOf(fromColId);
    const toIdx = order.indexOf(targetColId);
    if (fromIdx < 0 || toIdx < 0) return;
    // The dropped column lands exactly at the target's ORIGINAL index —
    // splice-remove-then-insert-at-that-index does this correctly in both
    // directions without any off-by-one adjustment.
    order.splice(fromIdx, 1);
    order.splice(toIdx, 0, fromColId);
    onOrderChange(order);
  }

  function handleDragEnd(): void {
    dragColId = null;
    dragOverColId = null;
  }

  return {
    get resizing() {
      return resizing;
    },
    get dragOverColId() {
      return dragOverColId;
    },
    get liveWidths() {
      return liveWidths;
    },
    widthFor,
    isDraggable,
    isResizeEdge,
    handleMouseDown,
    handleMouseMove,
    handleDoubleClick,
    handleDragStart,
    handleDragOver,
    handleDrop,
    handleDragEnd,
  };
}
