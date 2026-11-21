import type { QueueEntrySource, QueueState, Track } from '#lib/types/models.js';
import { getPlaybackState } from '#lib/stores/player.svelte.js';

const initialQueueState: QueueState = {
  tracks: [],
  currentIndex: -1,
  sources: [],
};

let queueState = $state<QueueState>({ ...initialQueueState });

/**
 * The queue is a pure reflection of the backend engine's own queue — the
 * single place that builds and advances it (see `#lib/utils/playback.js`'s
 * `startPlayback` and the transport controls, which call the backend
 * directly and never mutate this store's contents themselves). The only
 * writer is `setQueueState`, called from the `queue-changed` Tauri event
 * listener in `#lib/services/events.js`.
 */

/** Get the full queue state */
export function getQueueState(): QueueState {
  return queueState;
}

/** Current track in the queue */
export function getCurrentQueueTrack(): Track | null {
  return queueState.currentIndex >= 0 && queueState.currentIndex < queueState.tracks.length
    ? (queueState.tracks[queueState.currentIndex] ?? null)
    : null;
}

/** Upcoming tracks (after current) */
export function getUpcomingTracks(): Track[] {
  return queueState.currentIndex >= 0 ? queueState.tracks.slice(queueState.currentIndex + 1) : [];
}

/** One upcoming queue row, with its absolute queue index (for reorder/remove/jump calls) and source. */
export interface QueueEntry {
  track: Track;
  source: QueueEntrySource;
  index: number;
}

/**
 * Upcoming entries (after current), each carrying its own source and its
 * absolute index in the backend queue — the Up Next panel groups these into
 * "Playing Next" (user-queued) and the rest of the context by partitioning
 * this list into contiguous same-source runs, in the engine's own order,
 * rather than re-sorting by source (a "Play Next" click and a trailing "Add
 * to Queue" click can both be user-queued yet land in different positions).
 */
export function getUpcomingQueueEntries(): QueueEntry[] {
  if (queueState.currentIndex < 0) return [];
  const entries: QueueEntry[] = [];
  for (let i = queueState.currentIndex + 1; i < queueState.tracks.length; i++) {
    const track = queueState.tracks[i];
    const source = queueState.sources[i];
    if (track && source) entries.push({ track, source, index: i });
  }
  return entries;
}

/** Previous tracks (before current) */
export function getPreviousTracks(): Track[] {
  return queueState.currentIndex > 0 ? queueState.tracks.slice(0, queueState.currentIndex) : [];
}

/**
 * Whether there is a next track. Under repeat-all the engine wraps back to
 * the start (`resolve_manual_next_position`), so Next stays enabled at the
 * end of the queue too — matching Apple Music/Spotify, where the boundary
 * only disables Next when repeat is off.
 */
export function getHasNext(): boolean {
  if (queueState.tracks.length === 0 || queueState.currentIndex < 0) return false;
  if (getPlaybackState().repeat === 'all') return true;
  return queueState.currentIndex < queueState.tracks.length - 1;
}

/** Whether there is a previous track (see `getHasNext` — repeat-all wraps). */
export function getHasPrevious(): boolean {
  if (queueState.tracks.length === 0 || queueState.currentIndex < 0) return false;
  if (getPlaybackState().repeat === 'all') return true;
  return queueState.currentIndex > 0;
}

/** Total queue length */
export function getQueueLength(): number {
  return queueState.tracks.length;
}

/** Replace the queue state wholesale — called only from the `queue-changed` event handler. */
export function setQueueState(state: QueueState): void {
  queueState = state;
}
