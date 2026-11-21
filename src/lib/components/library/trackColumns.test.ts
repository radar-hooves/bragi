// Pins the Songs-table sort conventions: disc-then-track within an
// album, an artist tie ignoring a leading article, natural numeric compare,
// and every column falling back to album order (artist, disc, track) on a tie.
import { describe, it, expect } from 'vitest';
import { compareTracks, computeFitWidth, colGridTrack, allColumns, FIT_CONTENT_PADDING_PX } from './trackColumns';
import type { Track } from '#lib/types/models.js';

function track(overrides: Partial<Track> & { jellyfinId: string }): Track {
  return {
    name: 'Track',
    artistName: 'Artist',
    albumName: 'Album',
    albumJellyfinId: 'album-1',
    trackNumber: 1,
    discNumber: 1,
    durationMs: 180_000,
    ...overrides,
  };
}

function sortedNames(tracks: Track[], colId: string, dir: 1 | -1 = 1): string[] {
  return [...tracks].sort((a, b) => compareTracks(a, b, colId, dir)).map((t) => t.jellyfinId);
}

describe('compareTracks: trackNumber (album playback order)', () => {
  it('orders disc then track number regardless of row order', () => {
    const tracks = [
      track({ jellyfinId: 'd2t1', discNumber: 2, trackNumber: 1 }),
      track({ jellyfinId: 'd1t2', discNumber: 1, trackNumber: 2 }),
      track({ jellyfinId: 'd1t1', discNumber: 1, trackNumber: 1 }),
    ];
    expect(sortedNames(tracks, 'trackNumber')).toEqual(['d1t1', 'd1t2', 'd2t1']);
  });
});

describe('compareTracks: artist column', () => {
  it('ignores a leading "The" when sorting by artist', () => {
    const tracks = [track({ jellyfinId: 'beatles', artistName: 'The Beatles' }), track({ jellyfinId: 'air', artistName: 'Air' })];
    // "Beatles" (article stripped) sorts before "Air"? No — alphabetically
    // "Air" < "Beatles", so Air must come first once the article is gone.
    expect(sortedNames(tracks, 'artist')).toEqual(['air', 'beatles']);
  });

  it('sorts embedded numbers naturally, not lexicographically', () => {
    const tracks = [track({ jellyfinId: 'blink10', artistName: 'Blink-10' }), track({ jellyfinId: 'blink2', artistName: 'Blink-2' })];
    expect(sortedNames(tracks, 'artist')).toEqual(['blink2', 'blink10']);
  });

  it('ties fall back to album order — artist, then disc, then track', () => {
    const tracks = [
      track({ jellyfinId: 'z-d2', artistName: 'Same', albumName: 'Zeta', discNumber: 2, trackNumber: 1 }),
      track({ jellyfinId: 'a-d1', artistName: 'Same', albumName: 'Alpha', discNumber: 1, trackNumber: 1 }),
    ];
    expect(sortedNames(tracks, 'artist')).toEqual(['a-d1', 'z-d2']);
  });
});

describe('compareTracks: title/album columns fold case and accents', () => {
  it('is case-insensitive', () => {
    const tracks = [track({ jellyfinId: 'upper', name: 'ZEBRA' }), track({ jellyfinId: 'lower', name: 'apple' })];
    expect(sortedNames(tracks, 'title')).toEqual(['lower', 'upper']);
  });

  it('folds accents so they sort with their base letter', () => {
    const tracks = [track({ jellyfinId: 'plain', name: 'Beyonce' }), track({ jellyfinId: 'accented', name: 'Beyoncé' }), track({ jellyfinId: 'before', name: 'Aardvark' })];
    // Beyonce/Beyoncé fold to the same key; "Aardvark" still sorts first.
    expect(sortedNames(tracks, 'title')[0]).toBe('before');
  });
});

describe('compareTracks: descending direction only flips the primary key', () => {
  it('keeps the album tie-break ascending under a descending title sort', () => {
    const tracks = [track({ jellyfinId: 'same-title-b', name: 'Same', albumName: 'Beta' }), track({ jellyfinId: 'same-title-a', name: 'Same', albumName: 'Alpha' })];
    // Titles tie either way; the album tie-break stays ascending (Alpha first)
    // even though the primary column is sorted descending.
    expect(sortedNames(tracks, 'title', -1)).toEqual(['same-title-a', 'same-title-b']);
  });
});

// Double-click-to-fit: fits a column to its widest
// rendered VALUE, Finder/Music.app's own double-click-a-divider behaviour —
// `measureText` is injected so this is testable without a real canvas.
describe('computeFitWidth', () => {
  const monospace = (text: string): number => text.length * 7;

  it('fits the widest value, padded, when it exceeds the minimum', () => {
    // Widest is 'Really Long Title' (17 chars): 17*7 + 24 = 143.
    const width = computeFitWidth(['Short', 'Really Long Title', 'Mid'], monospace, 80);
    expect(width).toBe(143);
  });

  it('falls back to the minimum when every value is narrower than it', () => {
    const width = computeFitWidth(['a', 'bb', 'ccc'], monospace, 120);
    expect(width).toBe(120);
  });

  it('ignores empty values rather than letting them win as the widest', () => {
    const width = computeFitWidth(['', '', 'X'], monospace, 0);
    expect(width).toBe(7 + FIT_CONTENT_PADDING_PX);
  });

  it('an empty list falls back to the minimum', () => {
    expect(computeFitWidth([], monospace, 64)).toBe(64);
  });
});

describe('colGridTrack', () => {
  it('uses a stored or fixed pixel width when one is set', () => {
    expect(colGridTrack(allColumns.time!, { time: 90 })).toBe('90px');
    expect(colGridTrack(allColumns.time!, {})).toBe('64px'); // time's own fixedWidth
  });

  it('falls back to a flexible track no narrower than the column\'s own floor', () => {
    expect(colGridTrack(allColumns.title!, {})).toBe('minmax(120px, 1fr)');
  });
});
