import type { Album, Artist, Track } from '#lib/types/models.js';
import { getOfflineMode } from './offlineMode.svelte';

interface LibraryState {
  albums: Album[];
  artists: Artist[];
  tracks: Track[];
  isLoading: boolean;
  /**
   * True while any first-load library sync is running on an empty cache.
   * Independent of `isLoading` (which the set* actions clear): it drives a
   * "Setting up your library…" state so a freshly-connected/toggled backend
   * whose cache is still empty doesn't look broken during a long first sync.
   * Derived from `syncingCount` so overlapping backend syncs can't prematurely
   * clear it (a plain boolean would flash "empty" when the first of two
   * concurrent syncs settles).
   */
  isSyncing: boolean;
  /** Count of in-flight first-load syncs; `isSyncing = syncingCount > 0`. */
  syncingCount: number;
  lastSyncedAt: string | null;
  error: string | null;
}

const initialLibraryState: LibraryState = {
  albums: [],
  artists: [],
  tracks: [],
  isLoading: false,
  isSyncing: false,
  syncingCount: 0,
  lastSyncedAt: null,
  error: null,
};

let libraryState = $state<LibraryState>({ ...initialLibraryState });
let libraryVersion = $state(0);

export function getLibraryVersion(): number {
  return libraryVersion;
}

/** Get the library state */
export function getLibraryState(): LibraryState {
  return libraryState;
}

/** Total album count */
export function getAlbumCount(): number {
  return libraryState.albums.length;
}

/** Total artist count */
export function getArtistCount(): number {
  return libraryState.artists.length;
}

/** Total track count */
export function getTrackCount(): number {
  return libraryState.tracks.length;
}

/** Whether library is empty */
export function getIsEmpty(): boolean {
  return libraryState.albums.length === 0 && libraryState.artists.length === 0;
}

// Memoisation caches for offline-filtered getters.
// Keyed on (libraryVersion, offlineMode) instead of array-reference identity so that
// in-place mutations to tracks/albums (markTrackDownloaded, markTrackRemoved)
// correctly invalidate the cache after each mutator bumps libraryVersion. Array-
// reference keying would go permanently stale with in-place mutation because the
// reference never changes.
let cachedOfflineAlbums: Album[] | null = null;
let cachedOfflineAlbumsVersion: number = -1;
let cachedOfflineAlbumsMode: boolean | null = null;

let cachedOfflineArtists: Artist[] | null = null;
let cachedOfflineArtistsVersion: number = -1;
let cachedOfflineArtistsMode: boolean | null = null;

/** Albums filtered by offline mode; only albums with downloaded tracks when offline */
export function getOfflineFilteredAlbums(): Album[] {
  const offline = getOfflineMode();
  if (libraryVersion === cachedOfflineAlbumsVersion && offline === cachedOfflineAlbumsMode && cachedOfflineAlbums) {
    return cachedOfflineAlbums;
  }
  cachedOfflineAlbumsVersion = libraryVersion;
  cachedOfflineAlbumsMode = offline;
  cachedOfflineAlbums = offline ? libraryState.albums.filter((a) => a.downloadedTrackCount > 0) : libraryState.albums;
  return cachedOfflineAlbums;
}

/** Artists filtered by offline mode; only artists with downloaded tracks when offline */
export function getOfflineFilteredArtists(): Artist[] {
  const offline = getOfflineMode();
  if (libraryVersion === cachedOfflineArtistsVersion && offline === cachedOfflineArtistsMode && cachedOfflineArtists) {
    return cachedOfflineArtists;
  }
  cachedOfflineArtistsVersion = libraryVersion;
  cachedOfflineArtistsMode = offline;
  if (!offline) {
    cachedOfflineArtists = libraryState.artists;
  } else {
    const artistsWithDownloads = new Set(libraryState.tracks.filter((t) => t.cachedFilePath).map((t) => t.artistName));
    cachedOfflineArtists = libraryState.artists.filter((a) => artistsWithDownloads.has(a.name));
  }
  return cachedOfflineArtists;
}

// Actions
export function setLibraryLoading(isLoading: boolean): void {
  libraryState.isLoading = isLoading;
  libraryState.error = null;
}

/**
 * Mark the start of a first-load sync. Deliberately does NOT touch
 * `isLoading`/`error`; the `set*` actions clear `isLoading` when cache rows land
 * but a background first sync may still be in flight, which this tracks.
 * Reference-counted so overlapping backend syncs don't clear each other early.
 */
export function beginSyncing(): void {
  libraryState.syncingCount++;
  libraryState.isSyncing = true;
}

/** Mark the end of a first-load sync. Pairs 1:1 with `beginSyncing`. */
export function endSyncing(): void {
  libraryState.syncingCount = Math.max(0, libraryState.syncingCount - 1);
  libraryState.isSyncing = libraryState.syncingCount > 0;
}

export function setLibraryError(error: string): void {
  libraryState.isLoading = false;
  libraryState.error = error;
}

export function setAlbums(albums: Album[]): void {
  libraryState.albums = albums;
  libraryState.isLoading = false;
  libraryState.error = null;
  libraryVersion++;
}

export function setArtists(artists: Artist[]): void {
  libraryState.artists = artists;
  libraryState.isLoading = false;
  libraryState.error = null;
  libraryVersion++;
}

export function setTracks(tracks: Track[]): void {
  libraryState.tracks = tracks;
  libraryState.isLoading = false;
  libraryState.error = null;
  libraryVersion++;
}

export function setLibrarySynced(syncedAt: string): void {
  libraryState.lastSyncedAt = syncedAt;
  libraryState.isLoading = false;
  libraryState.error = null;
}

/**
 * Mark a track as downloaded in the in-memory library state.
 * Updates the track's cachedFilePath and increments the parent album's
 * downloadedTrackCount so UI indicators refresh without a full reload.
 */
export function markTrackDownloaded(trackId: string, filePath: string): void {
  const trackIdx = libraryState.tracks.findIndex((t) => t.jellyfinId === trackId);
  const track = trackIdx !== -1 ? libraryState.tracks[trackIdx] : undefined;
  if (!track || track.cachedFilePath) return;

  track.cachedFilePath = filePath;

  const albumIdx = libraryState.albums.findIndex((a) => a.jellyfinId === track.albumJellyfinId);
  const album = albumIdx !== -1 ? libraryState.albums[albumIdx] : undefined;
  if (album) {
    album.downloadedTrackCount++;
  }

  libraryVersion++;
}

/**
 * Mark a track as no longer downloaded (cache removed).
 * Clears cachedFilePath and decrements the parent album's downloadedTrackCount.
 */
export function markTrackRemoved(trackId: string): void {
  const trackIdx = libraryState.tracks.findIndex((t) => t.jellyfinId === trackId);
  const track = trackIdx !== -1 ? libraryState.tracks[trackIdx] : undefined;
  if (!track || !track.cachedFilePath) return;

  track.cachedFilePath = undefined;

  const albumIdx = libraryState.albums.findIndex((a) => a.jellyfinId === track.albumJellyfinId);
  const album = albumIdx !== -1 ? libraryState.albums[albumIdx] : undefined;
  if (album) {
    album.downloadedTrackCount = Math.max(0, album.downloadedTrackCount - 1);
  }

  libraryVersion++;
}

export function clearLibrary(): void {
  Object.assign(libraryState, initialLibraryState);
}
