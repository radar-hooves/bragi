/**
 * The one path every "add tracks to an existing Jellyfin playlist" entry
 * point uses (the Add to Playlist dialogue's existing-playlist option, drag
 * onto a playlist in the sidebar) — never the store action directly, so a
 * duplicate is always asked about exactly once, the same way everywhere.
 *
 * Reference (Apple Music, Spotify desktop): dropping a song already in the
 * target playlist asks "Add anyway" or "Skip" rather than silently
 * duplicating it or silently refusing.
 */

import type { JellyfinPlaylist } from '#lib/types/models.js';
import { getCachedJellyfinPlaylist, getJellyfinPlaylist } from '#lib/services/tauri.js';
import { addTracksToJellyfinPlaylist, getJellyfinPlaylistState } from '#lib/stores/jellyfinPlaylist.svelte.js';

export type AddDuplicateChoice = 'add' | 'skip' | 'cancel';

interface AddDuplicateDialogState {
  open: boolean;
  playlistName: string;
  duplicateCount: number;
  resolve: ((choice: AddDuplicateChoice) => void) | null;
}

let dialogState = $state<AddDuplicateDialogState>({
  open: false,
  playlistName: '',
  duplicateCount: 0,
  resolve: null,
});

/** State for the one shared "already in this playlist" dialog. */
export function getAddDuplicateDialogState(): AddDuplicateDialogState {
  return dialogState;
}

/** Answer the pending duplicate-add question (or close it via Escape/overlay, which resolves 'cancel'). */
export function resolveAddDuplicateDialog(choice: AddDuplicateChoice): void {
  dialogState.resolve?.(choice);
  dialogState = { open: false, playlistName: '', duplicateCount: 0, resolve: null };
}

// One question at a time: a second drop waits for the first to be answered
// rather than replacing it and leaving the first add waiting forever.
let lastQuestion: Promise<unknown> = Promise.resolve();

function askAddDuplicates(playlistName: string, duplicateCount: number): Promise<AddDuplicateChoice> {
  const question = lastQuestion.then(
    () =>
      new Promise<AddDuplicateChoice>((resolve) => {
        dialogState = { open: true, playlistName, duplicateCount, resolve };
      })
  );
  lastQuestion = question;
  return question;
}

/**
 * The target playlist's current track ids from the server, which is the
 * playlist's source of truth (another Jellyfin app may have added to it since
 * the cache last synced), falling back to the offline cache. A failure on
 * both is treated as "nothing known to be a duplicate": a hiccup here must
 * never block adding tracks the user asked to add.
 */
async function getExistingTrackIds(playlistId: string): Promise<Set<string>> {
  try {
    const fresh = await getJellyfinPlaylist(playlistId);
    return new Set(fresh.tracks.map((t) => t.jellyfinId));
  } catch {
    // Fall through to the cache.
  }
  try {
    const cached = await getCachedJellyfinPlaylist(playlistId);
    if (cached) return new Set(cached.tracks.map((t) => t.jellyfinId));
  } catch {
    // Nothing known.
  }
  return new Set();
}

/**
 * Add tracks to an existing Jellyfin playlist, asking first when any of them
 * are already in it. Resolves `false` if the user cancels; `true` once the
 * (possibly filtered) add has gone through, including the no-op case where
 * every incoming track was a duplicate the user chose to skip. Throws when the
 * server refuses the add, so a failure never reads as a cancel.
 */
export async function addTracksToPlaylistWithDuplicateCheck(
  playlist: JellyfinPlaylist,
  trackIds: string[]
): Promise<boolean> {
  const incoming = [...new Set(trackIds)];
  if (incoming.length === 0) return true;

  const existingIds = await getExistingTrackIds(playlist.jellyfinId);
  const duplicateCount = incoming.filter((id) => existingIds.has(id)).length;

  let finalIds = incoming;
  if (duplicateCount > 0) {
    const choice = await askAddDuplicates(playlist.name, duplicateCount);
    if (choice === 'cancel') return false;
    if (choice === 'skip') {
      finalIds = incoming.filter((id) => !existingIds.has(id));
      if (finalIds.length === 0) return true;
    }
  }

  if (await addTracksToJellyfinPlaylist(playlist.jellyfinId, finalIds)) return true;
  throw new Error(getJellyfinPlaylistState().error ?? 'Failed to add tracks');
}
