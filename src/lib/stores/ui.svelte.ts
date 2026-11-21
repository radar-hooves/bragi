import { getSettings, updateSettings } from '#lib/services/tauri.js';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { setMode as setModeWatcherMode } from 'mode-watcher';

export type AccentColour = 'blue' | 'purple' | 'pink' | 'red' | 'orange' | 'yellow' | 'green' | 'teal';

/** Follows the system light/dark appearance by default, with an explicit
 * override — the Apple Music / Spotify convention. Applied through
 * mode-watcher (never a custom theme store, per sveltekit-frontend.md);
 * persisted through the same settings round trip as every other display
 * preference (accent colour, zoom) so it survives across windows the way
 * mode-watcher's own localStorage does not reliably (the MiniPlayer is a
 * second Tauri window/webview). */
export type ThemeMode = 'system' | 'light' | 'dark';

/** Accent colour definitions with their CSS values */
export const ACCENT_COLOURS: Record<AccentColour, { primary: string; hover: string; subtle: string }> = {
  blue: { primary: '#4d8ee9', hover: '#6ba3f0', subtle: 'rgba(77, 142, 233, 0.12)' },
  purple: { primary: '#9b59b6', hover: '#a86bc4', subtle: 'rgba(155, 89, 182, 0.12)' },
  pink: { primary: '#e84393', hover: '#ec5fa5', subtle: 'rgba(232, 67, 147, 0.12)' },
  red: { primary: '#e74c3c', hover: '#eb6b5e', subtle: 'rgba(231, 76, 60, 0.12)' },
  orange: { primary: '#e67e22', hover: '#eb954a', subtle: 'rgba(230, 126, 34, 0.12)' },
  yellow: { primary: '#f1c40f', hover: '#f4d03f', subtle: 'rgba(241, 196, 15, 0.12)' },
  green: { primary: '#27ae60', hover: '#4cbb78', subtle: 'rgba(39, 174, 96, 0.12)' },
  teal: { primary: '#1abc9c', hover: '#48c9b0', subtle: 'rgba(26, 188, 156, 0.12)' },
};
export type ViewMode = 'grid' | 'list';
export type AlbumsSort = 'year-desc' | 'year-asc' | 'name-asc' | 'name-desc' | 'artist-asc';
export type ArtistsSort = 'name-asc' | 'name-desc' | 'albums-desc' | 'albums-asc';
export type GenresSort = 'name-asc' | 'name-desc' | 'tracks-desc' | 'time-desc';
export type ArtistAlbumsSort = 'year-desc' | 'year-asc' | 'name-asc' | 'name-desc';
export type RecentlyAddedSort = 'date-added-desc' | 'date-added-asc' | 'year-desc' | 'year-asc' | 'name-asc';
export type TracksSort = 'track-number' | 'name-asc' | 'name-desc' | 'artist-asc' | 'album-asc' | 'duration-asc' | 'duration-desc';
export type LibrarySection = 'albums' | 'artists' | 'tracks' | 'genres' | 'recentlyAdded' | 'recentlyPlayed' | 'queue' | 'playlists' | 'sharedPlaylists' | 'settings';

export type ColumnBrowserColumn = 'genres' | 'artists' | 'albums';

/** Every grid-shaped library view that shows a tile-size slider. Each keeps
 * its own size — Apple Music's View Options (⌘J) remembers icon size per
 * view rather than sharing one global setting, the same reasoning
 * already applied to track-list columns (see TrackListViewId below). */
export type GridViewId = 'albums' | 'artists' | 'genres' | 'recentlyAdded';
const GRID_VIEW_IDS: GridViewId[] = ['albums', 'artists', 'genres', 'recentlyAdded'];

/** Every track list that shows configurable columns. Each keeps its own
 * visibility, order and widths — Apple Music remembers columns per
 * playlist/view rather than sharing one global layout, and Bragi's single shared
 * layout was leaking between views (Recently Played had to
 * force-inject a column the user could never actually hide there). */
export type TrackListViewId = 'songs' | 'recentlyPlayed' | 'search';
const TRACK_LIST_VIEW_IDS: TrackListViewId[] = ['songs', 'recentlyPlayed', 'search'];

export interface TrackListViewColumnState {
  /** Visible column IDs, in display order. */
  columns: string[];
  /** Custom pixel widths, keyed by column ID; a column without an entry uses
   * its definition's default (`trackColumns.ts`). */
  widths: Record<string, number>;
}

const DEFAULT_TRACK_LIST_VIEW_COLUMNS: Record<TrackListViewId, string[]> = {
  songs: ['title', 'artist', 'album', 'time'],
  // Last Played is what makes this view useful — on by default, but (unlike
  // the old shared layout) the user can now actually hide it here without it also
  // vanishing from Songs.
  recentlyPlayed: ['title', 'artist', 'album', 'time', 'lastPlayed'],
  search: ['title', 'artist', 'album', 'time'],
};

function defaultTrackListViewColumns(): Record<TrackListViewId, TrackListViewColumnState> {
  return {
    songs: { columns: [...DEFAULT_TRACK_LIST_VIEW_COLUMNS.songs], widths: {} },
    recentlyPlayed: { columns: [...DEFAULT_TRACK_LIST_VIEW_COLUMNS.recentlyPlayed], widths: {} },
    search: { columns: [...DEFAULT_TRACK_LIST_VIEW_COLUMNS.search], widths: {} },
  };
}

/** A Jellyfin playlist's own column state — same shape as
 * `TrackListViewColumnState`, keyed by playlist id instead of a fixed view
 * id: confirmed live against Apple Music — "the
 * options you select will apply to that playlist only, and have to be
 * re-selected for each playlist." A playlist missing from the map (not yet
 * customised, or new) falls back to `DEFAULT_PLAYLIST_COLUMNS`. */
const DEFAULT_PLAYLIST_COLUMNS: string[] = ['title', 'artist', 'album', 'time'];

const DEFAULT_COLUMN_BROWSER_COLUMNS: ColumnBrowserColumn[] = ['genres', 'artists', 'albums'];

interface UIState {
  sidebarCollapsed: boolean;
  viewMode: ViewMode;
  artistViewMode: ViewMode;
  genreViewMode: ViewMode;
  currentSection: LibrarySection;
  columnBrowserVisible: boolean;
  columnBrowserHeight: number;
  columnBrowserColumns: ColumnBrowserColumn[];
  queueSidebarVisible: boolean;
  statusBarVisible: boolean;
  trackListViewColumns: Record<TrackListViewId, TrackListViewColumnState>;
  /** Per-playlist column state, keyed by Jellyfin playlist id — see `DEFAULT_PLAYLIST_COLUMNS` above. */
  playlistColumns: Record<string, TrackListViewColumnState>;
  gridTileSizes: Record<GridViewId, number>;
  searchQuery: string;
  isSearching: boolean;
  // Sort preferences
  albumsSort: AlbumsSort;
  artistsSort: ArtistsSort;
  genresSort: GenresSort;
  artistAlbumsSort: ArtistAlbumsSort;
  tracksSort: TracksSort;
  recentlyAddedSort: RecentlyAddedSort;
  // Theme (light/dark/system)
  themeMode: ThemeMode;
  // Accent colour
  accentColour: AccentColour;
  // UI zoom level
  uiZoomLevel: number;
  // Window decorations visibility (Linux-only, for tiling window managers)
  titleBarVisible: boolean;
}

const DEFAULT_COLUMN_BROWSER_HEIGHT = 200;
const MIN_COLUMN_BROWSER_HEIGHT = 100;
const MAX_COLUMN_BROWSER_HEIGHT = 600;

const DEFAULT_GRID_TILE_SIZE = 200;
const MIN_GRID_TILE_SIZE = 120;
const MAX_GRID_TILE_SIZE = 280;
const GRID_TILE_SIZE_STEP = 40; // 5 steps: 120, 160, 200, 240, 280

function defaultGridTileSizes(): Record<GridViewId, number> {
  return { albums: DEFAULT_GRID_TILE_SIZE, artists: DEFAULT_GRID_TILE_SIZE, genres: DEFAULT_GRID_TILE_SIZE, recentlyAdded: DEFAULT_GRID_TILE_SIZE };
}

const DEFAULT_UI_ZOOM_LEVEL = 100;
const MIN_UI_ZOOM_LEVEL = 80;
const MAX_UI_ZOOM_LEVEL = 130;
export const UI_ZOOM_LEVELS = [80, 90, 100, 110, 120, 130];

const initialUIState: UIState = {
  sidebarCollapsed: false,
  viewMode: 'grid',
  artistViewMode: 'list',
  genreViewMode: 'grid',
  currentSection: 'albums',
  columnBrowserVisible: false,
  columnBrowserHeight: DEFAULT_COLUMN_BROWSER_HEIGHT,
  columnBrowserColumns: DEFAULT_COLUMN_BROWSER_COLUMNS,
  queueSidebarVisible: false,
  statusBarVisible: true,
  trackListViewColumns: defaultTrackListViewColumns(),
  playlistColumns: {},
  gridTileSizes: defaultGridTileSizes(),
  searchQuery: '',
  isSearching: false,
  albumsSort: 'year-desc',
  artistsSort: 'name-asc',
  genresSort: 'name-asc',
  artistAlbumsSort: 'year-desc',
  tracksSort: 'track-number',
  recentlyAddedSort: 'date-added-desc',
  themeMode: 'system',
  accentColour: 'blue',
  uiZoomLevel: DEFAULT_UI_ZOOM_LEVEL,
  titleBarVisible: true,
};

let uiState = $state<UIState>({ ...initialUIState });

// A one-shot request to preselect a Settings category the next time the
// Settings view mounts (e.g. auth-expiry routes the user straight to
// Library). Transient navigation intent — never persisted.
export const SETTINGS_PANES = [
  'general',
  'playback',
  'library',
  'downloads',
  'services',
  'advanced',
  'about',
] as const;
export type SettingsPane = (typeof SETTINGS_PANES)[number];

let pendingSettingsCategory = $state<SettingsPane | null>(null);

/** Ask the Settings view to open on a specific category when it next renders. */
export function requestSettingsCategory(category: SettingsPane): void {
  pendingSettingsCategory = category;
}

/** Read and clear the pending Settings category (null if none was requested). */
export function consumeSettingsCategory(): SettingsPane | null {
  const c = pendingSettingsCategory;
  pendingSettingsCategory = null;
  return c;
}

// Persist layout state to SQLite via Tauri (debounced)
let saveTimeout: ReturnType<typeof setTimeout> | null = null;
let previousLayout = {
  sidebarCollapsed: initialUIState.sidebarCollapsed,
  viewMode: initialUIState.viewMode,
  artistViewMode: initialUIState.artistViewMode,
  genreViewMode: initialUIState.genreViewMode,
  columnBrowserVisible: initialUIState.columnBrowserVisible,
  columnBrowserHeight: initialUIState.columnBrowserHeight,
  columnBrowserColumns: initialUIState.columnBrowserColumns,
  queueSidebarVisible: initialUIState.queueSidebarVisible,
  statusBarVisible: initialUIState.statusBarVisible,
  trackListViewColumns: initialUIState.trackListViewColumns,
  playlistTrackColumns: initialUIState.playlistColumns,
  gridTileSizes: initialUIState.gridTileSizes,
  albumsSort: initialUIState.albumsSort,
  artistsSort: initialUIState.artistsSort,
  genresSort: initialUIState.genresSort,
  artistAlbumsSort: initialUIState.artistAlbumsSort,
  tracksSort: initialUIState.tracksSort,
  recentlyAddedSort: initialUIState.recentlyAddedSort,
  theme: initialUIState.themeMode,
  accentColour: initialUIState.accentColour,
  uiZoomLevel: initialUIState.uiZoomLevel,
  titleBarVisible: initialUIState.titleBarVisible,
};

$effect.root(() => {
  $effect(() => {
    const current = {
      sidebarCollapsed: uiState.sidebarCollapsed,
      viewMode: uiState.viewMode,
      artistViewMode: uiState.artistViewMode,
      genreViewMode: uiState.genreViewMode,
      columnBrowserVisible: uiState.columnBrowserVisible,
      columnBrowserHeight: uiState.columnBrowserHeight,
      columnBrowserColumns: uiState.columnBrowserColumns,
      queueSidebarVisible: uiState.queueSidebarVisible,
      statusBarVisible: uiState.statusBarVisible,
      trackListViewColumns: uiState.trackListViewColumns,
      playlistTrackColumns: uiState.playlistColumns,
      gridTileSizes: uiState.gridTileSizes,
      albumsSort: uiState.albumsSort,
      artistsSort: uiState.artistsSort,
      genresSort: uiState.genresSort,
      artistAlbumsSort: uiState.artistAlbumsSort,
      tracksSort: uiState.tracksSort,
      recentlyAddedSort: uiState.recentlyAddedSort,
      theme: uiState.themeMode,
      accentColour: uiState.accentColour,
      uiZoomLevel: uiState.uiZoomLevel,
      titleBarVisible: uiState.titleBarVisible,
    };

    if (
      current.sidebarCollapsed === previousLayout.sidebarCollapsed &&
      current.viewMode === previousLayout.viewMode &&
      current.artistViewMode === previousLayout.artistViewMode &&
      current.genreViewMode === previousLayout.genreViewMode &&
      current.columnBrowserVisible === previousLayout.columnBrowserVisible &&
      current.columnBrowserHeight === previousLayout.columnBrowserHeight &&
      JSON.stringify(current.columnBrowserColumns) === JSON.stringify(previousLayout.columnBrowserColumns) &&
      current.queueSidebarVisible === previousLayout.queueSidebarVisible &&
      current.statusBarVisible === previousLayout.statusBarVisible &&
      JSON.stringify(current.trackListViewColumns) === JSON.stringify(previousLayout.trackListViewColumns) &&
      JSON.stringify(current.playlistTrackColumns) === JSON.stringify(previousLayout.playlistTrackColumns) &&
      JSON.stringify(current.gridTileSizes) === JSON.stringify(previousLayout.gridTileSizes) &&
      current.albumsSort === previousLayout.albumsSort &&
      current.artistsSort === previousLayout.artistsSort &&
      current.genresSort === previousLayout.genresSort &&
      current.artistAlbumsSort === previousLayout.artistAlbumsSort &&
      current.tracksSort === previousLayout.tracksSort &&
      current.recentlyAddedSort === previousLayout.recentlyAddedSort &&
      current.theme === previousLayout.theme &&
      current.accentColour === previousLayout.accentColour &&
      current.uiZoomLevel === previousLayout.uiZoomLevel &&
      current.titleBarVisible === previousLayout.titleBarVisible
    ) {
      return;
    }

    previousLayout = { ...current };

    if (saveTimeout) clearTimeout(saveTimeout);
    saveTimeout = setTimeout(() => {
      updateSettings(current).catch((err) => console.error('Failed to persist UI layout:', err));
    }, 300);
  });
});

/** Get the UI state */
export function getUiState(): UIState {
  return uiState;
}

/** Whether the sidebar is expanded */
export function getSidebarExpanded(): boolean {
  return !uiState.sidebarCollapsed;
}

/** Whether search is active */
export function getHasSearchQuery(): boolean {
  return uiState.searchQuery.trim().length > 0;
}

/** Load persisted UI layout state from SQLite. Call once at app startup. */
export async function loadUIState(): Promise<void> {
  try {
    const settings = await getSettings();
    const viewMode = settings.viewMode === 'list' ? 'list' : 'grid';
    const artistViewMode = settings.artistViewMode === 'grid' ? 'grid' : 'list';
    const genreViewMode = settings.genreViewMode === 'list' ? 'list' : 'grid';
    const columnBrowserHeight = clampBrowserHeight(settings.columnBrowserHeight);
    const trackListViewColumns = parseTrackListViewColumns(settings.trackListViewColumns);
    const playlistColumns = parsePlaylistColumns(settings.playlistTrackColumns);
    const columnBrowserColumns = settings.columnBrowserColumns?.length > 0 ? (settings.columnBrowserColumns as ColumnBrowserColumn[]) : DEFAULT_COLUMN_BROWSER_COLUMNS;
    const albumsSort = (settings.albumsSort as AlbumsSort) ?? 'year-desc';
    const artistsSort = (settings.artistsSort as ArtistsSort) ?? 'name-asc';
    const genresSort = (settings.genresSort as GenresSort) ?? 'name-asc';
    const artistAlbumsSort = (settings.artistAlbumsSort as ArtistAlbumsSort) ?? 'year-desc';
    const tracksSort = (settings.tracksSort as TracksSort) ?? 'track-number';
    const recentlyAddedSort = (settings.recentlyAddedSort as RecentlyAddedSort) ?? 'date-added-desc';
    const gridTileSizes = parseGridTileSizes(settings.gridTileSizes);
    const statusBarVisible = settings.statusBarVisible ?? true;
    const themeMode = validateThemeMode(settings.theme);
    const accentColour = validateAccentColour(settings.accentColour);
    const uiZoomLevel = clampZoomLevel(settings.uiZoomLevel);
    const titleBarVisible = settings.titleBarVisible ?? true;
    previousLayout = {
      sidebarCollapsed: settings.sidebarCollapsed,
      viewMode,
      artistViewMode,
      genreViewMode,
      columnBrowserVisible: settings.columnBrowserVisible,
      columnBrowserHeight,
      columnBrowserColumns,
      queueSidebarVisible: settings.queueSidebarVisible,
      statusBarVisible,
      trackListViewColumns,
      playlistTrackColumns: playlistColumns,
      gridTileSizes,
      albumsSort,
      artistsSort,
      genresSort,
      artistAlbumsSort,
      tracksSort,
      recentlyAddedSort,
      theme: themeMode,
      accentColour,
      uiZoomLevel,
      titleBarVisible,
    };
    uiState.sidebarCollapsed = settings.sidebarCollapsed;
    uiState.viewMode = viewMode;
    uiState.artistViewMode = artistViewMode;
    uiState.genreViewMode = genreViewMode;
    uiState.columnBrowserVisible = settings.columnBrowserVisible;
    uiState.columnBrowserHeight = columnBrowserHeight;
    uiState.columnBrowserColumns = columnBrowserColumns;
    uiState.queueSidebarVisible = settings.queueSidebarVisible;
    uiState.statusBarVisible = statusBarVisible;
    uiState.trackListViewColumns = trackListViewColumns;
    uiState.playlistColumns = playlistColumns;
    uiState.gridTileSizes = gridTileSizes;
    uiState.albumsSort = albumsSort;
    uiState.artistsSort = artistsSort;
    uiState.genresSort = genresSort;
    uiState.artistAlbumsSort = artistAlbumsSort;
    uiState.tracksSort = tracksSort;
    uiState.recentlyAddedSort = recentlyAddedSort;
    uiState.themeMode = themeMode;
    uiState.accentColour = accentColour;
    uiState.uiZoomLevel = uiZoomLevel;
    uiState.titleBarVisible = titleBarVisible;
    // Apply the theme mode immediately — before accent, so the accent's
    // resulting selection/now-playing tint reads whichever surface theme is
    // now active (mode-watcher toggles the `.dark` class synchronously).
    applyThemeMode(themeMode);
    // Apply accent colour immediately
    applyAccentColour(accentColour);
    // Apply zoom level immediately
    applyZoomLevel(uiZoomLevel);
    // Apply window decorations (Linux-only setting)
    applyDecorations(titleBarVisible);
  } catch (err) {
    console.error('Failed to load UI layout:', err);
  }
}

// Actions
export function toggleSidebar(): void {
  uiState.sidebarCollapsed = !uiState.sidebarCollapsed;
}

export function setSidebarCollapsed(collapsed: boolean): void {
  uiState.sidebarCollapsed = collapsed;
}

export function setViewMode(viewMode: ViewMode): void {
  uiState.viewMode = viewMode;
}

export function setArtistViewMode(artistViewMode: ViewMode): void {
  uiState.artistViewMode = artistViewMode;
}

export function setGenreViewMode(genreViewMode: ViewMode): void {
  uiState.genreViewMode = genreViewMode;
}

export function setCurrentSection(currentSection: LibrarySection): void {
  clearSearch();
  uiState.currentSection = currentSection;
}

export function toggleColumnBrowser(): void {
  uiState.columnBrowserVisible = !uiState.columnBrowserVisible;
}

export function setColumnBrowserVisible(visible: boolean): void {
  uiState.columnBrowserVisible = visible;
}

export function setSearchQuery(searchQuery: string): void {
  uiState.searchQuery = searchQuery;
}

export function setIsSearching(isSearching: boolean): void {
  uiState.isSearching = isSearching;
}

export function clearSearch(): void {
  uiState.searchQuery = '';
  uiState.isSearching = false;
}

export function setColumnBrowserHeight(height: number): void {
  uiState.columnBrowserHeight = clampBrowserHeight(height);
}

function clampBrowserHeight(height: number): number {
  return Math.max(MIN_COLUMN_BROWSER_HEIGHT, Math.min(MAX_COLUMN_BROWSER_HEIGHT, height));
}

/** Merge whatever the backend stored over the per-view defaults: a view
 * missing from an older or corrupt payload falls back to its own default
 * rather than losing every view's layout to one bad key. */
function parseTrackListViewColumns(raw: Partial<Record<string, Partial<TrackListViewColumnState>>> | undefined): Record<TrackListViewId, TrackListViewColumnState> {
  const result = defaultTrackListViewColumns();
  if (!raw) return result;
  for (const view of TRACK_LIST_VIEW_IDS) {
    const entry = raw[view];
    if (entry?.columns?.length) result[view].columns = entry.columns;
    if (entry?.widths) result[view].widths = entry.widths;
  }
  return result;
}

/** Unlike `parseTrackListViewColumns`, keys here are arbitrary playlist ids,
 * not a fixed enum — every entry the backend actually stored is kept, and a
 * malformed one falls back to `DEFAULT_PLAYLIST_COLUMNS` rather than being
 * dropped, so one bad entry can't lose a whole playlist's customisation. */
function parsePlaylistColumns(
  raw: Partial<Record<string, Partial<TrackListViewColumnState>>> | undefined
): Record<string, TrackListViewColumnState> {
  const result: Record<string, TrackListViewColumnState> = {};
  if (!raw) return result;
  for (const [playlistId, entry] of Object.entries(raw)) {
    result[playlistId] = {
      columns: entry?.columns?.length ? entry.columns : [...DEFAULT_PLAYLIST_COLUMNS],
      widths: entry?.widths ?? {},
    };
  }
  return result;
}

function clampGridTileSize(size: number): number {
  return Math.max(MIN_GRID_TILE_SIZE, Math.min(MAX_GRID_TILE_SIZE, size));
}

/** Merge whatever the backend stored over the per-view defaults: a view
 * missing from an older or corrupt payload (including an older
 * install that still has the single flat number under the old key) falls
 * back to the shared default rather than losing every view's size. */
function parseGridTileSizes(raw: Partial<Record<string, number>> | undefined): Record<GridViewId, number> {
  const result = defaultGridTileSizes();
  if (!raw) return result;
  for (const view of GRID_VIEW_IDS) {
    const size = raw[view];
    if (typeof size === 'number') result[view] = clampGridTileSize(size);
  }
  return result;
}

/** List-row artwork thumbnail size in px, derived from a grid tile size on
 * the same 120-280 -> 32-64 scale the grid tile slider already uses. */
export function gridTileSizeToListArtworkSize(tileSize: number): number {
  return Math.round(32 + ((tileSize - MIN_GRID_TILE_SIZE) / (MAX_GRID_TILE_SIZE - MIN_GRID_TILE_SIZE)) * 32);
}

function clampZoomLevel(level: number): number {
  return Math.max(MIN_UI_ZOOM_LEVEL, Math.min(MAX_UI_ZOOM_LEVEL, level));
}

function validateAccentColour(colour: string | undefined): AccentColour {
  const validColours: AccentColour[] = ['blue', 'purple', 'pink', 'red', 'orange', 'yellow', 'green', 'teal'];
  if (colour && validColours.includes(colour as AccentColour)) {
    return colour as AccentColour;
  }
  return 'blue';
}

function validateThemeMode(theme: string | undefined): ThemeMode {
  if (theme === 'light' || theme === 'dark' || theme === 'system') {
    return theme;
  }
  return 'system';
}

/** Apply the theme mode through mode-watcher — the app's one dark-mode
 * mechanism (`sveltekit-frontend.md`). 'system' hands mode-watcher's own
 * media-query tracking back control; 'light'/'dark' is the explicit override. */
function applyThemeMode(themeMode: ThemeMode): void {
  setModeWatcherMode(themeMode);
}

/** `colours.primary` as an `r, g, b` triple, for building alpha-varied tints
 * (selection, now-playing) from the one hex value already defined per accent —
 * the same 6-hex-digit shape as ACCENT_COLOURS.primary and the --p11-* tokens,
 * never a general-purpose colour parser. */
function hexToRgbChannels(hex: string): string {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return `${r}, ${g}, ${b}`;
}

/** Apply accent colour by setting CSS custom properties on document root. */
function applyAccentColour(colour: AccentColour): void {
  if (typeof document !== 'undefined') {
    const colours = ACCENT_COLOURS[colour];
    const rgb = hexToRgbChannels(colours.primary);
    // P11 tokens (consumed by existing custom components)
    document.documentElement.style.setProperty('--p11-accent', colours.primary);
    document.documentElement.style.setProperty('--p11-accent-hover', colours.hover);
    document.documentElement.style.setProperty('--p11-accent-subtle', colours.subtle);
    // Selection and now-playing row tints were still the static tokens.css
    // blue regardless of the chosen accent — the accent reached buttons and
    // the focus ring but not the two row treatments Apple Music/Spotify key
    // off the same accent (found auditing this item).
    document.documentElement.style.setProperty('--p11-selection', `rgba(${rgb}, 0.18)`);
    document.documentElement.style.setProperty('--p11-now-playing', `rgba(${rgb}, 0.08)`);
    // The focus ring is its own token, not an alias of --p11-accent: the global
    // :focus-visible rule (src/lib/styles/tokens.css) and the per-row rules in
    // TrackRow/ColumnBrowserColumn read --p11-border-focus. Leaving it out is
    // what pinned every focus ring to the static blue in tokens.css.
    document.documentElement.style.setProperty('--p11-border-focus', colours.primary);
    // @poodle64/design-tokens override point. Tailwind registers the utilities
    // against these (--color-primary: var(--ds-color-primary)), NOT against the
    // bare shadcn names, so `bg-primary` / `ring-ring` — the Switch track, the
    // default Button, Checkbox, Progress, Tabs — only follow the accent when the
    // --ds-* token moves. Without these two, those primitives stay frozen on the
    // Bragi-blue defined in src/app.css.
    document.documentElement.style.setProperty('--ds-color-primary', colours.primary);
    document.documentElement.style.setProperty('--ds-color-ring', colours.primary);
    // shadcn tokens (consumed directly by src/lib/styles/tokens.css and the P11
    // components; redundant with the aliases above but kept explicit so a change
    // to the package's alias mapping cannot silently drop the accent).
    document.documentElement.style.setProperty('--primary', colours.primary);
    document.documentElement.style.setProperty('--ring', colours.primary);
    document.documentElement.style.setProperty('--accent', colours.subtle);
    document.documentElement.style.setProperty('--sidebar-primary', colours.primary);
    document.documentElement.style.setProperty('--sidebar-ring', colours.primary);
    document.documentElement.style.setProperty('--sidebar-accent', colours.subtle);
  }
}

/** Apply UI zoom level using CSS zoom property on root element. */
function applyZoomLevel(level: number): void {
  if (typeof document !== 'undefined') {
    document.documentElement.style.setProperty('zoom', `${level}%`);
  }
}

/** Apply window decorations (Linux-only for tiling window managers). */
async function applyDecorations(visible: boolean): Promise<void> {
  try {
    const window = getCurrentWindow();
    await window.setDecorations(visible);
    console.log(`Window decorations set to: ${visible}`);
  } catch (err) {
    console.error('Failed to set window decorations:', err);
  }
}

export { MIN_COLUMN_BROWSER_HEIGHT, MAX_COLUMN_BROWSER_HEIGHT };
export { MIN_GRID_TILE_SIZE, MAX_GRID_TILE_SIZE, GRID_TILE_SIZE_STEP };

/** Whether the Up Next panel is currently shown. */
export function getUpNextVisible(): boolean {
  return uiState.queueSidebarVisible;
}

export function setUpNextVisible(value: boolean): void {
  uiState.queueSidebarVisible = value;
}

export function toggleUpNext(): void {
  uiState.queueSidebarVisible = !uiState.queueSidebarVisible;
}

export function setStatusBarVisible(visible: boolean): void {
  uiState.statusBarVisible = visible;
}

export function getTrackListColumns(view: TrackListViewId): string[] {
  return uiState.trackListViewColumns[view].columns;
}

export function getTrackListColumnWidths(view: TrackListViewId): Record<string, number> {
  return uiState.trackListViewColumns[view].widths;
}

export function setTrackListColumns(view: TrackListViewId, columns: string[]): void {
  uiState.trackListViewColumns[view] = { ...uiState.trackListViewColumns[view], columns };
}

export function setTrackListColumnWidths(view: TrackListViewId, widths: Record<string, number>): void {
  uiState.trackListViewColumns[view] = { ...uiState.trackListViewColumns[view], widths };
}

export function getPlaylistColumns(playlistId: string): string[] {
  return uiState.playlistColumns[playlistId]?.columns ?? DEFAULT_PLAYLIST_COLUMNS;
}

export function getPlaylistColumnWidths(playlistId: string): Record<string, number> {
  return uiState.playlistColumns[playlistId]?.widths ?? {};
}

export function setPlaylistColumns(playlistId: string, columns: string[]): void {
  uiState.playlistColumns = {
    ...uiState.playlistColumns,
    [playlistId]: { columns, widths: getPlaylistColumnWidths(playlistId) },
  };
}

export function setPlaylistColumnWidths(playlistId: string, widths: Record<string, number>): void {
  uiState.playlistColumns = {
    ...uiState.playlistColumns,
    [playlistId]: { columns: getPlaylistColumns(playlistId), widths },
  };
}

/** Drops a deleted playlist's column customisation — called once the delete
 * itself succeeds, so this map doesn't grow forever with entries for
 * playlists that no longer exist. */
export function deletePlaylistColumns(playlistId: string): void {
  if (!(playlistId in uiState.playlistColumns)) return;
  const next = { ...uiState.playlistColumns };
  delete next[playlistId];
  uiState.playlistColumns = next;
}

export function setAlbumsSort(albumsSort: AlbumsSort): void {
  uiState.albumsSort = albumsSort;
}

export function setArtistsSort(artistsSort: ArtistsSort): void {
  uiState.artistsSort = artistsSort;
}

export function setGenresSort(genresSort: GenresSort): void {
  uiState.genresSort = genresSort;
}

export function setRecentlyAddedSort(recentlyAddedSort: RecentlyAddedSort): void {
  uiState.recentlyAddedSort = recentlyAddedSort;
}

export function setArtistAlbumsSort(artistAlbumsSort: ArtistAlbumsSort): void {
  uiState.artistAlbumsSort = artistAlbumsSort;
}

export function setTracksSort(tracksSort: TracksSort): void {
  uiState.tracksSort = tracksSort;
}

export function getGridTileSize(view: GridViewId): number {
  return uiState.gridTileSizes[view];
}

export function setGridTileSize(view: GridViewId, size: number): void {
  uiState.gridTileSizes[view] = clampGridTileSize(size);
}

export function setColumnBrowserColumns(columns: ColumnBrowserColumn[]): void {
  const validColumns = columns.length > 0 ? columns : DEFAULT_COLUMN_BROWSER_COLUMNS;
  uiState.columnBrowserColumns = validColumns;
}

export function toggleColumnBrowserColumn(column: ColumnBrowserColumn): void {
  const current = uiState.columnBrowserColumns;
  const isVisible = current.includes(column);

  if (isVisible && current.length === 1) {
    return;
  }

  const newColumns = isVisible
    ? current.filter((c) => c !== column)
    : [...current, column].sort((a, b) => {
        const order: ColumnBrowserColumn[] = ['genres', 'artists', 'albums'];
        return order.indexOf(a) - order.indexOf(b);
      });

  uiState.columnBrowserColumns = newColumns;
}

/** Explicit theme override — System, Light or Dark in Settings > Appearance. */
export function setThemeMode(themeMode: ThemeMode): void {
  applyThemeMode(themeMode);
  uiState.themeMode = themeMode;
}

export function setAccentColour(colour: AccentColour): void {
  applyAccentColour(colour);
  uiState.accentColour = colour;
}

export function setUiZoomLevel(level: number): void {
  const clamped = clampZoomLevel(level);
  applyZoomLevel(clamped);
  uiState.uiZoomLevel = clamped;
}

/** Step to the next/previous entry in UI_ZOOM_LEVELS — the ⌘/Ctrl+Plus/Minus
 * shortcut. A level that has drifted off the fixed list (should never
 * happen, but a corrupt settings payload could get here) steps from the
 * default rather than throwing. */
export function stepUiZoomLevel(direction: 1 | -1): void {
  const currentIndex = UI_ZOOM_LEVELS.indexOf(uiState.uiZoomLevel);
  const baseIndex = currentIndex === -1 ? UI_ZOOM_LEVELS.indexOf(DEFAULT_UI_ZOOM_LEVEL) : currentIndex;
  const nextIndex = Math.max(0, Math.min(UI_ZOOM_LEVELS.length - 1, baseIndex + direction));
  setUiZoomLevel(UI_ZOOM_LEVELS[nextIndex] ?? DEFAULT_UI_ZOOM_LEVEL);
}

/** ⌘/Ctrl+0 — reset UI zoom to 100%, the Chrome/Electron/Spotify convention. */
export function resetUiZoomLevel(): void {
  setUiZoomLevel(DEFAULT_UI_ZOOM_LEVEL);
}

export function setTitleBarVisible(visible: boolean): void {
  applyDecorations(visible);
  uiState.titleBarVisible = visible;
}
