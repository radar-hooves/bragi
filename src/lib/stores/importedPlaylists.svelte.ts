/**
 * Shared/imported playlist state management store.
 *
 * Unlike Jellyfin playlists, imported playlists are entirely local — reads and
 * writes never touch a media server, so there's no offline cache layer or
 * stale-while-revalidate dance here: every read is already a fast local SQLite
 * query, and every open re-resolves tracks against the current library.
 */

import type { ImportedPlaylist, ImportedPlaylistDetail, ImportPlaylistTrackInput } from '#lib/types/models.js';
import {
  getImportedPlaylists as fetchImportedPlaylists,
  getImportedPlaylist as fetchImportedPlaylist,
  importPlaylist as apiImportPlaylist,
  importPlaylistFromJellyfinLink as apiImportPlaylistFromJellyfinLink,
  deleteImportedPlaylist as apiDeleteImportedPlaylist,
} from '#lib/services/tauri.js';

interface ImportedPlaylistState {
  /** List of all imported playlists (summary only). */
  playlists: ImportedPlaylist[];
  /** Currently selected imported playlist ID. */
  selectedPlaylistId: string | null;
  /** Currently selected imported playlist with resolved tracks. */
  currentPlaylist: ImportedPlaylistDetail | null;
  /** Whether the playlist list or a selected detail is being loaded. */
  isLoading: boolean;
  /** Whether an import/delete operation is in progress. */
  isOperating: boolean;
  /** Error message if something failed. */
  error: string | null;
}

const initialState: ImportedPlaylistState = {
  playlists: [],
  selectedPlaylistId: null,
  currentPlaylist: null,
  isLoading: false,
  isOperating: false,
  error: null,
};

let importedPlaylistState = $state<ImportedPlaylistState>({ ...initialState });

/** ID of the most recently requested playlist, to guard against races. */
let latestRequestedPlaylistId: string | null = null;

export function getImportedPlaylistState(): ImportedPlaylistState {
  return importedPlaylistState;
}

function setError(error: string | null): void {
  importedPlaylistState.error = error;
  importedPlaylistState.isLoading = false;
  importedPlaylistState.isOperating = false;
}

/** Clear the current imported-playlist selection. */
export function clearCurrentImportedPlaylist(): void {
  importedPlaylistState.selectedPlaylistId = null;
  importedPlaylistState.currentPlaylist = null;
}

/** Load every imported playlist (sidebar summaries). */
export async function loadImportedPlaylists(): Promise<void> {
  importedPlaylistState.isLoading = true;
  importedPlaylistState.error = null;

  try {
    const playlists = await fetchImportedPlaylists();
    importedPlaylistState.playlists = playlists;
    importedPlaylistState.isLoading = false;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    setError(message);
  }
}

/** Select an imported playlist and load its (freshly-resolved) detail. */
export async function selectImportedPlaylist(playlistId: string): Promise<void> {
  importedPlaylistState.selectedPlaylistId = playlistId;
  await loadImportedPlaylist(playlistId);
}

/** Load one imported playlist's detail, re-resolving tracks against the local library. */
export async function loadImportedPlaylist(playlistId: string): Promise<void> {
  latestRequestedPlaylistId = playlistId;
  importedPlaylistState.isLoading = true;
  importedPlaylistState.error = null;

  try {
    const detail = await fetchImportedPlaylist(playlistId);
    if (latestRequestedPlaylistId !== playlistId) return;
    if (!detail) {
      setError('This imported playlist no longer exists.');
      return;
    }
    importedPlaylistState.currentPlaylist = detail;
    importedPlaylistState.isLoading = false;
  } catch (error) {
    if (latestRequestedPlaylistId !== playlistId) return;
    const message = error instanceof Error ? error.message : String(error);
    setError(message);
  }
}

/** Apply a freshly-imported playlist's detail to state, newest-first in the list. */
function applyImportedPlaylist(detail: ImportedPlaylistDetail): void {
  importedPlaylistState.playlists = [
    { id: detail.id, name: detail.name, importedAt: detail.importedAt },
    ...importedPlaylistState.playlists,
  ];
  importedPlaylistState.selectedPlaylistId = detail.id;
  importedPlaylistState.currentPlaylist = detail;
  importedPlaylistState.isOperating = false;
}

export async function importPlaylist(
  name: string,
  tracks: ImportPlaylistTrackInput[]
): Promise<ImportedPlaylistDetail | null> {
  importedPlaylistState.isOperating = true;
  importedPlaylistState.error = null;

  try {
    const detail = await apiImportPlaylist(name, tracks);
    applyImportedPlaylist(detail);
    return detail;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    setError(message);
    return null;
  }
}

/** Import a playlist straight from a friend's plain Jellyfin server (no Bragi share code). */
export async function importPlaylistFromJellyfinLink(
  serverUrl: string,
  username: string,
  password: string,
  itemId: string
): Promise<ImportedPlaylistDetail | null> {
  importedPlaylistState.isOperating = true;
  importedPlaylistState.error = null;

  try {
    const detail = await apiImportPlaylistFromJellyfinLink(serverUrl, username, password, itemId);
    applyImportedPlaylist(detail);
    return detail;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    setError(message);
    return null;
  }
}

/** Delete an imported playlist. */
export async function deleteImportedPlaylist(playlistId: string): Promise<boolean> {
  importedPlaylistState.isOperating = true;
  importedPlaylistState.error = null;

  try {
    await apiDeleteImportedPlaylist(playlistId);
    importedPlaylistState.playlists = importedPlaylistState.playlists.filter((p) => p.id !== playlistId);
    if (importedPlaylistState.currentPlaylist?.id === playlistId) {
      importedPlaylistState.currentPlaylist = null;
      importedPlaylistState.selectedPlaylistId = null;
    }
    importedPlaylistState.isOperating = false;
    return true;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    setError(message);
    return false;
  }
}
