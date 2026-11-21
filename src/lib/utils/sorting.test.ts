// Guards the Recently Added ordering. The date-added tier only
// works because `dateCreated` now survives the trip from the media server; these
// pin the tie-breaking and the pre-v15 `undefined` case, which is what a
// partially-backfilled library actually looks like on first run.
//
// Also pins the sorting conventions: a leading article ignored
// for artists, case/accent folding and natural numeric compare for names.
import { describe, it, expect } from 'vitest';
import { sortAlbums, sortArtists, sortRecentlyAdded, sortTracks } from './sorting';
import type { Album, Artist, Track } from '#lib/types/models.js';

function album(name: string, dateCreated?: number, year?: number, artistName = 'Artist'): Album {
  return {
    jellyfinId: `id-${name}`,
    name,
    artistName,
    artistJellyfinId: 'artist-1',
    trackCount: 1,
    durationMs: 1000,
    downloadedTrackCount: 0,
    ...(dateCreated === undefined ? {} : { dateCreated }),
    ...(year === undefined ? {} : { year }),
  };
}

function artist(name: string, albumCount = 1, sortName?: string): Artist {
  return {
    jellyfinId: `artist-${name}`,
    name,
    albumCount,
    ...(sortName === undefined ? {} : { sortName }),
  };
}

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

const DAY = 86_400;

describe('sortRecentlyAdded', () => {
  it('puts the newest date-added first, ignoring alphabetical order', () => {
    // Names run opposite to dates, so a fallback to name sorting would fail here.
    const albums = [album('A', 100 * DAY), album('Z', 200 * DAY)];

    const sorted = sortRecentlyAdded(albums, 'date-added-desc');

    expect(sorted.map((a) => a.name)).toEqual(['Z', 'A']);
  });

  it('reverses under date-added-asc', () => {
    const albums = [album('Z', 200 * DAY), album('A', 100 * DAY)];

    expect(sortRecentlyAdded(albums, 'date-added-asc').map((a) => a.name)).toEqual(['A', 'Z']);
  });

  it('sinks albums with no dateCreated to the end under both date orders', () => {
    // Pre-v15 rows carry no date. Treating a missing date as 0 would float them
    // to the top of the ascending sort and bury real albums.
    const albums = [album('NoDate'), album('Dated', 100 * DAY)];

    expect(sortRecentlyAdded(albums, 'date-added-desc').map((a) => a.name)).toEqual(['Dated', 'NoDate']);
    expect(sortRecentlyAdded(albums, 'date-added-asc').map((a) => a.name)).toEqual(['Dated', 'NoDate']);
  });

  it('breaks date ties by name', () => {
    const albums = [album('B', 100 * DAY), album('A', 100 * DAY)];

    expect(sortRecentlyAdded(albums, 'date-added-desc').map((a) => a.name)).toEqual(['A', 'B']);
  });

  it('sorts by release year independently of date added', () => {
    // Newest addition is the oldest release: year sorting must not track dates.
    const albums = [album('Old', 200 * DAY, 1985), album('New', 100 * DAY, 2024)];

    expect(sortRecentlyAdded(albums, 'year-desc').map((a) => a.name)).toEqual(['New', 'Old']);
    expect(sortRecentlyAdded(albums, 'year-asc').map((a) => a.name)).toEqual(['Old', 'New']);
  });

  it('does not mutate the input array', () => {
    const albums = [album('Z', 200 * DAY), album('A', 100 * DAY)];

    sortRecentlyAdded(albums, 'date-added-asc');

    expect(albums.map((a) => a.name)).toEqual(['Z', 'A']);
  });

  it('serves a memoised result for the same input and sort key', () => {
    const albums = [album('A', 100 * DAY), album('Z', 200 * DAY)];

    expect(sortRecentlyAdded(albums, 'date-added-desc')).toBe(sortRecentlyAdded(albums, 'date-added-desc'));
  });
});

describe('sortArtists', () => {
  it('ignores a leading "The" when sorting by name', () => {
    const artists = [artist('The Beatles'), artist('Air')];

    expect(sortArtists(artists, 'name-asc').map((a) => a.name)).toEqual(['Air', 'The Beatles']);
  });

  it('prefers an explicit sortName over stripping the article itself', () => {
    const artists = [artist('The Beatles', 1, 'Beatles'), artist('Air', 1, 'Air')];

    expect(sortArtists(artists, 'name-asc').map((a) => a.name)).toEqual(['Air', 'The Beatles']);
  });

  it('breaks an albums-count tie by name, ignoring the article', () => {
    const artists = [artist('The Beatles', 5), artist('Air', 5)];

    expect(sortArtists(artists, 'albums-desc').map((a) => a.name)).toEqual(['Air', 'The Beatles']);
  });
});

describe('sortAlbums', () => {
  it('folds accents when sorting by name', () => {
    const albums = [album('Beyoncé'), album('Aardvark')];

    expect(sortAlbums(albums, 'name-asc').map((a) => a.name)).toEqual(['Aardvark', 'Beyoncé']);
  });

  it('ignores the artist article when sorting by artist', () => {
    const albums = [album('X', undefined, undefined, 'The Beatles'), album('Y', undefined, undefined, 'Air')];

    expect(sortAlbums(albums, 'artist-asc').map((a) => a.name)).toEqual(['Y', 'X']);
  });
});

describe('sortTracks', () => {
  it('breaks a track-number tie by disc, not just track number', () => {
    // Same track number on two different discs must not collide.
    const tracks = [track({ jellyfinId: 'd2t1', albumName: 'Same Album', discNumber: 2, trackNumber: 1 }), track({ jellyfinId: 'd1t1', albumName: 'Same Album', discNumber: 1, trackNumber: 1 })];

    expect(sortTracks(tracks, 'track-number').map((t) => t.jellyfinId)).toEqual(['d1t1', 'd2t1']);
  });

  it('sorts embedded numbers naturally when sorting by artist', () => {
    const tracks = [track({ jellyfinId: 'blink10', artistName: 'Blink-10' }), track({ jellyfinId: 'blink2', artistName: 'Blink-2' })];

    expect(sortTracks(tracks, 'artist-asc').map((t) => t.jellyfinId)).toEqual(['blink2', 'blink10']);
  });
});
