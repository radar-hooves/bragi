// Pins the one search ranking: match tier (exact > prefix >
// contains), the ordering.ts fold within a tier, and the top-result pick.
import { describe, it, expect } from 'vitest';
import type { Album, Artist, Track } from '#lib/types/models.js';
import { rankSearchResults } from './searchRanking';

function track(name: string, artistName = 'Someone'): Track {
  return { jellyfinId: name, name, artistName } as unknown as Track;
}
function album(name: string, artistName = 'Someone'): Album {
  return { jellyfinId: name, name, artistName } as unknown as Album;
}
function artist(name: string, sortName?: string): Artist {
  return { jellyfinId: name, name, sortName } as unknown as Artist;
}

describe('rankSearchResults — match tier', () => {
  it('ranks an exact match ahead of a prefix match ahead of a mid-string hit', () => {
    const r = rankSearchResults('rock', {
      tracks: [track('Hard Rock Cafe'), track('Rocketeer'), track('Rock')],
      albums: [],
      artists: [],
    });
    expect(r.tracks.map((t) => t.name)).toEqual(['Rock', 'Rocketeer', 'Hard Rock Cafe']);
  });

  it('folds case and accents before matching', () => {
    const r = rankSearchResults('BEYONCE', { tracks: [], albums: [], artists: [artist('Beyoncé')] });
    expect(r.artists.map((a) => a.name)).toEqual(['Beyoncé']);
  });

  it('drops a candidate that matches neither the primary name nor (for tracks/albums) the artist', () => {
    const r = rankSearchResults('zzz', { tracks: [track('Rock')], albums: [], artists: [] });
    expect(r.tracks).toEqual([]);
  });

  it('matches a track or album by artist name as well as by title', () => {
    const r = rankSearchResults('adele', {
      tracks: [track('Someone Like You', 'Adele')],
      albums: [album('19', 'Adele')],
      artists: [],
    });
    expect(r.tracks).toHaveLength(1);
    expect(r.albums).toHaveLength(1);
  });

  it('breaks a tied tier with the shared folded comparator, natural-numeric and all', () => {
    const r = rankSearchResults('track', {
      tracks: [track('Track 10'), track('Track 2'), track('Track 1')],
      albums: [],
      artists: [],
    });
    expect(r.tracks.map((t) => t.name)).toEqual(['Track 1', 'Track 2', 'Track 10']);
  });
});

describe('rankSearchResults — artist article stripping', () => {
  it('ranks "The Beatles" as an exact match for the query "beatles"', () => {
    const r = rankSearchResults('beatles', { tracks: [], albums: [], artists: [artist('The Beatles')] });
    expect(r.artists.map((a) => a.name)).toEqual(['The Beatles']);
  });

  it('never strips an article for a track or album title (ordering.ts scope)', () => {
    const r = rankSearchResults('wall', {
      tracks: [track('The Wall', 'Pink Floyd'), track('Wall', 'Someone Else')],
      albums: [],
      artists: [],
    });
    // "Wall" is an exact title match; "The Wall" only a prefix-after-strip
    // would not apply to a title, so it ranks by contains/prefix on the raw name.
    expect(r.tracks[0]!.name).toBe('Wall');
  });
});

describe('rankSearchResults — empty query', () => {
  it('returns every group empty and no top result', () => {
    const r = rankSearchResults('   ', { tracks: [track('Rock')], albums: [], artists: [] });
    expect(r).toEqual({ tracks: [], albums: [], artists: [], topResult: null });
  });
});

describe('rankSearchResults — top result', () => {
  it('picks an exact artist match over a prefix album or track match', () => {
    const r = rankSearchResults('air', {
      tracks: [track('Airport')],
      albums: [album('Airwaves')],
      artists: [artist('Air')],
    });
    expect(r.topResult).toEqual({ type: 'artist', item: r.artists[0] });
  });

  it('falls back to an album, then a track, when no artist matches', () => {
    const albumOnly = rankSearchResults('air', { tracks: [], albums: [album('Air')], artists: [] });
    expect(albumOnly.topResult).toEqual({ type: 'album', item: albumOnly.albums[0] });

    const trackOnly = rankSearchResults('air', { tracks: [track('Air')], albums: [], artists: [] });
    expect(trackOnly.topResult).toEqual({ type: 'track', item: trackOnly.tracks[0] });
  });

  it('gives no top result when only a weak mid-string match exists', () => {
    const r = rankSearchResults('air', { tracks: [track('Repair Shop')], albums: [], artists: [] });
    expect(r.tracks).toHaveLength(1); // still a result, just not confident enough to headline
    expect(r.topResult).toBeNull();
  });

  it('gives no top result when nothing matched at all', () => {
    const r = rankSearchResults('zzz', { tracks: [], albums: [], artists: [] });
    expect(r.topResult).toBeNull();
  });
});
