// Play Next / Add to Queue (the TrackContextMenu / ArtistAlbumSection context
// menu items): a user-added item goes through the exact same backend queue
// mechanism `startPlayback` uses internally (`add_to_queue`), just with
// position 'next' or 'end' — never a local queue mutation.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { Track } from '#lib/types/models.js';

const addToQueue = vi.fn();
const toastSuccess = vi.fn();
const toastError = vi.fn();

vi.mock('#lib/services/tauri.js', () => ({
  addToQueue: (...args: unknown[]) => addToQueue(...args),
}));

vi.mock('svelte-sonner', () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccess(...args),
    error: (...args: unknown[]) => toastError(...args),
  },
}));

const { queueTrackNext, queueTrackAtEnd } = await import('./queueActions');

function track(id: string): Track {
  return { jellyfinId: id, name: id } as unknown as Track;
}

describe('queueTrackNext', () => {
  beforeEach(() => {
    addToQueue.mockReset();
    toastSuccess.mockClear();
    toastError.mockClear();
  });

  it("queues the track at 'next' and confirms with a toast", async () => {
    addToQueue.mockResolvedValue(undefined);
    await queueTrackNext([track('a')]);
    expect(addToQueue).toHaveBeenCalledWith([track('a')], 'next');
    expect(toastSuccess).toHaveBeenCalledWith('Playing next', { description: 'a' });
    expect(toastError).not.toHaveBeenCalled();
  });

  it('reports failure with a toast instead of throwing', async () => {
    addToQueue.mockRejectedValue(new Error('backend unreachable'));
    await expect(queueTrackNext([track('a')])).resolves.toBeUndefined();
    expect(toastError).toHaveBeenCalled();
    expect(toastSuccess).not.toHaveBeenCalled();
  });

  it('a no-op with an empty list — never a zero-track queue call', async () => {
    await queueTrackNext([]);
    expect(addToQueue).not.toHaveBeenCalled();
  });

  it('describes several tracks by count, not the first name', async () => {
    addToQueue.mockResolvedValue(undefined);
    await queueTrackNext([track('a'), track('b'), track('c')]);
    expect(toastSuccess).toHaveBeenCalledWith('Playing next', { description: '3 tracks' });
  });
});

describe('queueTrackAtEnd', () => {
  beforeEach(() => {
    addToQueue.mockReset();
    toastSuccess.mockClear();
    toastError.mockClear();
  });

  it("queues the track at 'end' and confirms with a toast", async () => {
    addToQueue.mockResolvedValue(undefined);
    await queueTrackAtEnd([track('b')]);
    expect(addToQueue).toHaveBeenCalledWith([track('b')], 'end');
    expect(toastSuccess).toHaveBeenCalled();
  });
});
