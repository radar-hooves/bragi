import type { Album, Artist, Track } from '#lib/types/models.js';
import type { AlbumsSort, ArtistsSort, RecentlyAddedSort, TracksSort } from '#lib/stores/ui.svelte.js';
import { compareArtistNames, compareFolded } from '#lib/utils/ordering.js';

export interface SortOption {
  value: string;
  label: string;
}

export const albumsSortOptions: SortOption[] = [
  { value: 'year-desc', label: 'Year (Newest)' },
  { value: 'year-asc', label: 'Year (Oldest)' },
  { value: 'name-asc', label: 'Name (A-Z)' },
  { value: 'name-desc', label: 'Name (Z-A)' },
  { value: 'artist-asc', label: 'Artist (A-Z)' },
];

export const artistsSortOptions: SortOption[] = [
  { value: 'name-asc', label: 'Name (A-Z)' },
  { value: 'name-desc', label: 'Name (Z-A)' },
  { value: 'albums-desc', label: 'Albums (Most)' },
  { value: 'albums-asc', label: 'Albums (Least)' },
];

export const genresSortOptions: SortOption[] = [
  { value: 'name-asc', label: 'Name (A-Z)' },
  { value: 'name-desc', label: 'Name (Z-A)' },
  { value: 'tracks-desc', label: 'Songs (Most)' },
  { value: 'time-desc', label: 'Total time (Longest)' },
];

export const artistAlbumsSortOptions: SortOption[] = [
  { value: 'year-desc', label: 'Year (Newest)' },
  { value: 'year-asc', label: 'Year (Oldest)' },
  { value: 'name-asc', label: 'Name (A-Z)' },
  { value: 'name-desc', label: 'Name (Z-A)' },
];

export const recentlyAddedSortOptions: SortOption[] = [
  { value: 'date-added-desc', label: 'Date Added (Newest)' },
  { value: 'date-added-asc', label: 'Date Added (Oldest)' },
  { value: 'year-desc', label: 'Year (Newest)' },
  { value: 'year-asc', label: 'Year (Oldest)' },
  { value: 'name-asc', label: 'Name (A-Z)' },
];

export const tracksSortOptions: SortOption[] = [
  { value: 'track-number', label: 'Track Number' },
  { value: 'name-asc', label: 'Name (A-Z)' },
  { value: 'name-desc', label: 'Name (Z-A)' },
  { value: 'artist-asc', label: 'Artist (A-Z)' },
  { value: 'album-asc', label: 'Album (A-Z)' },
  { value: 'duration-asc', label: 'Duration (Short)' },
  { value: 'duration-desc', label: 'Duration (Long)' },
];

// Sort memoisation: avoid re-sorting when the input data and sort key haven't changed.
// Each sort function caches its last result by input reference + sort key.
let albumsSortCache: { ref: unknown; sort: string; result: Album[] } = { ref: null, sort: '', result: [] };
let artistsSortCache: { ref: unknown; sort: string; result: Artist[] } = { ref: null, sort: '', result: [] };
let tracksSortCache: { ref: unknown; sort: string; result: Track[] } = { ref: null, sort: '', result: [] };
let recentlyAddedSortCache: { ref: unknown; sort: string; result: Album[] } = { ref: null, sort: '', result: [] };

export function sortAlbums(albums: Album[], sort: AlbumsSort): Album[] {
  if (albums === albumsSortCache.ref && sort === albumsSortCache.sort) return albumsSortCache.result;
  const sorted = [...albums];
  switch (sort) {
    case 'year-desc':
      sorted.sort((a, b) => (b.year ?? 0) - (a.year ?? 0) || compareFolded(a.name, b.name));
      break;
    case 'year-asc':
      sorted.sort((a, b) => (a.year ?? 9999) - (b.year ?? 9999) || compareFolded(a.name, b.name));
      break;
    case 'name-asc':
      sorted.sort((a, b) => compareFolded(a.name, b.name));
      break;
    case 'name-desc':
      sorted.sort((a, b) => compareFolded(b.name, a.name));
      break;
    case 'artist-asc':
      sorted.sort((a, b) => compareArtistNames(a.artistName, b.artistName) || compareFolded(a.name, b.name));
      break;
  }
  albumsSortCache = { ref: albums, sort, result: sorted };
  return sorted;
}

/**
 * Sort the Recently Added grid.
 *
 * `dateCreated` is absent on albums cached before schema v15. Those sort
 * to the end under either date order rather than jumping to the top as a 0
 * would, so a partially-backfilled library still reads sensibly.
 */
export function sortRecentlyAdded(albums: Album[], sort: RecentlyAddedSort): Album[] {
  if (albums === recentlyAddedSortCache.ref && sort === recentlyAddedSortCache.sort) {
    return recentlyAddedSortCache.result;
  }
  const sorted = [...albums];
  switch (sort) {
    case 'date-added-desc':
      sorted.sort((a, b) => (b.dateCreated ?? -Infinity) - (a.dateCreated ?? -Infinity) || compareFolded(a.name, b.name));
      break;
    case 'date-added-asc':
      sorted.sort((a, b) => (a.dateCreated ?? Infinity) - (b.dateCreated ?? Infinity) || compareFolded(a.name, b.name));
      break;
    case 'year-desc':
      sorted.sort((a, b) => (b.year ?? 0) - (a.year ?? 0) || compareFolded(a.name, b.name));
      break;
    case 'year-asc':
      sorted.sort((a, b) => (a.year ?? 9999) - (b.year ?? 9999) || compareFolded(a.name, b.name));
      break;
    case 'name-asc':
      sorted.sort((a, b) => compareFolded(a.name, b.name));
      break;
  }
  recentlyAddedSortCache = { ref: albums, sort, result: sorted };
  return sorted;
}

export function sortArtists(artists: Artist[], sort: ArtistsSort): Artist[] {
  if (artists === artistsSortCache.ref && sort === artistsSortCache.sort) return artistsSortCache.result;
  const sorted = [...artists];
  const byName = (a: Artist, b: Artist) => compareArtistNames(a.name, b.name, a.sortName, b.sortName);
  switch (sort) {
    case 'name-asc':
      sorted.sort(byName);
      break;
    case 'name-desc':
      sorted.sort((a, b) => byName(b, a));
      break;
    case 'albums-desc':
      sorted.sort((a, b) => b.albumCount - a.albumCount || byName(a, b));
      break;
    case 'albums-asc':
      sorted.sort((a, b) => a.albumCount - b.albumCount || byName(a, b));
      break;
  }
  artistsSortCache = { ref: artists, sort, result: sorted };
  return sorted;
}

export function sortTracks(tracks: Track[], sort: TracksSort): Track[] {
  if (tracks === tracksSortCache.ref && sort === tracksSortCache.sort) return tracksSortCache.result;
  const sorted = [...tracks];
  switch (sort) {
    case 'track-number':
      sorted.sort((a, b) => compareFolded(a.albumName, b.albumName) || a.discNumber - b.discNumber || a.trackNumber - b.trackNumber);
      break;
    case 'name-asc':
      sorted.sort((a, b) => compareFolded(a.name, b.name));
      break;
    case 'name-desc':
      sorted.sort((a, b) => compareFolded(b.name, a.name));
      break;
    case 'artist-asc':
      sorted.sort((a, b) => compareArtistNames(a.artistName, b.artistName) || compareFolded(a.name, b.name));
      break;
    case 'album-asc':
      sorted.sort((a, b) => compareFolded(a.albumName, b.albumName) || a.discNumber - b.discNumber || a.trackNumber - b.trackNumber);
      break;
    case 'duration-asc':
      sorted.sort((a, b) => a.durationMs - b.durationMs);
      break;
    case 'duration-desc':
      sorted.sort((a, b) => b.durationMs - a.durationMs);
      break;
  }
  tracksSortCache = { ref: tracks, sort, result: sorted };
  return sorted;
}
