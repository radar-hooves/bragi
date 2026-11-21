// The search box's latest-wins session: answers landing out of order,
// a clear or an opened result racing a request still in flight.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { RankedSearchResults } from './searchRanking';

const performLibrarySearch = vi.fn();
vi.mock('./search', () => ({
  performLibrarySearch: (...args: unknown[]) => performLibrarySearch(...args),
}));

const { createSearchSession } = await import('./searchSession.svelte');

function answer(name: string): RankedSearchResults {
  return { tracks: [], albums: [], artists: [], topResult: { type: 'artist', item: { name } } } as unknown as RankedSearchResults;
}

/** One pending search per call, settled by the test in whatever order it chooses. */
function deferredSearches() {
  const pending: { resolve: (r: RankedSearchResults) => void; reject: (e: Error) => void }[] = [];
  performLibrarySearch.mockImplementation(
    () =>
      new Promise((resolve, reject) => {
        pending.push({ resolve, reject });
      })
  );
  return pending;
}

describe('createSearchSession', () => {
  beforeEach(() => {
    performLibrarySearch.mockReset();
  });

  it('lands only the newest query when answers arrive out of order', async () => {
    const pending = deferredSearches();
    const session = createSearchSession(() => ({ isSyncing: false }));

    const older = session.search('port');
    const newer = session.search('portishead');
    pending[1]!.resolve(answer('Portishead'));
    expect(await newer).toEqual(answer('Portishead'));
    pending[0]!.resolve(answer('Portugal. The Man'));

    // The older query resolves to null, so Return on it activates nothing.
    expect(await older).toBeNull();
    expect(session.results).toEqual(answer('Portishead'));
    expect(session.searching).toBe(false);
  });

  it('stays cleared when the box is cleared with a search still in flight', async () => {
    const pending = deferredSearches();
    const session = createSearchSession(() => ({ isSyncing: false }));

    const inFlight = session.search('portishead');
    expect(await session.search('')).toBeNull();
    expect(session.searching).toBe(false);

    pending[0]!.resolve(answer('Portishead'));
    expect(await inFlight).toBeNull();
    expect(session.results).toBeNull();
    expect(session.searching).toBe(false);
  });

  it('discards the in-flight answer when a result is opened', async () => {
    const pending = deferredSearches();
    const session = createSearchSession(() => ({ isSyncing: false }));

    const first = session.search('port');
    pending[0]!.resolve(answer('Portishead'));
    await first;
    const second = session.search('portis');
    session.dismiss();

    pending[1]!.resolve(answer('Portishead'));
    expect(await second).toBeNull();
    expect(session.results).toBeNull();
  });

  it('ignores a superseded request that fails', async () => {
    const pending = deferredSearches();
    const errors = vi.spyOn(console, 'error').mockImplementation(() => {});
    const session = createSearchSession(() => ({ isSyncing: false }));

    const older = session.search('port');
    const newer = session.search('portishead');
    pending[1]!.resolve(answer('Portishead'));
    await newer;
    pending[0]!.reject(new Error('server went away'));

    expect(await older).toBeNull();
    expect(session.results).toEqual(answer('Portishead'));
    expect(errors).not.toHaveBeenCalled();
    errors.mockRestore();
  });
});
