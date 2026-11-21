import { describe, it, expect } from 'vitest';
import { buildUpNextRows } from './upNextRows';
import type { QueueEntry } from '#lib/stores/queue.svelte.js';
import type { Track } from '#lib/types/models.js';

function track(id: string): Track {
  return { jellyfinId: id, name: id } as unknown as Track;
}

function entry(id: string, source: QueueEntry['source'], index: number): QueueEntry {
  return { track: track(id), source, index };
}

describe('buildUpNextRows', () => {
  it('is empty for no upcoming entries', () => {
    expect(buildUpNextRows([])).toEqual([]);
  });

  it('puts a single "Playing Next" header above a contiguous user-queued run', () => {
    const rows = buildUpNextRows([entry('a', 'userQueued', 1), entry('b', 'userQueued', 2)]);
    expect(rows).toEqual([
      { kind: 'header', key: 'header-1', label: 'Playing Next' },
      { kind: 'entry', key: '1', entry: entry('a', 'userQueued', 1) },
      { kind: 'entry', key: '2', entry: entry('b', 'userQueued', 2) },
    ]);
  });

  it('puts a single "Next Up" header above a contiguous context run', () => {
    const rows = buildUpNextRows([entry('a', 'context', 1), entry('b', 'context', 2)]);
    expect(rows).toEqual([
      { kind: 'header', key: 'header-1', label: 'Next Up' },
      { kind: 'entry', key: '1', entry: entry('a', 'context', 1) },
      { kind: 'entry', key: '2', entry: entry('b', 'context', 2) },
    ]);
  });

  it('groups "Playing Next" above "Next Up" in the engine\'s own order — the reference-app shape', () => {
    const entries = [
      entry('next-1', 'userQueued', 1),
      entry('next-2', 'userQueued', 2),
      entry('ctx-1', 'context', 3),
      entry('ctx-2', 'context', 4),
    ];
    const rows = buildUpNextRows(entries);

    expect(rows.map((r) => (r.kind === 'header' ? `H:${r.label}` : r.entry.track.jellyfinId))).toEqual([
      'H:Playing Next',
      'next-1',
      'next-2',
      'H:Next Up',
      'ctx-1',
      'ctx-2',
    ]);
  });

  it('starts a new header for a later, non-contiguous run of the same source rather than merging it', () => {
    // A "Play Next" run at the front, then context, then a trailing "Add to
    // Queue" (QueueInsertPosition::End) lands after the context tail.
    const entries = [
      entry('next-1', 'userQueued', 1),
      entry('ctx-1', 'context', 2),
      entry('later', 'userQueued', 3),
    ];
    const rows = buildUpNextRows(entries);

    expect(rows.map((r) => (r.kind === 'header' ? `H:${r.label}` : r.entry.track.jellyfinId))).toEqual([
      'H:Playing Next',
      'next-1',
      'H:Next Up',
      'ctx-1',
      'H:Playing Next',
      'later',
    ]);
  });
});
