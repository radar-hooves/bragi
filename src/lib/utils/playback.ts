import { getPlaybackState, setPlaying, getHasTrack } from '#lib/stores/player.svelte.js';
import { playContext, setRepeat, pause as tauriPause, resume as tauriResume } from '#lib/services/tauri.js';
import { reportTransportError } from './transportError';
import type { Track } from '#lib/types/models.js';

/**
 * Start playback of a track within a list, queueing the rest — the one
 * mechanism every "play this row" entry point goes through (Songs, an album,
 * an artist, a playlist, search, the column browser, recently played/added).
 *
 * Builds and plays the whole context in a single backend round trip
 * (`playContext`): the queue and the current track are never set locally —
 * both arrive via the backend's `playback-state-changed`/`queue-changed`
 * events, which is what makes the engine the single source of truth for the
 * queue instead of two mechanisms that can drift apart.
 *
 * The current shuffle preference is passed straight into `playContext`, which
 * applies it while building the queue — never a follow-up `setShuffle` call,
 * which would re-permute the just-built queue a second time and visibly
 * reorder it a moment after it first appeared (is still honoured: the
 * engine shuffles the fresh queue exactly once, here).
 */
export async function startPlayback(tracks: Track[], startIndex: number): Promise<void> {
  if (tracks.length === 0) return;
  if (!tracks[startIndex]) return;
  await playContext(tracks, startIndex, getPlaybackState().shuffle);
  // Sync the persisted repeat mode so it survives restart. The engine
  // defaults to off and only learns the preference once playback begins.
  // Repeat never reorders the queue, so — unlike shuffle — this can safely
  // stay a separate call with no double-broadcast risk.
  await setRepeat(getPlaybackState().repeat).catch((err) =>
    console.error('Failed to sync repeat mode to the engine on playback start:', err)
  );
}

/** Start playback from a specific track in a list, locating it by jellyfinId. */
export async function playTrackFromList(track: Track, tracks: Track[]): Promise<void> {
  const idx = tracks.findIndex((t) => t.jellyfinId === track.jellyfinId);
  await startPlayback(tracks, idx >= 0 ? idx : 0);
}

/**
 * Toggle play/pause of whatever is currently loaded — the Space bar (global,
 * outside a text field: `globalShortcuts.ts`) and the transport bar's own
 * button both go through this one mechanism, so they can never disagree on
 * what "toggle" means. A no-op with nothing loaded, matching Music.app/Spotify
 * (Space does nothing until something has played).
 */
export async function togglePlayPause(): Promise<void> {
  if (!getHasTrack()) return;
  const newPlaying = !getPlaybackState().isPlaying;
  setPlaying(newPlaying);
  try {
    if (newPlaying) await tauriResume();
    else await tauriPause();
  } catch (err) {
    // Roll back the optimistic flip — the backend never confirmed it via
    // playback-state-changed, so trusting it would leave the UI showing the
    // wrong state indefinitely.
    setPlaying(!newPlaying);
    reportTransportError(newPlaying ? 'Resume' : 'Pause', err);
  }
}

/** Shuffle an array of tracks and start playback from the first. */
export async function shuffleAndPlay(tracks: Track[]): Promise<void> {
  if (tracks.length === 0) return;
  const shuffled = [...tracks];
  for (let i = shuffled.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [shuffled[i], shuffled[j]] = [shuffled[j]!, shuffled[i]!];
  }
  await startPlayback(shuffled, 0);
}
