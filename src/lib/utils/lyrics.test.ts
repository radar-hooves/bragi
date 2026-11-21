import { describe, it, expect } from 'vitest';
import { activeLyricIndex, resolveActiveLyricIndex } from './lyrics';
import type { LyricsLine, TrackLyrics } from '#lib/types/models.js';

const synced = (starts: number[]): TrackLyrics => ({
  synced: true,
  lines: starts.map((startMs, i) => ({ startMs, text: `line ${i}` }) satisfies LyricsLine),
});

describe('activeLyricIndex', () => {
  const lines: LyricsLine[] = [
    { startMs: 0, text: 'a' },
    { startMs: 1000, text: 'b' },
    { startMs: 2000, text: 'c' },
  ];

  it('returns -1 before the first line', () => {
    expect(activeLyricIndex([{ startMs: 500, text: 'x' }], 0)).toBe(-1);
  });

  it('treats a 0ms first line as active-able', () => {
    expect(activeLyricIndex(lines, 0)).toBe(0);
  });

  it('picks the last line at or before the position', () => {
    expect(activeLyricIndex(lines, 1500)).toBe(1);
    expect(activeLyricIndex(lines, 2000)).toBe(2);
    expect(activeLyricIndex(lines, 9999)).toBe(2);
  });
});

describe('resolveActiveLyricIndex (offset)', () => {
  it('returns -1 for null or empty lyrics', () => {
    expect(resolveActiveLyricIndex(null, 5000, 200_000, 0)).toBe(-1);
    expect(resolveActiveLyricIndex({ synced: true, lines: [] }, 5000, 200_000, 0)).toBe(-1);
  });

  it('applies no offset when 0 (baseline)', () => {
    const l = synced([0, 1000, 2000, 3000]);
    expect(resolveActiveLyricIndex(l, 1500, 200_000, 0)).toBe(1);
  });

  it('a positive offset advances the highlight (lyrics earlier)', () => {
    const l = synced([0, 1000, 2000, 3000]);
    // At 900ms with +250ms we evaluate at 1150ms -> line 1 (would be line 0 at 0 offset).
    expect(resolveActiveLyricIndex(l, 900, 200_000, 0)).toBe(0);
    expect(resolveActiveLyricIndex(l, 900, 200_000, 250)).toBe(1);
  });

  it('a negative offset delays the highlight (lyrics later)', () => {
    const l = synced([0, 1000, 2000, 3000]);
    // At 1100ms with -250ms we evaluate at 850ms -> back to line 0.
    expect(resolveActiveLyricIndex(l, 1100, 200_000, 0)).toBe(1);
    expect(resolveActiveLyricIndex(l, 1100, 200_000, -250)).toBe(0);
  });

  it('can go before the first line with a large negative offset', () => {
    const l = synced([0, 1000, 2000]);
    expect(resolveActiveLyricIndex(l, 500, 200_000, -1000)).toBe(-1);
  });

  it('ignores the offset for unsynced lyrics (progress estimate)', () => {
    const unsynced: TrackLyrics = {
      synced: false,
      lines: [{ text: 'a' }, { text: 'b' }, { text: 'c' }, { text: 'd' }],
    };
    // 50% of a 4-line track -> index 2, regardless of the offset.
    expect(resolveActiveLyricIndex(unsynced, 5000, 10_000, 0)).toBe(2);
    expect(resolveActiveLyricIndex(unsynced, 5000, 10_000, 5000)).toBe(2);
  });

  it('returns -1 for unsynced lyrics with unknown duration', () => {
    const unsynced: TrackLyrics = { synced: false, lines: [{ text: 'a' }] };
    expect(resolveActiveLyricIndex(unsynced, 5000, 0, 0)).toBe(-1);
  });
});
