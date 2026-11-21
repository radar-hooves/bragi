/**
 * Jellyfin playlist state management store.
 * Handles playlist CRUD operations with Jellyfin server.
 *
 * Unlike the music service playlists, Jellyfin playlists sync bidirectionally
 * so other Jellyfin apps (Finamp, Symfonium, etc.) see the changes.
 */

import type { JellyfinPlaylist, JellyfinPlaylistDetail } from '#lib/types/models.js';
import {
  getJellyfinPlaylists as fetchPlaylists,
  createJellyfinPlaylist as apiCreatePlaylist,
  getJellyfinPlaylist as fetchPlaylist,
  getCachedJellyfinPlaylist as fetchCachedPlaylist,
  deleteJellyfinPlaylist as apiDeletePlaylist,
  renameJellyfinPlaylist as apiRenamePlaylist,
  addTracksToJellyfinPlaylist as apiAddTracks,
  removeTracksFromJellyfinPlaylist as apiRemoveTracks,
  moveJellyfinPlaylistItem as apiMoveItem,
} from '#lib/services/tauri.js';

// ============================================================================
// State Types
// ============================================================================

interface JellyfinPlaylistState {
  /** List of all playlists (summary only). */
  playlists: JellyfinPlaylist[];
  /** Currently selected playlist ID. */
  selectedPlaylistId: string | null;
  /** Currently selected playlist with full details. */
  currentPlaylist: JellyfinPlaylistDetail | null;
  /** Whether playlists are being loaded. */
  isLoading: boolean;
  /** Whether a playlist operation is in progress. */
  isOperating: boolean;
  /** Error message if something failed. */
  error: string | null;
}

// ============================================================================
// Store
// ============================================================================

const initialState: JellyfinPlaylistState = {
  playlists: [],
  selectedPlaylistId: null,
  currentPlaylist: null,
  isLoading: false,
  isOperating: false,
  error: null,
};

let jellyfinPlaylistState = $state<JellyfinPlaylistState>({ ...initialState });

/**
 * In-memory cache of previously-loaded playlist details, keyed by Jellyfin
 * playlist ID. Enables stale-while-revalidate: re-opening a playlist renders
 * the cached detail instantly while a fresh copy is fetched in the background.
 * Not reactive on purpose — it is a plain data store, not view state.
 */
const playlistDetailCache = new Map<string, JellyfinPlaylistDetail>();

/**
 * ID of the most recently requested playlist. Used to guard against races when
 * the user rapidly switches playlists: a slower in-flight fetch must not clobber
 * the view state of a newer selection.
 */
let latestRequestedPlaylistId: string | null = null;

/** Shallow-then-deep structural comparison of two playlist details. */
function playlistDetailsEqual(a: JellyfinPlaylistDetail, b: JellyfinPlaylistDetail): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** Get the Jellyfin playlist state */
export function getJellyfinPlaylistState(): JellyfinPlaylistState {
  return jellyfinPlaylistState;
}

/** Whether playlists are available (not loading and no error). */
export function getJellyfinPlaylistsAvailable(): boolean {
  return !jellyfinPlaylistState.isLoading && jellyfinPlaylistState.error === null;
}

/** Playlist count. */
export function getJellyfinPlaylistCount(): number {
  return jellyfinPlaylistState.playlists.length;
}

// ============================================================================
// Actions
// ============================================================================

/** Set loading state. */
function setLoading(loading: boolean): void {
  jellyfinPlaylistState.isLoading = loading;
}

/** Set operating state (for mutations). */
function setOperating(operating: boolean): void {
  jellyfinPlaylistState.isOperating = operating;
}

/** Set error state. */
function setError(error: string | null): void {
  jellyfinPlaylistState.error = error;
  jellyfinPlaylistState.isLoading = false;
  jellyfinPlaylistState.isOperating = false;
}

/** Clear the current playlist selection. */
export function clearCurrentJellyfinPlaylist(): void {
  jellyfinPlaylistState.selectedPlaylistId = null;
  jellyfinPlaylistState.currentPlaylist = null;
}

/** Select a playlist and load its details. */
export async function selectJellyfinPlaylist(playlistId: string): Promise<void> {
  // A rename/delete requested for another playlist (the sidebar row menu)
  // is superseded the moment a DIFFERENT one is
  // selected — it must never survive to ambush whoever opens the original
  // target normally, later, once it happens to load.
  if (pendingPlaylistIntent && pendingPlaylistIntent.playlistId !== playlistId) {
    pendingPlaylistIntent = null;
  }
  jellyfinPlaylistState.selectedPlaylistId = playlistId;
  await loadJellyfinPlaylist(playlistId);
}

/** Load all playlists for the active backend. */
export async function loadJellyfinPlaylists(): Promise<void> {
  // Playlists now work on whichever backend is active (Jellyfin or OpenSubsonic)
  // via the unified MediaClient, so there is no backend gate here — the
  // caller only loads once a backend is connected.
  setLoading(true);
  setError(null);

  try {
    const playlists = await fetchPlaylists();
    jellyfinPlaylistState.playlists = playlists;
    jellyfinPlaylistState.isLoading = false;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    setError(message);
  }
}

/**
 * Load a specific Jellyfin playlist with its tracks.
 *
 * Stale-while-revalidate strategy:
 * - If the playlist is already the one on screen, do nothing (no refetch, no spinner).
 * - If a cached detail exists, render it synchronously (no spinner) and refresh in
 *   the background, updating the view only if the fresh copy actually differs.
 * - Only a genuine cold load (nothing cached) shows the loading spinner.
 */
export async function loadJellyfinPlaylist(playlistId: string): Promise<void> {
  latestRequestedPlaylistId = playlistId;

  // Already showing this playlist — nothing to do.
  if (jellyfinPlaylistState.currentPlaylist?.jellyfinId === playlistId) {
    return;
  }

  const cached = playlistDetailCache.get(playlistId);
  if (cached) {
    // L1 (session) warm path: render the in-memory detail instantly, revalidate.
    jellyfinPlaylistState.currentPlaylist = cached;
    jellyfinPlaylistState.isLoading = false;
    jellyfinPlaylistState.error = null;
    void revalidateJellyfinPlaylist(playlistId);
    return;
  }

  // L2 (SQLite) hydrate-then-revalidate: the in-memory Map is empty at every
  // launch, so the FIRST click used to be a cold network round-trip. Serve the
  // persisted copy instantly (also enables offline browsing) and refresh in the
  // background. Falls through to the network only on a true cache miss.
  try {
    const local = await fetchCachedPlaylist(playlistId);
    if (local && latestRequestedPlaylistId === playlistId) {
      playlistDetailCache.set(playlistId, local);
      jellyfinPlaylistState.currentPlaylist = local;
      jellyfinPlaylistState.isLoading = false;
      jellyfinPlaylistState.error = null;
      void revalidateJellyfinPlaylist(playlistId);
      return;
    }
  } catch {
    // A local-cache read failure is non-fatal — fall through to the network.
  }

  // Superseded while awaiting the local cache read — a newer selection now owns
  // the view/loading state, so do not fall through and clobber it (or leave the
  // spinner stuck by racing the newer request's settle).
  if (latestRequestedPlaylistId !== playlistId) {
    return;
  }

  // Cold path: nothing cached anywhere, show the spinner while we fetch.
  setLoading(true);
  setError(null);

  try {
    const playlist = await fetchPlaylist(playlistId);
    playlistDetailCache.set(playlistId, playlist);
    // Only apply to the view if this is still the playlist the user wants.
    if (latestRequestedPlaylistId === playlistId) {
      jellyfinPlaylistState.currentPlaylist = playlist;
      jellyfinPlaylistState.isLoading = false;
    }
  } catch (error) {
    // Only surface the error if this request still reflects the user's intent.
    if (latestRequestedPlaylistId === playlistId) {
      const message = error instanceof Error ? error.message : String(error);
      setError(message);
      // A rename/delete raised for this playlist can never be acted on if it
      // never loads — drop it rather than leave it to ambush a later,
      // successful open.
      if (pendingPlaylistIntent?.playlistId === playlistId) {
        pendingPlaylistIntent = null;
      }
    }
  }
}

/**
 * Fetch a fresh copy of a playlist in the background and update the cache (and
 * the view, if it is still showing this playlist and the data changed).
 * Failures are swallowed: a warm cache hit should never surface a network error.
 */
async function revalidateJellyfinPlaylist(playlistId: string): Promise<void> {
  try {
    const fresh = await fetchPlaylist(playlistId);
    playlistDetailCache.set(playlistId, fresh);
    if (
      latestRequestedPlaylistId === playlistId &&
      jellyfinPlaylistState.currentPlaylist?.jellyfinId === playlistId &&
      !playlistDetailsEqual(jellyfinPlaylistState.currentPlaylist, fresh)
    ) {
      jellyfinPlaylistState.currentPlaylist = fresh;
    }
  } catch {
    // Keep showing the cached data; background refresh failures are non-fatal.
  }
}

/** Create a new playlist on the active backend. */
export async function createJellyfinPlaylist(name: string): Promise<JellyfinPlaylist | null> {
  setOperating(true);
  setError(null);

  try {
    const playlist = await apiCreatePlaylist(name);
    jellyfinPlaylistState.playlists = [...jellyfinPlaylistState.playlists, playlist];
    jellyfinPlaylistState.isOperating = false;
    return playlist;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    setError(message);
    return null;
  }
}

/** Delete a Jellyfin playlist. */
export async function deleteJellyfinPlaylist(playlistId: string): Promise<boolean> {
  setOperating(true);
  setError(null);

  try {
    await apiDeletePlaylist(playlistId);
    playlistDetailCache.delete(playlistId);
    jellyfinPlaylistState.playlists = jellyfinPlaylistState.playlists.filter((p) => p.jellyfinId !== playlistId);
    if (jellyfinPlaylistState.currentPlaylist?.jellyfinId === playlistId) {
      jellyfinPlaylistState.currentPlaylist = null;
    }
    jellyfinPlaylistState.isOperating = false;
    return true;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    setError(message);
    return false;
  }
}

/** Rename a Jellyfin playlist. */
export async function renameJellyfinPlaylist(playlistId: string, name: string): Promise<boolean> {
  setOperating(true);
  setError(null);

  try {
    await apiRenamePlaylist(playlistId, name);
    jellyfinPlaylistState.playlists = jellyfinPlaylistState.playlists.map((p) =>
      p.jellyfinId === playlistId ? { ...p, name } : p
    );
    jellyfinPlaylistState.isOperating = false;
    return true;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    setError(message);
    return false;
  }
}

/** Add tracks to a Jellyfin playlist. */
export async function addTracksToJellyfinPlaylist(
  playlistId: string,
  trackIds: string[]
): Promise<boolean> {
  setOperating(true);
  setError(null);

  try {
    const updated = await apiAddTracks(playlistId, trackIds);
    playlistDetailCache.set(playlistId, updated);
    if (jellyfinPlaylistState.currentPlaylist?.jellyfinId === playlistId) {
      jellyfinPlaylistState.currentPlaylist = updated;
    }
    jellyfinPlaylistState.playlists = jellyfinPlaylistState.playlists.map((p) =>
      p.jellyfinId === playlistId ? { ...p, itemCount: updated.tracks.length } : p
    );
    jellyfinPlaylistState.isOperating = false;
    return true;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    setError(message);
    return false;
  }
}

/** Remove tracks from a Jellyfin playlist. */
export async function removeTracksFromJellyfinPlaylist(
  playlistId: string,
  entryIds: string[]
): Promise<boolean> {
  setOperating(true);
  setError(null);

  try {
    const updated = await apiRemoveTracks(playlistId, entryIds);
    playlistDetailCache.set(playlistId, updated);
    if (jellyfinPlaylistState.currentPlaylist?.jellyfinId === playlistId) {
      jellyfinPlaylistState.currentPlaylist = updated;
    }
    jellyfinPlaylistState.playlists = jellyfinPlaylistState.playlists.map((p) =>
      p.jellyfinId === playlistId ? { ...p, itemCount: updated.tracks.length } : p
    );
    jellyfinPlaylistState.isOperating = false;
    return true;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    setError(message);
    return false;
  }
}

/** Move a track within a Jellyfin playlist. */
export async function moveJellyfinPlaylistItem(
  playlistId: string,
  itemId: string,
  newIndex: number
): Promise<boolean> {
  setOperating(true);
  setError(null);

  try {
    const updated = await apiMoveItem(playlistId, itemId, newIndex);
    playlistDetailCache.set(playlistId, updated);
    if (jellyfinPlaylistState.currentPlaylist?.jellyfinId === playlistId) {
      jellyfinPlaylistState.currentPlaylist = updated;
    }
    jellyfinPlaylistState.isOperating = false;
    return true;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    setError(message);
    return false;
  }
}

// ============================================================================
// Pending intent
// ============================================================================

/**
 * A rename/delete requested somewhere that isn't the open playlist view
 * itself (the sidebar row menu) — PlaylistView owns the one rename-in-place
 * mechanism and the one delete-confirmation dialog; a requester selects the
 * playlist and leaves this flag for PlaylistView to pick up once it mounts
 * with that playlist loaded, rather than reimplementing either (the same
 * pending-reveal pattern `revealInLibrary.svelte.ts` already uses for "Show
 * in Library").
 */
let pendingPlaylistIntent = $state<{ type: 'rename' | 'delete'; playlistId: string } | null>(null);

export function getPendingPlaylistIntent(): { type: 'rename' | 'delete'; playlistId: string } | null {
  return pendingPlaylistIntent;
}

export function requestPlaylistRename(playlistId: string): void {
  pendingPlaylistIntent = { type: 'rename', playlistId };
}

export function requestPlaylistDelete(playlistId: string): void {
  pendingPlaylistIntent = { type: 'delete', playlistId };
}

export function clearPendingPlaylistIntent(): void {
  pendingPlaylistIntent = null;
}
