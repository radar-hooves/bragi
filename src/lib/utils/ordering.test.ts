// Pins the sorting conventions, mirrored in
// src-tauri/src/ordering.rs — case/accent folding, natural numeric compare,
// leading-article strip for artists, track and artist-album tie-breaks.
import { describe, it, expect, vi } from 'vitest';
import { compareArtistAlbumOrder, compareArtistNames, compareFolded, compareTrackOrder, fold, stripLeadingArticle } from './ordering';
import ORDERING_FIXTURE from '../../../src-tauri/fixtures/ordering-names.json';

describe('fold', () => {
  it('folds case', () => {
    expect(fold('ABBA')).toBe(fold('abba'));
  });

  it('folds accents', () => {
    expect(fold('Beyoncé')).toBe(fold('Beyonce'));
    expect(fold('Sigur Rós')).toBe(fold('Sigur Ros'));
  });
});

describe('stripLeadingArticle', () => {
  it('drops "The", "A" and "An"', () => {
    expect(stripLeadingArticle('The Beatles')).toBe('Beatles');
    expect(stripLeadingArticle('A Tribe Called Quest')).toBe('Tribe Called Quest');
    expect(stripLeadingArticle('An Cafe')).toBe('Cafe');
  });

  it('leaves a name with no leading article untouched', () => {
    expect(stripLeadingArticle('Air')).toBe('Air');
    expect(stripLeadingArticle('A')).toBe('A');
  });
});

describe('compareFolded', () => {
  it('sorts numbers naturally, not lexicographically', () => {
    const titles = ['Track 10', 'Track 2', 'Track 1'];
    expect([...titles].sort(compareFolded)).toEqual(['Track 1', 'Track 2', 'Track 10']);
  });

  it('never strips an article — that is compareArtistNames-only', () => {
    expect(compareFolded('The Wall', 'Wall')).not.toBe(0);
  });

  it('compares digit runs by magnitude, ignoring leading zeros', () => {
    // A raw-length compare would put "Track 07" (2 chars) after "Track 8" (1
    // char) even though 7 < 8 — the bug this pins.
    expect([...['Track 8', 'Track 07']].sort(compareFolded)).toEqual(['Track 07', 'Track 8']);
    expect(compareFolded('Track 07', 'Track 8')).toBeLessThan(0);
  });

  it('breaks an equal-magnitude tie by zero count, for a total order', () => {
    expect(compareFolded('Track 7', 'Track 07')).toBeLessThan(0);
    expect(compareFolded('Track 07', 'Track 007')).toBeLessThan(0);
    expect(compareFolded('Track 07', 'Track 07')).toBe(0);
  });
});

// Pins the TypeScript comparator and the SQLite BRAGI_FOLD/BRAGI_ARTIST
// collations (src-tauri/src/ordering.rs) against one shared fixture, the
// same pattern settings-defaults.json uses for the TS/Rust settings defaults.
describe('ordering-names fixture (pinned against src-tauri/src/ordering.rs)', () => {
  it('compareFolded sorts the shared titles fixture into its expected order', () => {
    expect([...ORDERING_FIXTURE.titles].sort(compareFolded)).toEqual(ORDERING_FIXTURE.titlesSorted);
  });

  it('compareArtistNames sorts the shared artists fixture into its expected order', () => {
    expect([...ORDERING_FIXTURE.artists].sort((a, b) => compareArtistNames(a, b))).toEqual(ORDERING_FIXTURE.artistsSorted);
  });
});

describe('fold memoisation', () => {
  it('folds each distinct title once however many comparisons a sort makes (regression guard)', () => {
    // A sort chains several folded comparisons per pair, so an unmemoised fold
    // recomputed each title many times over (~2 s for 30,000 titles).
    // Counting folds, not timing them, keeps the guard exact on a loaded host.
    const titles = Array.from({ length: 5_000 }, (_, i) => `Track ${i} Memo Guard Title`);
    const normalize = vi.spyOn(String.prototype, 'normalize');
    try {
      [...titles].sort(compareFolded);
      expect(normalize).toHaveBeenCalledTimes(titles.length);
      normalize.mockClear();
      [...titles].sort(compareFolded);
      expect(normalize).not.toHaveBeenCalled();
    } finally {
      normalize.mockRestore();
    }
  });
});

describe('compareArtistNames', () => {
  it('ignores a leading article', () => {
    expect(compareArtistNames('The Beatles', 'Beatles')).toBe(0);
  });

  it('sorts a mixed set alphabetically once articles are ignored', () => {
    const names = ['The Black Keys', 'Air', 'A Tribe Called Quest'];
    expect([...names].sort((a, b) => compareArtistNames(a, b))).toEqual(['Air', 'The Black Keys', 'A Tribe Called Quest']);
  });

  it('prefers an explicit sortName over stripping the article itself', () => {
    // Un-stripped "Air" already sorts first; the point is the explicit
    // sortName is honoured rather than re-derived from the display name.
    expect(compareArtistNames('The Beatles', 'Air', 'Beatles', 'Air')).toBeGreaterThan(0);
  });
});

describe('compareTrackOrder', () => {
  it('orders disc then track number', () => {
    const tracks = [
      { discNumber: 2, trackNumber: 1, name: 'B' },
      { discNumber: 1, trackNumber: 2, name: 'A' },
      { discNumber: 1, trackNumber: 1, name: 'C' },
    ];
    expect([...tracks].sort(compareTrackOrder).map((t) => t.name)).toEqual(['C', 'A', 'B']);
  });

  it('breaks a disc/track tie by title', () => {
    const tracks = [
      { discNumber: 1, trackNumber: 1, name: 'Zeta' },
      { discNumber: 1, trackNumber: 1, name: 'Alpha' },
    ];
    expect([...tracks].sort(compareTrackOrder).map((t) => t.name)).toEqual(['Alpha', 'Zeta']);
  });
});

describe('compareArtistAlbumOrder', () => {
  it('orders an artists albums newest release first', () => {
    const albums = [
      { year: 1990, name: 'First' },
      { year: 2020, name: 'Latest' },
    ];
    expect([...albums].sort(compareArtistAlbumOrder).map((a) => a.name)).toEqual(['Latest', 'First']);
  });

  it('sinks a missing year to the end regardless of title', () => {
    const albums = [{ name: 'No Year' }, { year: 2000, name: 'Has Year' }];
    expect([...albums].sort(compareArtistAlbumOrder).map((a) => a.name)).toEqual(['Has Year', 'No Year']);
  });

  it('breaks an equal-year tie by folded title', () => {
    const albums = [
      { year: 2000, name: 'Zeta' },
      { year: 2000, name: 'Álpha' },
    ];
    expect([...albums].sort(compareArtistAlbumOrder).map((a) => a.name)).toEqual(['Álpha', 'Zeta']);
  });
});
