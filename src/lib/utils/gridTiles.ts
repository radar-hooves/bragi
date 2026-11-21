/**
 * The tile-grid column formula every auto-fill grid shares (AlbumGrid, ArtistGrid and GenresView each carried their own copy of
 * `GAP_PX`/`DEFAULT_TILE` and the same floor-division). One definition, kept
 * in sync with the CSS it must match: `--space-md` (the grid gap) and
 * `--grid-tile-size`'s default.
 */

export const GRID_GAP_PX = 12; // must match --space-md
export const DEFAULT_GRID_TILE_PX = 200; // must match --grid-tile-size default

/** How many `tilePx`-wide tiles, `gapPx` apart, fit an `auto-fill` grid `width` wide. */
export function gridColumnsForWidth(width: number, tilePx: number, gapPx: number = GRID_GAP_PX): number {
  return Math.max(1, Math.floor((width + gapPx) / (tilePx + gapPx)));
}
