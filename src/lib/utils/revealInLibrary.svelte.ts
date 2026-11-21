import type { Track } from '#lib/types/models.js';
import { syncColumnBrowserToSelection } from '#lib/stores/columnBrowser.svelte.js';
import { getGenreStats } from '#lib/services/tauri.js';
import { setCurrentSection, setColumnBrowserVisible } from '#lib/stores/ui.svelte.js';
import { navigateTo } from '#lib/stores/navigation.svelte.js';

/**
 * The track the list should scroll to, if any.
 *
 * Held as state rather than dispatched as an event: a reveal from another
 * section mounts TrackList in the same tick, and an event fired before its
 * listener exists is delivered to nobody. The list clears this once it has
 * scrolled, so a later reveal of the same track fires again.
 */
let pendingRevealId = $state<string | null>(null);

export function getPendingRevealId(): string | null {
  return pendingRevealId;
}

export function clearPendingReveal(): void {
  pendingRevealId = null;
}

/**
 * "Show in Library" — the iTunes/Spotify Ctrl+L reveal.
 *
 * Deliberately an explicit action rather than the queue automatically dragging
 * the view around: the queue advances while you are browsing something else,
 * and auto-following yanks the list out from under you.
 */
export async function revealTrackInLibrary(track: Track | null | undefined): Promise<void> {
  if (!track) return;

  const artist = track.artistName || null;
  const albumId = track.albumJellyfinId || null;

  // Select and navigate first, so the view moves immediately; the genre is a
  // highlight, not a filter (the album is strictly narrower), and resolving it
  // needs a round trip.
  syncColumnBrowserToSelection({ artist, albumId });
  setCurrentSection('tracks');
  setColumnBrowserVisible(true);
  navigateTo({
    section: 'tracks',
    selectedArtistId: null,
    selectedAlbumId: albumId,
    columnBrowserVisible: true,
  });
  pendingRevealId = track.jellyfinId;

  // A track's `genre` is the RAW tag string; the browser's genre column lists
  // normalised genres. Match its first tag against the real genre list
  // — read from the backend rather than from whatever the column browser
  // happens to have loaded, so this works on the first reveal of a session too.
  const firstTag = track.genre?.split(';')[0]?.trim();
  if (!firstTag) return;
  try {
    const genres = await getGenreStats();
    const genre = genres.find((g) => g.name.toLowerCase() === firstTag.toLowerCase());
    if (genre) syncColumnBrowserToSelection({ genre: genre.name, artist, albumId });
  } catch (e) {
    console.error('Failed to resolve the revealed track’s genre:', e);
  }
}
