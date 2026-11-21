import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { tick } from 'svelte';
import { render, cleanup, fireEvent } from '@testing-library/svelte';
import UpNextPanel from './UpNextPanel.svelte';
import type { QueueEntrySource, Track } from '#lib/types/models.js';

vi.mock('#lib/services/tauri.js', () => ({
  removeFromQueue: vi.fn().mockResolvedValue(undefined),
  reorderQueue: vi.fn().mockResolvedValue(undefined),
  jumpToQueueIndex: vi.fn().mockResolvedValue(undefined),
  clearQueue: vi.fn().mockResolvedValue(undefined),
}));

const { removeFromQueue, reorderQueue, jumpToQueueIndex, clearQueue } = await import('#lib/services/tauri.js');
const { setQueueState } = await import('#lib/stores/queue.svelte.js');

function track(id: string): Track {
  return {
    jellyfinId: id,
    mbid: null,
    artistMbid: null,
    releaseMbid: null,
    name: `Song ${id}`,
    artistName: 'Artist',
    albumName: 'Album',
    albumJellyfinId: 'album-1',
    trackNumber: 1,
    discNumber: 1,
    durationMs: 200_000,
    genre: null,
    year: null,
    artworkUrl: null,
    cachedFilePath: null,
    fileSizeBytes: null,
    normalizationGainDb: null,
  } as unknown as Track;
}

/** now, next-1, next-2 (userQueued), ctx-1, ctx-2 (context) — currentIndex 0. */
function seedMixedQueue(): void {
  const tracks = [track('now'), track('next-1'), track('next-2'), track('ctx-1'), track('ctx-2')];
  const sources: QueueEntrySource[] = ['context', 'userQueued', 'userQueued', 'context', 'context'];
  setQueueState({ tracks, currentIndex: 0, sources });
}

function rowFor(container: HTMLElement, absoluteIndex: number): HTMLElement {
  return container.querySelector<HTMLElement>(`[data-entry-id="${absoluteIndex}"]`)!;
}

describe('UpNextPanel', () => {
  beforeEach(() => {
    setQueueState({ tracks: [], currentIndex: -1, sources: [] });
    vi.mocked(removeFromQueue).mockClear();
    vi.mocked(reorderQueue).mockClear();
    vi.mocked(jumpToQueueIndex).mockClear();
    vi.mocked(clearQueue).mockClear();
  });

  afterEach(() => {
    cleanup();
  });

  it('shows the current track under "Now Playing", separate from the queue rows', () => {
    seedMixedQueue();
    const { container } = render(UpNextPanel);
    expect(container.querySelector('.now-playing-row .track-name')?.textContent).toBe('Song now');
    expect(container.querySelector('[data-entry-id="0"]')).toBeFalsy(); // the current track is not a queue row
  });

  it('groups Playing Next (user-queued) above Next Up (the rest of the context)', () => {
    seedMixedQueue();
    const { container } = render(UpNextPanel);

    const headers = Array.from(container.querySelectorAll('.section-header')).map((el) => el.textContent);
    expect(headers).toEqual(['Playing Next', 'Next Up']);

    const names = Array.from(container.querySelectorAll('.entry-row .track-name')).map((el) => el.textContent);
    expect(names).toEqual(['Song next-1', 'Song next-2', 'Song ctx-1', 'Song ctx-2']);
  });

  it('follows setQueueState (queue-changed) rather than holding its own copy', async () => {
    seedMixedQueue();
    const { container } = render(UpNextPanel);
    expect(container.querySelectorAll('.entry-row').length).toBe(4);

    setQueueState({ tracks: [track('now'), track('only-next')], currentIndex: 0, sources: ['context', 'context'] });
    await tick();

    const names = Array.from(container.querySelectorAll('.entry-row .track-name')).map((el) => el.textContent);
    expect(names).toEqual(['Song only-next']);
  });

  it('double-clicking a row jumps to that queue index', async () => {
    seedMixedQueue();
    const { container } = render(UpNextPanel);
    await fireEvent.dblClick(rowFor(container, 3));
    expect(jumpToQueueIndex).toHaveBeenCalledWith(3);
  });

  it('Return jumps to the active row after ArrowDown selects it', async () => {
    seedMixedQueue();
    const { container } = render(UpNextPanel);
    const viewport = container.querySelector<HTMLElement>('.up-next-viewport')!;

    viewport.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }));
    await tick();
    expect(rowFor(container, 1).classList.contains('active')).toBe(true);

    viewport.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    expect(jumpToQueueIndex).toHaveBeenCalledWith(1);
  });

  it('removes the selected rows highest-index-first, so an earlier removal never shifts a later target', async () => {
    seedMixedQueue();
    const { container } = render(UpNextPanel);

    await fireEvent.click(rowFor(container, 1));
    await fireEvent.click(rowFor(container, 4), { shiftKey: true });

    const viewport = container.querySelector<HTMLElement>('.up-next-viewport')!;
    viewport.dispatchEvent(new KeyboardEvent('keydown', { key: 'Delete', bubbles: true, cancelable: true }));

    // The removal loop is a sequence of awaited calls (never a Promise.all —
    // each must land before the next fires, so an earlier removal's shift
    // never races a later one); draining it takes more than one tick.
    await vi.waitFor(() => expect(removeFromQueue).toHaveBeenCalledTimes(4));

    expect(vi.mocked(removeFromQueue).mock.calls.map((c) => c[0])).toEqual([4, 3, 2, 1]);
  });

  it('drags a row onto another and reorders by absolute queue index', async () => {
    seedMixedQueue();
    const { container } = render(UpNextPanel);
    const from = rowFor(container, 4);
    const onto = rowFor(container, 1);

    await fireEvent.dragStart(from);
    // jsdom's getBoundingClientRect is all-zero, so the drop always resolves
    // to "before" the hovered row — deterministic without mocking layout.
    await fireEvent.dragOver(onto);
    await fireEvent.drop(onto);

    expect(reorderQueue).toHaveBeenCalledWith(4, 1);
  });

  it('Clear calls clearQueue, and is disabled with nothing queued', async () => {
    const empty = render(UpNextPanel);
    expect(empty.container.querySelector<HTMLButtonElement>('.clear-btn')?.disabled).toBe(true);
    empty.unmount();

    seedMixedQueue();
    const { container } = render(UpNextPanel);
    const clearBtn = container.querySelector<HTMLButtonElement>('.clear-btn')!;
    expect(clearBtn.disabled).toBe(false);
    await fireEvent.click(clearBtn);
    expect(clearQueue).toHaveBeenCalledOnce();
  });
});
