import type { Track } from '#lib/types/models.js';
import { getTrackMeta } from '#lib/stores/trackMetadata.svelte.js';
import { formatRelativeTimestamp } from '#lib/utils/formatTimestamp.js';
import { compareArtistNames, compareFolded } from '#lib/utils/ordering.js';

/**
 * Present a track's RAW genre tag readably.
 *
 * `cached_tracks.genre` is the whole tag as the server gave it, so a
 * multi-value tag arrives as "Alternative Metal;Heavy Metal;Rock". This is
 * display formatting only — the authoritative split, case-folding and dedupe
 * live in Rust (`src-tauri/src/genres.rs`) and produce `track_genres`.
 */
function formatGenreTag(raw: string | null | undefined): string {
  if (!raw) return '';
  return raw
    .split(';')
    .map((part) => part.trim())
    .filter(Boolean)
    .join(', ');
}

export interface ColumnDef {
  id: string;
  label: string;
  align: 'left' | 'right' | 'center';
  fixedWidth?: number;
  /** Resize floor (px) — keeps the header label legible; Finder/Music/Mail's
   * own list-view columns never shrink past their label either. */
  minWidth: number;
  render: (track: Track) => string;
}

export function formatDuration(ms: number): string {
  const totalSeconds = Math.floor(ms / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, '0')}`;
}

export const allColumns: Record<string, ColumnDef> = {
  title: { id: 'title', label: 'Title', align: 'left', minWidth: 120, render: (t) => t.name },
  trackNumber: {
    id: 'trackNumber',
    label: 'Track #',
    align: 'right',
    fixedWidth: 60,
    minWidth: 50,
    render: (t) => (t.trackNumber > 0 ? String(t.trackNumber) : ''),
  },
  discNumber: {
    id: 'discNumber',
    label: 'Disc #',
    align: 'right',
    fixedWidth: 60,
    minWidth: 50,
    render: (t) => (t.discNumber > 0 ? String(t.discNumber) : ''),
  },
  time: {
    id: 'time',
    label: 'Time',
    align: 'right',
    fixedWidth: 64,
    minWidth: 50,
    render: (t) => formatDuration(t.durationMs),
  },
  artist: { id: 'artist', label: 'Artist', align: 'left', minWidth: 80, render: (t) => t.artistName },
  album: { id: 'album', label: 'Album', align: 'left', minWidth: 80, render: (t) => t.albumName },
  genre: { id: 'genre', label: 'Genre', align: 'left', minWidth: 80, render: (t) => formatGenreTag(t.genre) },
  year: {
    id: 'year',
    label: 'Year',
    align: 'right',
    fixedWidth: 64,
    minWidth: 50,
    render: (t) => (t.year ? String(t.year) : ''),
  },
  lastPlayed: {
    id: 'lastPlayed',
    label: 'Last Played',
    align: 'right',
    fixedWidth: 120,
    minWidth: 90,
    render: (t) => {
      const ms = getTrackMeta(t.jellyfinId).lastPlayedMs;
      return ms ? formatRelativeTimestamp(ms) : '';
    },
  },
  playCount: {
    id: 'playCount',
    label: 'Plays',
    align: 'right',
    fixedWidth: 56,
    minWidth: 50,
    render: (t) => {
      const count = getTrackMeta(t.jellyfinId).playCount;
      return count > 0 ? String(count) : '';
    },
  },
  skipCount: {
    id: 'skipCount',
    label: 'Skips',
    align: 'right',
    fixedWidth: 56,
    minWidth: 50,
    render: (t) => {
      const count = getTrackMeta(t.jellyfinId).skipCount;
      return count > 0 ? String(count) : '';
    },
  },
};

/** Always rendered, and never draggable to reorder: the identity column
 * (Title) is pinned first exactly like Finder's Name column and Apple
 * Music's Song Name column, and Time is Bragi's own standing product
 * decision (kept as-is here — not something this pass revisits). */
export const ALWAYS_COLUMNS = ['title', 'time'];

/** The one column that can never be hidden or dragged to a different
 * position — every reference list view (Finder, Mail, Apple Music) pins its
 * leading identity column the same way. */
export const PINNED_FIRST_COLUMN = 'title';

export function resolveActiveColumns(visibleColumns: string[]): ColumnDef[] {
  const seen = new Set<string>();
  const result: ColumnDef[] = [];
  for (const id of visibleColumns) {
    if (!seen.has(id) && allColumns[id]) {
      seen.add(id);
      result.push(allColumns[id]);
    }
  }
  for (const id of ALWAYS_COLUMNS) {
    if (!seen.has(id) && allColumns[id]) {
      result.push(allColumns[id]);
    }
  }
  return result;
}

export function colWidth(col: ColumnDef, columnWidths: Record<string, number>): string | undefined {
  const px = columnWidths[col.id];
  if (px) return `${px}px`;
  if (col.fixedWidth) return `${col.fixedWidth}px`;
  return undefined;
}

/** Every column the visibility menu can toggle, in `allColumns`' own order —
 * TrackList (Songs, Recently Played, Search) offers all of them. */
export const DEFAULT_TOGGLEABLE_COLUMNS: string[] = Object.keys(allColumns).filter(
  (id) => !ALWAYS_COLUMNS.includes(id)
);

/** What a Jellyfin playlist ENTRY can actually supply:
 * trackNumber/discNumber/genre/year live on the library `Track`, not on
 * `JellyfinPlaylistTrack` — Apple Music's own playlist "Songs" view has no
 * per-album track/disc column either, since a playlist isn't album-ordered.
 * Last Played/Plays/Skips ARE meaningful: they come from the local per-track
 * metadata store, keyed by jellyfinId, which every playlist entry carries. */
export const PLAYLIST_TOGGLEABLE_COLUMNS: string[] = ['artist', 'album', 'lastPlayed', 'playCount', 'skipCount'];

/** A column's grid track for a CSS-grid row (PlaylistView): its stored or
 * fixed width, else a flexible share no narrower than its own legibility
 * floor — the same sizing `colWidth` gives a `<table>`'s `<col>`. */
export function colGridTrack(col: ColumnDef, columnWidths: Record<string, number>): string {
  const px = colWidth(col, columnWidths);
  return px ?? `minmax(${col.minWidth}px, 1fr)`;
}

/** Horizontal padding either side of a cell's text — kept in sync with the
 * header/cell CSS padding so a fit-to-content width doesn't clip. */
export const FIT_CONTENT_PADDING_PX = 24;

/**
 * Double-click-to-fit: the width a column needs to
 * show its widest rendered VALUE without truncation — Finder/Music.app's own
 * "double-click a column divider" behaviour. `measureText` is injected so
 * this stays a pure function: the caller measures with a real canvas context
 * (and its own cell font) or, in a test, a synthetic measurer.
 */
export function computeFitWidth(values: string[], measureText: (text: string) => number, minWidth: number): number {
  let max = 0;
  for (const value of values) {
    if (!value) continue;
    const width = measureText(value);
    if (width > max) max = width;
  }
  return Math.max(minWidth, Math.ceil(max + FIT_CONTENT_PADDING_PX));
}

/**
 * What breaks a tie on each sort column, always ascending whichever way the
 * column itself is sorted: sorting by album plays each album in disc-then-track
 * order, as Apple Music and Spotify do.
 */
const TIE_BREAKS: Record<string, string[]> = {
  album: ['artist', 'discNumber', 'trackNumber'],
  artist: ['album', 'discNumber', 'trackNumber'],
  year: ['artist', 'album', 'discNumber', 'trackNumber'],
  genre: ['artist', 'album', 'discNumber', 'trackNumber'],
  title: ['artist', 'album'],
  trackNumber: ['album'],
  discNumber: ['album', 'trackNumber'],
};
const DEFAULT_TIE_BREAK = ['artist', 'album', 'discNumber', 'trackNumber'];

/**
 * Compare two tracks on one column, using the shared ordering model
 * (`#lib/utils/ordering.js`) for every name/title column so this always agrees
 * with the SQL collations in `src-tauri/src/ordering.rs`.
 */
function compareBy(a: Track, b: Track, colId: string): number {
  switch (colId) {
    case 'title':
      return compareFolded(a.name, b.name);
    case 'artist':
      return compareArtistNames(a.artistName, b.artistName);
    case 'album':
      return compareFolded(a.albumName, b.albumName);
    case 'genre':
      return compareFolded(formatGenreTag(a.genre), formatGenreTag(b.genre));
    case 'trackNumber':
      return a.trackNumber - b.trackNumber;
    case 'discNumber':
      return a.discNumber - b.discNumber;
    case 'time':
      return a.durationMs - b.durationMs;
    case 'year':
      return (a.year ?? 0) - (b.year ?? 0);
    case 'lastPlayed':
      return getTrackMeta(a.jellyfinId).lastPlayedMs - getTrackMeta(b.jellyfinId).lastPlayedMs;
    case 'playCount':
      return getTrackMeta(a.jellyfinId).playCount - getTrackMeta(b.jellyfinId).playCount;
    case 'skipCount':
      return getTrackMeta(a.jellyfinId).skipCount - getTrackMeta(b.jellyfinId).skipCount;
    default:
      return 0;
  }
}

/** Order two tracks by `colId` in `dir`, then by that column's tie-breaks. */
export function compareTracks(a: Track, b: Track, colId: string, dir: 1 | -1): number {
  if (colId === 'trackNumber' && a.discNumber !== b.discNumber) return (a.discNumber - b.discNumber) * dir;
  const primary = compareBy(a, b, colId) * dir;
  if (primary !== 0) return primary;
  for (const key of TIE_BREAKS[colId] ?? DEFAULT_TIE_BREAK) {
    const c = compareBy(a, b, key);
    if (c !== 0) return c;
  }
  return 0;
}
