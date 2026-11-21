import type { Track } from '#lib/types/models.js';
import type { ColumnBrowserData } from '#lib/services/tauri.js';
import { queryLibraryView } from '#lib/services/tauri.js';
import { getOfflineMode } from './offlineMode.svelte';

export interface ColumnBrowserState {
  selectedGenres: Set<string>;
  selectedArtists: Set<string>;
  selectedAlbums: Set<string>;
}

const initialState: ColumnBrowserState = {
  selectedGenres: new Set(),
  selectedArtists: new Set(),
  selectedAlbums: new Set(),
};

let columnBrowserState = $state<ColumnBrowserState>({ ...initialState });

export function getColumnBrowserState(): ColumnBrowserState {
  return columnBrowserState;
}

const emptyColumnBrowserData: ColumnBrowserData = {
  genres: [],
  artists: [],
  albums: [],
  totalTrackCount: 0,
};

let columnBrowserData = $state<ColumnBrowserData>(emptyColumnBrowserData);
let filteredTracks = $state<Track[]>([]);
let requestSeq = 0;

/** Get column browser data (genres, artists, albums, totalTrackCount). */
export function getColumnBrowserData(): ColumnBrowserData {
  return columnBrowserData;
}

/** Get filtered tracks from the last backend query. */
export function getFilteredTracks(): Track[] {
  return filteredTracks;
}

/**
 * Query the backend for column browser data and filtered tracks.
 * Uses requestSeq to discard stale responses from rapid interactions.
 */
export async function refreshLibraryView(sort?: string): Promise<void> {
  const seq = ++requestSeq;
  const { selectedGenres, selectedArtists, selectedAlbums } = columnBrowserState;

  const params: import('#lib/services/tauri.js').LibraryViewParams = {};
  if (selectedGenres.size > 0) params.genres = [...selectedGenres];
  if (selectedArtists.size > 0) params.artists = [...selectedArtists];
  if (selectedAlbums.size > 0) params.albumIds = [...selectedAlbums];
  // Only the user's own Offline Mode hides what isn't downloaded. A server
  // merely out of reach leaves the cached library browsable (undownloaded
  // tracks show greyed in TrackRow), never an empty list.
  if (getOfflineMode()) params.offlineOnly = true;
  if (sort) params.sort = sort;

  try {
    const result = await queryLibraryView(params);
    if (seq !== requestSeq) return; // stale response
    columnBrowserData = result.columnBrowser;
    filteredTracks = result.tracks;
  } catch (e) {
    console.error('Failed to query library view:', e);
  }
}

// Actions

export function selectGenre(genre: string, multiSelect: boolean): void {
  const newGenres = new Set(columnBrowserState.selectedGenres);

  if (multiSelect) {
    if (newGenres.has(genre)) {
      newGenres.delete(genre);
    } else {
      newGenres.add(genre);
    }
  } else {
    newGenres.clear();
    newGenres.add(genre);
  }

  columnBrowserState.selectedGenres = newGenres;
  columnBrowserState.selectedArtists = new Set();
  columnBrowserState.selectedAlbums = new Set();
}

export function selectArtist(artist: string, multiSelect: boolean): void {
  const newArtists = new Set(columnBrowserState.selectedArtists);

  if (multiSelect) {
    if (newArtists.has(artist)) {
      newArtists.delete(artist);
    } else {
      newArtists.add(artist);
    }
  } else {
    newArtists.clear();
    newArtists.add(artist);
  }

  columnBrowserState.selectedArtists = newArtists;
  columnBrowserState.selectedAlbums = new Set();
}

export function selectAlbum(albumId: string, multiSelect: boolean): void {
  const newAlbums = new Set(columnBrowserState.selectedAlbums);

  if (multiSelect) {
    if (newAlbums.has(albumId)) {
      newAlbums.delete(albumId);
    } else {
      newAlbums.add(albumId);
    }
  } else {
    newAlbums.clear();
    newAlbums.add(albumId);
  }

  columnBrowserState.selectedAlbums = newAlbums;
}

export function clearGenreSelection(): void {
  columnBrowserState.selectedGenres = new Set();
  columnBrowserState.selectedArtists = new Set();
  columnBrowserState.selectedAlbums = new Set();
}

export function clearArtistSelection(): void {
  columnBrowserState.selectedArtists = new Set();
  columnBrowserState.selectedAlbums = new Set();
}

export function clearAlbumSelection(): void {
  columnBrowserState.selectedAlbums = new Set();
}

export function clearAllSelections(): void {
  columnBrowserState.selectedGenres = new Set();
  columnBrowserState.selectedArtists = new Set();
  columnBrowserState.selectedAlbums = new Set();
}

/**
 * What the main view is currently showing, expressed in column-browser terms.
 *
 * `artist` is the artist NAME, not its Jellyfin id — the column browser keys
 * artists by `cached_tracks.artist_name`. That value is derived from Jellyfin's
 * AlbumArtist (see `JellyfinItem::primary_artist_name`), so an album's
 * `artistName` and its tracks' `artistName` always agree; selecting both an
 * album and its artist can never filter the track list to nothing.
 */
export interface ColumnBrowserSelection {
  genre?: string | null;
  artist?: string | null;
  albumId?: string | null;
}

/**
 * Make the column browser reflect what the view has navigated to.
 *
 * Every navigation entry point goes through here instead of pasting its own
 * `clearAllSelections(); selectX(...)` pair — that is how the browser ended up
 * following album clicks but not artist clicks, search results or playback.
 * Called with nothing, it resets to All / All / All.
 */
export function syncColumnBrowserToSelection(selection: ColumnBrowserSelection = {}): void {
  const genres = new Set<string>();
  const artists = new Set<string>();
  const albums = new Set<string>();

  if (selection.genre) genres.add(selection.genre);
  if (selection.artist) artists.add(selection.artist);
  if (selection.albumId) albums.add(selection.albumId);

  columnBrowserState.selectedGenres = genres;
  columnBrowserState.selectedArtists = artists;
  columnBrowserState.selectedAlbums = albums;
}

/** Serialisable copy of the current selection, for the navigation history. */
export interface ColumnBrowserSnapshot {
  genres: string[];
  artists: string[];
  albums: string[];
}

export function snapshotColumnBrowser(): ColumnBrowserSnapshot {
  return {
    genres: [...columnBrowserState.selectedGenres],
    artists: [...columnBrowserState.selectedArtists],
    albums: [...columnBrowserState.selectedAlbums],
  };
}

export function restoreColumnBrowser(snapshot: ColumnBrowserSnapshot): void {
  columnBrowserState.selectedGenres = new Set(snapshot.genres);
  columnBrowserState.selectedArtists = new Set(snapshot.artists);
  columnBrowserState.selectedAlbums = new Set(snapshot.albums);
}
