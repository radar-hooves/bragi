/**
 * The one library search entry point: the search box, ⌘F and the results
 * view all resolve a query through this single call, which picks cache vs.
 * live and ranks the result — no caller branches on which backend answered
 * or in what order it came back.
 */
import { searchLibrary, searchLibraryLive } from '#lib/services/tauri.js';
import { getOfflineMode } from '#lib/stores/offlineMode.svelte.js';
import { rankSearchResults, type RankedSearchResults } from './searchRanking';

export interface SearchContext {
  /** A backend's very first sync is still filling the cache — go straight to
   * a live search rather than under-reporting results against a cache that
   * doesn't have the library in it yet. */
  isSyncing: boolean;
}

const EMPTY_RESULTS: RankedSearchResults = { tracks: [], albums: [], artists: [], topResult: null };

export async function performLibrarySearch(query: string, ctx: SearchContext): Promise<RankedSearchResults> {
  if (!query.trim()) return EMPTY_RESULTS;

  // Only the user's own Offline Mode skips the live search: an unreachable
  // signal can be a false alarm, and a live search that fails already falls
  // back to the cache's answer.
  const offline = getOfflineMode();
  const preferLive = ctx.isSyncing && !offline;
  // A live search failing mid-sync answers from whatever the cache holds so
  // far, as the empty-cache fallback below keeps the cache's answer.
  let results = await (preferLive ? searchLibraryLive(query).catch(() => searchLibrary(query)) : searchLibrary(query));

  // An interrupted first sync, or a stale cache mid-resync, can leave the
  // cache empty/incomplete without `isSyncing` firing. A cached search coming
  // back fully empty is cheap to double-check live rather than risk reporting
  // "no results" when the server actually has matches.
  const cacheEmpty = results.tracks.length === 0 && results.albums.length === 0 && results.artists.length === 0;
  if (!preferLive && cacheEmpty && !offline) {
    try {
      results = await searchLibraryLive(query);
    } catch {
      // Offline, or the server dropped mid-search: the cache's answer — even
      // an empty one — is real and stands, rather than the search reverting
      // to no-results-shown.
    }
  }

  return rankSearchResults(query, results);
}
