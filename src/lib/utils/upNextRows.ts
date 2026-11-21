import type { QueueEntry } from '#lib/stores/queue.svelte.js';

export type UpNextRow =
  | { kind: 'header'; key: string; label: string }
  | { kind: 'entry'; key: string; entry: QueueEntry };

/**
 * Groups upcoming queue entries into contiguous same-source runs, each under
 * a heading — "Playing Next" for user-queued tracks (Play Next / Add to
 * Queue), "Next Up" for the rest of the played context — matching what Apple
 * Music and Spotify both show above the "Now playing" track.
 *
 * A run is never re-sorted or merged with a later run of the same source: a
 * "Play Next" click lands contiguously right after the current track, but a
 * trailing "Add to Queue" (`QueueInsertPosition::End`) lands after the
 * context tail as its own, later run — this reflects the engine's actual
 * queue order rather than inventing a shape it doesn't have.
 */
export function buildUpNextRows(entries: QueueEntry[]): UpNextRow[] {
  const rows: UpNextRow[] = [];
  let previousSource: QueueEntry['source'] | null = null;

  for (const entry of entries) {
    if (entry.source !== previousSource) {
      rows.push({
        kind: 'header',
        key: `header-${entry.index}`,
        label: entry.source === 'userQueued' ? 'Playing Next' : 'Next Up',
      });
      previousSource = entry.source;
    }
    rows.push({ kind: 'entry', key: String(entry.index), entry });
  }

  return rows;
}
