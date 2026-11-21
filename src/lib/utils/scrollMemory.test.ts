import { describe, it, expect, beforeEach, vi } from 'vitest';
import { bindScrollMemory, clearScrollMemory, getSavedScrollTop, saveScrollTop } from './scrollMemory.svelte';

function fakeScrollable(initialTop = 0) {
  const el = { scrollTop: initialTop, isConnected: true, addEventListener: vi.fn(), removeEventListener: vi.fn() };
  return el as unknown as HTMLElement & { scrollTop: number };
}

describe('bindScrollMemory', () => {
  beforeEach(() => clearScrollMemory());

  it('restores nothing (starts at 0) the first time a key is seen', () => {
    const el = fakeScrollable(500); // some stale value from a previous element
    bindScrollMemory('tracks', el);
    expect(el.scrollTop).toBe(0);
  });

  it('restores a previously saved position for the same key', () => {
    const first = fakeScrollable(0);
    const cleanupFirst = bindScrollMemory('albums', first);
    first.scrollTop = 340;
    cleanupFirst?.(); // simulates leaving the view — saves on the way out

    const second = fakeScrollable(0);
    bindScrollMemory('albums', second);
    expect(second.scrollTop).toBe(340);
  });

  it('keeps a different key at its own position — opening a different item resets', () => {
    const albumA = fakeScrollable(0);
    const cleanupA = bindScrollMemory('album:aaa', albumA);
    albumA.scrollTop = 200;
    cleanupA?.(); // leaving album A saves its position

    const albumB = fakeScrollable(0);
    bindScrollMemory('album:bbb', albumB);
    expect(albumB.scrollTop).toBe(0); // a different album starts fresh

    const backToA = fakeScrollable(0);
    bindScrollMemory('album:aaa', backToA);
    expect(backToA.scrollTop).toBe(200); // returning to album A resumes it
  });

  it('saves on scroll (rAF-coalesced) without waiting for cleanup', async () => {
    vi.useFakeTimers();
    const el = fakeScrollable(0);
    bindScrollMemory('genres', el);
    const onScroll = (el.addEventListener as ReturnType<typeof vi.fn>).mock.calls.find((c) => c[0] === 'scroll')?.[1] as () => void;
    expect(onScroll).toBeInstanceOf(Function);

    el.scrollTop = 150;
    onScroll();
    // rAF-coalesced: fire twice before the frame, only one save should be pending.
    el.scrollTop = 175;
    onScroll();
    await vi.runAllTimersAsync();

    const other = fakeScrollable(0);
    bindScrollMemory('genres', other);
    expect(other.scrollTop).toBe(175);
    vi.useRealTimers();
  });

  it('does a final save on cleanup even if no scroll event fired since the last one', () => {
    const el = fakeScrollable(0);
    const cleanup = bindScrollMemory('playlist:xyz', el);
    el.scrollTop = 90; // e.g. set programmatically, no native scroll event in this test
    cleanup?.();

    const reopened = fakeScrollable(0);
    bindScrollMemory('playlist:xyz', reopened);
    expect(reopened.scrollTop).toBe(90);
  });

  it('retains the last visible position when teardown follows DOM removal', () => {
    const el = document.createElement('div');
    document.body.append(el);
    const cleanup = bindScrollMemory('artist', el);
    el.scrollTop = 560;
    el.dispatchEvent(new Event('scroll'));
    el.remove();
    el.scrollTop = 0;
    cleanup?.();
    expect(getSavedScrollTop('artist')).toBe(560);
  });

  it('saves user scrolling during loading without restoring into an empty view', () => {
    saveScrollTop('artist', 560);
    const el = document.createElement('div');
    document.body.append(el);
    const cleanup = bindScrollMemory('artist', el, false);
    expect(el.scrollTop).toBe(0);
    el.scrollTop = 320;
    el.dispatchEvent(new Event('scroll'));
    el.remove();
    el.scrollTop = 0;
    cleanup?.();
    expect(getSavedScrollTop('artist')).toBe(320);
  });

  it('retains the saved position until loading finishes when nobody scrolls', () => {
    saveScrollTop('artist', 560);
    const el = fakeScrollable();
    bindScrollMemory('artist', el, false)?.();
    expect(getSavedScrollTop('artist')).toBe(560);
    bindScrollMemory('artist', el, true)?.();
    expect(el.scrollTop).toBe(560);
  });

  it('does nothing when the element is not yet mounted', () => {
    expect(bindScrollMemory('tracks', undefined)).toBeUndefined();
  });
});
