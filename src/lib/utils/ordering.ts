/**
 * The one music sort order, mirrored from `src-tauri/src/ordering.rs` so the
 * SQL `ORDER BY` and every frontend comparator agree: case/accent folding,
 * natural numeric comparison ("2" before "10"), and a leading-article strip
 * for artist names.
 */

// Combining-mark code point ranges — kept as explicit ranges (not the broader
// \p{Mn} Unicode property) so this matches src-tauri/src/ordering.rs's
// `is_combining_mark` exactly; a wider frontend fold would disagree with the
// SQL collation on non-Latin scripts.
const COMBINING_MARK_RANGES: ReadonlyArray<readonly [number, number]> = [
  [0x0300, 0x036f],
  [0x1ab0, 0x1aff],
  [0x1dc0, 0x1dff],
  [0x20d0, 0x20ff],
];

function isCombiningMark(codePoint: number): boolean {
  return COMBINING_MARK_RANGES.some(([lo, hi]) => codePoint >= lo && codePoint <= hi);
}

// A sort chains several folded comparisons per pair (disc/track/title,
// artist/album tie-breaks), and the same title or artist name recurs across
// many of those pairs — NFD-decomposing and rebuilding it from scratch each
// time is what made a 4,233-title sort take ~300ms instead of ~15-30ms.
// Cache the fold, capped so a pathological caller (an
// unbounded stream of distinct strings) can't grow it forever.
const foldCache = new Map<string, string>();
const FOLD_CACHE_LIMIT = 100_000;

/** Case- and accent-fold a string for comparison: NFD-decompose, drop the
 * combining marks decomposition split off, then lowercase. Memoised — see
 * `foldCache` above. */
export function fold(s: string): string {
  const cached = foldCache.get(s);
  if (cached !== undefined) return cached;
  const folded = Array.from(s.normalize('NFD'))
    .filter((ch) => !isCombiningMark(ch.codePointAt(0)!))
    .join('')
    .toLowerCase();
  if (foldCache.size >= FOLD_CACHE_LIMIT) foldCache.clear();
  foldCache.set(s, folded);
  return folded;
}

const LEADING_ARTICLE_RE = /^(the|an|a) /i;

/** Drop a leading "The "/"An "/"A " (case-insensitive) for sort purposes only
 * — the display name is never touched. */
export function stripLeadingArticle(s: string): string {
  const trimmed = s.trimStart();
  const match = LEADING_ARTICLE_RE.exec(trimmed);
  if (match && trimmed.length > match[0].length) {
    return trimmed.slice(match[0].length);
  }
  return s;
}

function isAsciiDigit(c: string): boolean {
  return c >= '0' && c <= '9';
}

/** Strip leading zeros off a digit run, keeping the last one for an
 * all-zero run ("000" -> "0") so the result is never empty. */
function stripLeadingZeros(digits: string): string {
  let i = 0;
  while (i < digits.length - 1 && digits[i] === '0') i++;
  return digits.slice(i);
}

/** Compare two already-folded strings, treating a run of digits as one number
 * rather than comparing digit-by-digit (so "2" sorts before "10", and "07"
 * sorts by its magnitude — 7 — not by having one more character than "7"). */
function naturalCompare(a: string, b: string): number {
  let i = 0;
  let j = 0;
  while (i < a.length || j < b.length) {
    if (i >= a.length) return -1;
    if (j >= b.length) return 1;
    const ca = a[i]!;
    const cb = b[j]!;
    if (isAsciiDigit(ca) && isAsciiDigit(cb)) {
      const startA = i;
      const startB = j;
      while (i < a.length && isAsciiDigit(a[i]!)) i++;
      while (j < b.length && isAsciiDigit(b[j]!)) j++;
      const numA = a.slice(startA, i);
      const numB = b.slice(startB, j);
      const magA = stripLeadingZeros(numA);
      const magB = stripLeadingZeros(numB);
      if (magA.length !== magB.length) return magA.length < magB.length ? -1 : 1;
      if (magA !== magB) return magA < magB ? -1 : 1;
      // Equal magnitude ("7" vs "07"): fall back to the zero count so the
      // comparison is still a total order, fewer zeros sorting first.
      const zerosA = numA.length - magA.length;
      const zerosB = numB.length - magB.length;
      if (zerosA !== zerosB) return zerosA < zerosB ? -1 : 1;
      continue;
    }
    if (ca === cb) {
      i++;
      j++;
      continue;
    }
    return ca < cb ? -1 : 1;
  }
  return 0;
}

/** Compare two names (album/track titles, genre names): fold, then natural. */
export function compareFolded(a: string, b: string): number {
  return naturalCompare(fold(a), fold(b));
}

/**
 * Compare two artist names: prefer each side's Jellyfin `sortName` when
 * present, drop a leading article, then {@link compareFolded}. Stripping an
 * article from an already sort-adjusted name is a no-op, so this is safe to
 * call whether or not a `sortName` was supplied.
 */
export function compareArtistNames(aName: string, bName: string, aSortName?: string | null, bSortName?: string | null): number {
  const aKey = aSortName?.trim() || aName;
  const bKey = bSortName?.trim() || bName;
  return compareFolded(stripLeadingArticle(aKey), stripLeadingArticle(bKey));
}

/** An album's tracks: disc, then track, then title (deterministic tie-break). */
export function compareTrackOrder(a: { discNumber: number; trackNumber: number; name: string }, b: { discNumber: number; trackNumber: number; name: string }): number {
  return a.discNumber - b.discNumber || a.trackNumber - b.trackNumber || compareFolded(a.name, b.name);
}

/**
 * An artist's albums list: newest release first (Apple Music/Spotify
 * convention), falling back to the folded title for an equal or missing year.
 */
export function compareArtistAlbumOrder(a: { year?: number; name: string }, b: { year?: number; name: string }): number {
  const yearA = a.year ?? -Infinity;
  const yearB = b.year ?? -Infinity;
  if (yearA !== yearB) return yearB - yearA;
  return compareFolded(a.name, b.name);
}
