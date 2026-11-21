/**
 * The one search ranking, shared by the cached (SQLite FTS) and live
 * (Jellyfin/Subsonic) result paths so search reads the same regardless of
 * which backend produced the candidates (search polish).
 *
 * Reference: Apple Music (macOS) and Spotify both fold case/accents, treat a
 * fully-typed word as an exact or prefix match ahead of a mid-string hit, and
 * ignore a leading "The" when the query targets an artist — the same fold
 * `ordering.ts` already uses for sorting (mirrored in
 * `src-tauri/src/ordering.rs`), so a search result and a sorted list agree on
 * what "the same name" means.
 */
import type { Album, Artist, Track } from '#lib/types/models.js';
import { compareArtistNames, compareFolded, fold, stripLeadingArticle } from './ordering';

export interface SearchResultSet {
  tracks: Track[];
  albums: Album[];
  artists: Artist[];
}

export type TopResult =
  | { type: 'track'; item: Track }
  | { type: 'album'; item: Album }
  | { type: 'artist'; item: Artist };

export interface RankedSearchResults extends SearchResultSet {
  /** The single best match across all three groups, or `null` when nothing
   * matched confidently enough to headline (Apple Music/Spotify only ever
   * show a Top Result for an exact or prefix hit, never a loose substring
   * one). */
  topResult: TopResult | null;
}

/** Lower is a stronger match: 0 exact, 1 prefix, 2 contains, 3 no match at all. */
type MatchTier = 0 | 1 | 2 | 3;

const NO_MATCH: MatchTier = 3;

/** Tier a single already-folded name achieves against an already-folded query. */
function tierOfName(foldedName: string, foldedQuery: string): MatchTier {
  if (foldedName === foldedQuery) return 0;
  if (foldedName.startsWith(foldedQuery)) return 1;
  if (foldedName.includes(foldedQuery)) return 2;
  return NO_MATCH;
}

/** Best tier `name` achieves against an already-folded query. `stripArticle`
 * additionally tries the name with a leading "The"/"A"/"An" dropped, so
 * searching "Beatles" ranks "The Beatles" as a prefix match, matching
 * `compareArtistNames`'s own article-stripping scope (artist names only). */
function matchTier(name: string, foldedQuery: string, stripArticle: boolean): MatchTier {
  if (!name) return NO_MATCH;
  const direct = tierOfName(fold(name), foldedQuery);
  if (!stripArticle || direct === 0) return direct;
  const stripped = tierOfName(fold(stripLeadingArticle(name)), foldedQuery);
  return Math.min(direct, stripped) as MatchTier;
}

function rank<T>(
  items: T[],
  foldedQuery: string,
  tierOf: (item: T) => MatchTier,
  compare: (a: T, b: T) => number
): T[] {
  return items
    .map((item) => ({ item, tier: tierOf(item) }))
    .filter((entry) => entry.tier < NO_MATCH)
    .sort((a, b) => a.tier - b.tier || compare(a.item, b.item))
    .map((entry) => entry.item);
}

const trackTier = (t: Track, q: string): MatchTier =>
  Math.min(matchTier(t.name, q, false), matchTier(t.artistName, q, true)) as MatchTier;

const albumTier = (a: Album, q: string): MatchTier =>
  Math.min(matchTier(a.name, q, false), matchTier(a.artistName, q, true)) as MatchTier;

const artistTier = (a: Artist, q: string): MatchTier => matchTier(a.name, q, true);

/**
 * Re-rank a candidate set (already filtered by the backend — FTS5 or a
 * Jellyfin/Subsonic search) into the one display order: exact match first,
 * then prefix, then a mid-string hit, each tier broken by the shared folded
 * comparator. Safe to call on an empty query (returns everything as a
 * `NO_MATCH`-filtered empty set) — callers skip that case upstream instead.
 */
export function rankSearchResults(query: string, results: SearchResultSet): RankedSearchResults {
  const foldedQuery = fold(query.trim());
  if (!foldedQuery) return { tracks: [], albums: [], artists: [], topResult: null };

  const tracks = rank(results.tracks, foldedQuery, (t) => trackTier(t, foldedQuery), (a, b) => compareFolded(a.name, b.name));
  const albums = rank(results.albums, foldedQuery, (a) => albumTier(a, foldedQuery), (a, b) => compareFolded(a.name, b.name));
  const artists = rank(
    results.artists,
    foldedQuery,
    (a) => artistTier(a, foldedQuery),
    (a, b) => compareArtistNames(a.name, b.name, a.sortName, b.sortName)
  );

  const topResult = pickTopResult(foldedQuery, tracks, albums, artists);
  return { tracks, albums, artists, topResult };
}

/** Best overall match, artist > album > track on an equal tier — an exact
 * artist name is the strongest, most specific signal a query can give
 * (Apple Music's Top Result hero favours the same order). `null` below tier 1
 * (nothing exact or prefix-matched) so a vague substring hit never headlines. */
function pickTopResult(foldedQuery: string, tracks: Track[], albums: Album[], artists: Artist[]): TopResult | null {
  const artistTierValue = artists.length > 0 ? artistTier(artists[0]!, foldedQuery) : NO_MATCH;
  const albumTierValue = albums.length > 0 ? albumTier(albums[0]!, foldedQuery) : NO_MATCH;
  const trackTierValue = tracks.length > 0 ? trackTier(tracks[0]!, foldedQuery) : NO_MATCH;

  const best = Math.min(artistTierValue, albumTierValue, trackTierValue);
  if (best > 1) return null;
  if (artistTierValue === best) return { type: 'artist', item: artists[0]! };
  if (albumTierValue === best) return { type: 'album', item: albums[0]! };
  return { type: 'track', item: tracks[0]! };
}
