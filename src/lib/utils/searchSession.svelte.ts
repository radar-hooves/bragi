/**
 * The search box's one latest-wins session over `performLibrarySearch`:
 * only the newest query's answer ever lands, and dismissing search
 * (clearing the box, opening a result) also discards whatever is still in
 * flight, so a slow earlier answer can never bring the results back. Return
 * activates the answer `search` resolves to, which is always that same
 * query's, never the previous one's.
 *
 * A factory returning reactive getters, the shape `createSelection` uses.
 */
import { performLibrarySearch, type SearchContext } from './search';
import type { RankedSearchResults } from './searchRanking';

export interface SearchSession {
  /** The newest query's ranked results, or null while search is dismissed. */
  readonly results: RankedSearchResults | null;
  /** A request for the newest query is in flight. */
  readonly searching: boolean;
  /** Resolves to this query's results, or null when it was blank, failed, or was superseded. */
  search: (query: string) => Promise<RankedSearchResults | null>;
  /** Drop the results and anything still in flight. */
  dismiss: () => void;
}

export function createSearchSession(getContext: () => SearchContext): SearchSession {
  let results = $state<RankedSearchResults | null>(null);
  let searching = $state(false);
  let latest = 0;

  function dismiss(): void {
    latest++;
    results = null;
    searching = false;
  }

  async function search(query: string): Promise<RankedSearchResults | null> {
    if (!query.trim()) {
      dismiss();
      return null;
    }
    const id = ++latest;
    searching = true;
    try {
      const answer = await performLibrarySearch(query, getContext());
      if (id !== latest) return null;
      results = answer;
      return answer;
    } catch (e) {
      if (id !== latest) return null;
      console.error('Search failed:', e);
      results = null;
      return null;
    } finally {
      if (id === latest) searching = false;
    }
  }

  return {
    get results() {
      return results;
    },
    get searching() {
      return searching;
    },
    search,
    dismiss,
  };
}
