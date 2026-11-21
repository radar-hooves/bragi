import { describe, it, expect, vi } from 'vitest';
import { createReorderDrag } from './reorderDrag.svelte';

/** A row element whose top half/bottom half decide "drop before"/"drop after" (`onDragOver`'s pointer-half math). */
function rowAt(top: number, height: number): HTMLElement {
  const el = document.createElement('div');
  el.getBoundingClientRect = () => ({ top, height, left: 0, right: 0, bottom: top + height, x: 0, y: top, width: 0, toJSON: () => ({}) }) as DOMRect;
  return el;
}

function dragOverEvent(target: EventTarget, clientY: number): DragEvent {
  const event = new Event('dragover', { bubbles: true, cancelable: true }) as DragEvent;
  Object.defineProperty(event, 'currentTarget', { value: target });
  Object.defineProperty(event, 'clientY', { value: clientY });
  return event;
}

describe('createReorderDrag', () => {
  it('drags a row down past a later one and reports the resolved (from, to)', () => {
    const onReorder = vi.fn();
    const reorder = createReorderDrag({ onReorder });

    reorder.onDragStart(new Event('dragstart') as DragEvent, 0);
    expect(reorder.draggingIndex).toBe(0);

    // Row 2 spans y 80-120; dropping in its bottom half (110) lands AFTER it.
    reorder.onDragOver(dragOverEvent(rowAt(80, 40), 110), 2);
    expect(reorder.dropIndex).toBe(3); // the gap after row 2

    reorder.onDrop(new Event('drop') as DragEvent);
    // Removing index 0 first shifts everything down by one, so the gap at 3 becomes index 2.
    expect(onReorder).toHaveBeenCalledWith(0, 2);
    expect(reorder.draggingIndex).toBeNull();
    expect(reorder.dropIndex).toBeNull();
  });

  it('drops in the top half of a row and lands BEFORE it', () => {
    const onReorder = vi.fn();
    const reorder = createReorderDrag({ onReorder });

    reorder.onDragStart(new Event('dragstart') as DragEvent, 3);
    reorder.onDragOver(dragOverEvent(rowAt(80, 40), 90), 2); // top half of row 2 (80-100)
    reorder.onDrop(new Event('drop') as DragEvent);

    expect(onReorder).toHaveBeenCalledWith(3, 2);
  });

  it('dropping back onto the same resting position commits nothing', () => {
    const onReorder = vi.fn();
    const reorder = createReorderDrag({ onReorder });

    reorder.onDragStart(new Event('dragstart') as DragEvent, 1);
    reorder.onDragOver(dragOverEvent(rowAt(40, 40), 50), 1); // top half of row 1 -> gap at 1 -> resolves to index 1 (unchanged)
    reorder.onDrop(new Event('drop') as DragEvent);

    expect(onReorder).not.toHaveBeenCalled();
  });

  it('dragend clears drag state without committing', () => {
    const onReorder = vi.fn();
    const reorder = createReorderDrag({ onReorder });

    reorder.onDragStart(new Event('dragstart') as DragEvent, 0);
    reorder.onDragOver(dragOverEvent(rowAt(80, 40), 110), 2);
    reorder.onDragEnd();

    expect(reorder.draggingIndex).toBeNull();
    expect(reorder.dropIndex).toBeNull();
    expect(onReorder).not.toHaveBeenCalled();
  });

  it('a drop with nothing dragging is a no-op', () => {
    const onReorder = vi.fn();
    const reorder = createReorderDrag({ onReorder });
    reorder.onDrop(new Event('drop') as DragEvent);
    expect(onReorder).not.toHaveBeenCalled();
  });
});
