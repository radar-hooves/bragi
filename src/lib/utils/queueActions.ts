import { addToQueue } from '#lib/services/tauri.js';
import { toast } from 'svelte-sonner';
import type { Track } from '#lib/types/models.js';

/** "Song" or "12 tracks" — the toast description for a one- or many-track queue action. */
function describeTracks(tracks: Track[]): string {
  return tracks.length === 1 ? tracks[0]!.name : `${tracks.length} tracks`;
}

/**
 * Play Next / Add to Queue (Apple Music/Spotify convention): a user-added
 * item goes straight through the same backend queue mechanism as everything
 * else (`add_to_queue`, already used internally by `startPlayback` — this is
 * simply the first UI surface that reaches it with `position: 'next'`). The
 * queue-changed event updates every view; no local queue mutation here.
 *
 * Takes an array so a single track and a whole album/artist/selection share
 * one call — never a scalar variant beside a plural one.
 */
export async function queueTrackNext(tracks: Track[]): Promise<void> {
  if (tracks.length === 0) return;
  try {
    await addToQueue(tracks, 'next');
    toast.success('Playing next', { description: describeTracks(tracks) });
  } catch (err) {
    console.error('Failed to queue tracks next:', err);
    toast.error('Could not queue tracks', { description: describeTracks(tracks) });
  }
}

export async function queueTrackAtEnd(tracks: Track[]): Promise<void> {
  if (tracks.length === 0) return;
  try {
    await addToQueue(tracks, 'end');
    toast.success('Added to queue', { description: describeTracks(tracks) });
  } catch (err) {
    console.error('Failed to add tracks to queue:', err);
    toast.error('Could not queue tracks', { description: describeTracks(tracks) });
  }
}
