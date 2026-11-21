/**
 * One drag-to-reorder state machine for a row list:
 * `UpNextPanel` and `PlaylistView` each hand-wrote the same dragging/drop
 * index, half-row insertion gap, and gap-to-index conversion. Native HTML5
 * DnD — works on both WebKitGTK and WKWebView. The caller supplies its own
 * commit (`onReorder`), since what actually happens on drop differs (a
 * Jellyfin Move call with an optimistic splice + revert, vs a plain queue
 * reorder) — this only owns the pointer-tracking arithmetic.
 */

export interface ReorderDragOptions {
  /** Commits a resolved reorder — `from`/`to` are real, distinct indices. */
  onReorder: (from: number, to: number) => void;
}

export interface ReorderDrag {
  readonly draggingIndex: number | null;
  /** The insertion GAP in the current array where the dragged row will land (0..length) — not a row index. */
  readonly dropIndex: number | null;
  onDragStart: (event: DragEvent, index: number) => void;
  onDragOver: (event: DragEvent, index: number) => void;
  onDragEnd: () => void;
  onDrop: (event: DragEvent) => void;
}

export function createReorderDrag(options: ReorderDragOptions): ReorderDrag {
  const { onReorder } = options;
  let draggingIndex = $state<number | null>(null);
  let dropIndex = $state<number | null>(null);

  function reset(): void {
    draggingIndex = null;
    dropIndex = null;
  }

  function onDragStart(event: DragEvent, index: number): void {
    draggingIndex = index;
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = 'move';
      // Required or WebKitGTK may never start the drag / fire `drop`.
      event.dataTransfer.setData('text/plain', String(index));
    }
  }

  function onDragOver(event: DragEvent, index: number): void {
    if (draggingIndex === null) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
    // Drop before or after the hovered row based on the pointer's half.
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const after = event.clientY - rect.top > rect.height / 2;
    dropIndex = index + (after ? 1 : 0);
  }

  function onDragEnd(): void {
    reset();
  }

  function onDrop(event: DragEvent): void {
    event.preventDefault();
    const from = draggingIndex;
    const gap = dropIndex;
    reset();
    if (from === null || gap === null) return;

    // Convert the insertion gap to a final resting index: removing `from`
    // first shifts everything after it down by one.
    let to = gap;
    if (from < to) to -= 1;
    if (to === from || to < 0) return;

    onReorder(from, to);
  }

  return {
    get draggingIndex() {
      return draggingIndex;
    },
    get dropIndex() {
      return dropIndex;
    },
    onDragStart,
    onDragOver,
    onDragEnd,
    onDrop,
  };
}
