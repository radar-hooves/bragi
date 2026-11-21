// Regression guard for the accent colour not reaching shadcn primitives.
// @poodle64/design-tokens registers the Tailwind utilities against --ds-color-*
// (--color-primary: var(--ds-color-primary)), so a switch/button styled with
// `bg-primary` ignores an override of the bare --primary alias and stays on the
// palette baked into src/app.css. Setting only the bare names is exactly the bug.
import { describe, it, expect, vi, beforeEach } from 'vitest';

const getSettings = vi.fn();

vi.mock('#lib/services/tauri.js', () => ({
  getSettings: () => getSettings(),
  // Returns Promise<void> in src/lib/services/tauri.ts, and the layout effect
  // calls .catch() on it from a 300ms debounce. A bare vi.fn() hands back
  // undefined, so whether the suite passes depends on the timer losing its
  // race with teardown — which it did, for the first time, under a package
  // upgrade that shifted import timing.
  updateSettings: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: vi.fn(() => ({ setDecorations: vi.fn() })),
}));

// mode-watcher is Bragi's one dark-mode mechanism (sveltekit-frontend.md) —
// stubbed here so these tests assert the store calls it correctly without
// depending on its real DOM/matchMedia-driven class toggling.
const setModeWatcherModeSpy = vi.fn();
vi.mock('mode-watcher', () => ({
  setMode: (...args: unknown[]) => setModeWatcherModeSpy(...args),
}));

const {
  ACCENT_COLOURS,
  setAccentColour,
  setThemeMode,
  getTrackListColumns,
  getTrackListColumnWidths,
  setTrackListColumns,
  setTrackListColumnWidths,
  getPlaylistColumns,
  getPlaylistColumnWidths,
  setPlaylistColumns,
  setPlaylistColumnWidths,
  deletePlaylistColumns,
  getGridTileSize,
  setGridTileSize,
  MIN_GRID_TILE_SIZE,
  MAX_GRID_TILE_SIZE,
  getUiState,
  setUiZoomLevel,
  stepUiZoomLevel,
  resetUiZoomLevel,
  UI_ZOOM_LEVELS,
} = await import('./ui.svelte');

describe('applyAccentColour', () => {
  beforeEach(() => {
    document.documentElement.removeAttribute('style');
  });

  it('moves the --ds-* override point the Tailwind utilities compile against', () => {
    setAccentColour('red');

    const style = document.documentElement.style;
    expect(style.getPropertyValue('--ds-color-primary')).toBe(ACCENT_COLOURS.red.primary);
    expect(style.getPropertyValue('--ds-color-ring')).toBe(ACCENT_COLOURS.red.primary);
  });

  it('still sets the P11 and bare shadcn tokens the app CSS reads directly', () => {
    setAccentColour('green');

    const style = document.documentElement.style;
    expect(style.getPropertyValue('--p11-accent')).toBe(ACCENT_COLOURS.green.primary);
    expect(style.getPropertyValue('--p11-accent-hover')).toBe(ACCENT_COLOURS.green.hover);
    expect(style.getPropertyValue('--primary')).toBe(ACCENT_COLOURS.green.primary);
    expect(style.getPropertyValue('--ring')).toBe(ACCENT_COLOURS.green.primary);
    expect(style.getPropertyValue('--accent')).toBe(ACCENT_COLOURS.green.subtle);
  });

  it('moves the focus-ring token the global :focus-visible rule reads', () => {
    setAccentColour('red');
    expect(document.documentElement.style.getPropertyValue('--p11-border-focus')).toBe(ACCENT_COLOURS.red.primary);

    setAccentColour('green');
    expect(document.documentElement.style.getPropertyValue('--p11-border-focus')).toBe(ACCENT_COLOURS.green.primary);
  });

  it('repoints every token when the accent changes again', () => {
    setAccentColour('red');
    setAccentColour('teal');

    const style = document.documentElement.style;
    expect(style.getPropertyValue('--ds-color-primary')).toBe(ACCENT_COLOURS.teal.primary);
    expect(style.getPropertyValue('--p11-accent')).toBe(ACCENT_COLOURS.teal.primary);
  });

  // Regression guard: --p11-selection and --p11-now-playing (TrackRow,
  // ArtistCard, AlbumListItem, ArtistAlbumSection) stayed the static
  // tokens.css blue no matter which accent was chosen — the accent reached
  // buttons and the focus ring but not the selected/playing row treatments
  // Apple Music and Spotify key off the same accent colour.
  it('carries the chosen accent into the selection and now-playing row tints', () => {
    setAccentColour('teal');

    const style = document.documentElement.style;
    expect(style.getPropertyValue('--p11-selection')).toBe('rgba(26, 188, 156, 0.18)');
    expect(style.getPropertyValue('--p11-now-playing')).toBe('rgba(26, 188, 156, 0.08)');

    setAccentColour('red');
    expect(style.getPropertyValue('--p11-selection')).toBe('rgba(231, 76, 60, 0.18)');
    expect(style.getPropertyValue('--p11-now-playing')).toBe('rgba(231, 76, 60, 0.08)');
  });
});

// Per-view track-list columns: Songs, Recently Played and Search each
// keep their own visibility/order/width, the way Apple Music remembers
// columns per playlist rather than sharing one global layout — the shared
// single `trackListColumns` this replaced let Recently Played's forced
// "Last Played" column leak into every other view.
describe('per-view track list columns', () => {
  it('gives each view its own default columns', () => {
    expect(getTrackListColumns('songs')).toEqual(['title', 'artist', 'album', 'time']);
    expect(getTrackListColumns('recentlyPlayed')).toEqual(['title', 'artist', 'album', 'time', 'lastPlayed']);
    expect(getTrackListColumns('search')).toEqual(['title', 'artist', 'album', 'time']);
  });

  it("changing one view's columns never touches another view's", () => {
    setTrackListColumns('songs', ['title', 'genre']);
    expect(getTrackListColumns('songs')).toEqual(['title', 'genre']);
    expect(getTrackListColumns('recentlyPlayed')).toEqual(['title', 'artist', 'album', 'time', 'lastPlayed']);
    expect(getTrackListColumns('search')).toEqual(['title', 'artist', 'album', 'time']);
  });

  it("changing one view's widths never touches another view's", () => {
    setTrackListColumnWidths('search', { artist: 200 });
    expect(getTrackListColumnWidths('search')).toEqual({ artist: 200 });
    expect(getTrackListColumnWidths('songs')).toEqual({});
    expect(getTrackListColumnWidths('recentlyPlayed')).toEqual({});
  });

  it('hiding Last Played in Recently Played leaves it alone everywhere else', () => {
    // The exact regression the shared store caused: Recently Played had to
    // force-inject lastPlayed on every render because there was nowhere to
    // remember "hidden, just for this view".
    setTrackListColumns('recentlyPlayed', ['title', 'artist', 'album', 'time']);
    expect(getTrackListColumns('recentlyPlayed')).toEqual(['title', 'artist', 'album', 'time']);
    expect(getTrackListColumns('songs')).not.toContain('lastPlayed');
  });
});

// Per-playlist track list columns: confirmed live
// against Apple Music — "the options you select will apply to that playlist
// only, and have to be re-selected for each playlist" — so each Jellyfin
// playlist id keeps its own visibility/order/width, never one shared layout.
describe('per-playlist track list columns', () => {
  it('gives a never-customised playlist the title/artist/album/time default', () => {
    expect(getPlaylistColumns('pl-fresh')).toEqual(['title', 'artist', 'album', 'time']);
    expect(getPlaylistColumnWidths('pl-fresh')).toEqual({});
  });

  it("changing one playlist's columns never touches another playlist's", () => {
    setPlaylistColumns('pl-a', ['title', 'genre']);
    expect(getPlaylistColumns('pl-a')).toEqual(['title', 'genre']);
    expect(getPlaylistColumns('pl-b')).toEqual(['title', 'artist', 'album', 'time']);
  });

  it("changing one playlist's widths never touches another playlist's", () => {
    setPlaylistColumnWidths('pl-c', { artist: 220 });
    expect(getPlaylistColumnWidths('pl-c')).toEqual({ artist: 220 });
    expect(getPlaylistColumnWidths('pl-d')).toEqual({});
  });

  it('persists across launches — loadUIState reads each playlist back from settings, a missing one falls back to default', async () => {
    const { loadUIState } = await import('./ui.svelte');
    getSettings.mockResolvedValueOnce({
      playlistTrackColumns: { 'pl-loaded': { columns: ['title', 'lastPlayed'], widths: { title: 300 } } },
    });

    await loadUIState();

    expect(getPlaylistColumns('pl-loaded')).toEqual(['title', 'lastPlayed']);
    expect(getPlaylistColumnWidths('pl-loaded')).toEqual({ title: 300 });
    expect(getPlaylistColumns('pl-never-loaded')).toEqual(['title', 'artist', 'album', 'time']);
  });

  // Fresh-eyes review: a deleted playlist's customisation must not linger in
  // this map forever — PlaylistView calls this once the delete itself succeeds.
  it('deletePlaylistColumns drops a customised entry back to the default, without touching another playlist', () => {
    setPlaylistColumns('pl-deleted', ['title', 'genre']);
    setPlaylistColumnWidths('pl-deleted', { title: 300 });
    setPlaylistColumns('pl-kept', ['title', 'lastPlayed']);

    deletePlaylistColumns('pl-deleted');

    expect(getPlaylistColumns('pl-deleted')).toEqual(['title', 'artist', 'album', 'time']);
    expect(getPlaylistColumnWidths('pl-deleted')).toEqual({});
    expect(getPlaylistColumns('pl-kept')).toEqual(['title', 'lastPlayed']); // untouched
  });

  it('deletePlaylistColumns on a never-customised playlist is a no-op', () => {
    expect(() => deletePlaylistColumns('pl-never-existed')).not.toThrow();
    expect(getPlaylistColumns('pl-never-existed')).toEqual(['title', 'artist', 'album', 'time']);
  });
});

// Per-view grid tile size (zooming): albums, artists,
// genres and recentlyAdded each remember their own size, the way Apple
// Music's View Options (⌘J) remembers icon size per view rather than
// sharing one global setting.
describe('per-view grid tile size', () => {
  beforeEach(() => {
    setGridTileSize('albums', 200);
    setGridTileSize('artists', 200);
    setGridTileSize('genres', 200);
    setGridTileSize('recentlyAdded', 200);
  });

  it('gives every grid view the same 200px default', () => {
    expect(getGridTileSize('albums')).toBe(200);
    expect(getGridTileSize('artists')).toBe(200);
    expect(getGridTileSize('genres')).toBe(200);
    expect(getGridTileSize('recentlyAdded')).toBe(200);
  });

  it("changing one view's tile size never touches another view's", () => {
    setGridTileSize('albums', 280);
    expect(getGridTileSize('albums')).toBe(280);
    expect(getGridTileSize('artists')).toBe(200);
    expect(getGridTileSize('genres')).toBe(200);
    expect(getGridTileSize('recentlyAdded')).toBe(200);
  });

  it('clamps to the 120-280 range in both directions', () => {
    setGridTileSize('genres', 1000);
    expect(getGridTileSize('genres')).toBe(MAX_GRID_TILE_SIZE);

    setGridTileSize('genres', 0);
    expect(getGridTileSize('genres')).toBe(MIN_GRID_TILE_SIZE);
  });

  it('persists across launches — loadUIState reads each view back from settings, missing views fall back to default', async () => {
    const { loadUIState } = await import('./ui.svelte');
    getSettings.mockResolvedValueOnce({ gridTileSizes: { albums: 280, artists: 120 } });

    await loadUIState();

    expect(getGridTileSize('albums')).toBe(280);
    expect(getGridTileSize('artists')).toBe(120);
    expect(getGridTileSize('genres')).toBe(200); // missing from the payload -> default
    expect(getGridTileSize('recentlyAdded')).toBe(200);
  });
});

// UI zoom: the ⌘/Ctrl+Plus/Minus/0 convention every
// Chromium/Electron interface uses (Spotify's desktop app included), since
// Bragi's window is a webview rather than native chrome — Music.app has no
// equivalent because it's plain AppKit. Steps and limits: docs/product.
describe('UI zoom stepping', () => {
  beforeEach(() => setUiZoomLevel(100));

  it('is a fixed 10%-step list from 80% to 130%, 100% default', () => {
    expect(UI_ZOOM_LEVELS).toEqual([80, 90, 100, 110, 120, 130]);
    expect(getUiState().uiZoomLevel).toBe(100);
  });

  it('steps up one level at a time', () => {
    stepUiZoomLevel(1);
    expect(getUiState().uiZoomLevel).toBe(110);
    stepUiZoomLevel(1);
    expect(getUiState().uiZoomLevel).toBe(120);
  });

  it('steps down one level at a time', () => {
    stepUiZoomLevel(-1);
    expect(getUiState().uiZoomLevel).toBe(90);
  });

  it('clamps at the top of the range instead of running off the list', () => {
    for (let i = 0; i < 10; i++) stepUiZoomLevel(1);
    expect(getUiState().uiZoomLevel).toBe(130);
  });

  it('clamps at the bottom of the range instead of running off the list', () => {
    for (let i = 0; i < 10; i++) stepUiZoomLevel(-1);
    expect(getUiState().uiZoomLevel).toBe(80);
  });

  it('⌘/Ctrl+0 resets to 100% from any level', () => {
    setUiZoomLevel(130);
    resetUiZoomLevel();
    expect(getUiState().uiZoomLevel).toBe(100);

    setUiZoomLevel(80);
    resetUiZoomLevel();
    expect(getUiState().uiZoomLevel).toBe(100);
  });

  it('persists across launches — loadUIState reads the level back from settings', async () => {
    const { loadUIState } = await import('./ui.svelte');
    getSettings.mockResolvedValueOnce({ uiZoomLevel: 120 });

    await loadUIState();

    expect(getUiState().uiZoomLevel).toBe(120);
  });
});

// Theme: follows the system appearance by default, with
// an explicit override, the Apple Music / Spotify convention — applied
// through mode-watcher (sveltekit-frontend.md), never a custom mechanism.
describe('theme mode', () => {
  beforeEach(() => {
    setModeWatcherModeSpy.mockClear();
  });

  it('defaults to system', () => {
    expect(getUiState().themeMode).toBe('system');
  });

  it('honours an explicit override by calling mode-watcher directly', () => {
    setThemeMode('dark');
    expect(getUiState().themeMode).toBe('dark');
    expect(setModeWatcherModeSpy).toHaveBeenCalledWith('dark');

    setThemeMode('light');
    expect(getUiState().themeMode).toBe('light');
    expect(setModeWatcherModeSpy).toHaveBeenCalledWith('light');
  });

  it('hands control back to the system preference', () => {
    setThemeMode('dark');
    setThemeMode('system');
    expect(getUiState().themeMode).toBe('system');
    expect(setModeWatcherModeSpy).toHaveBeenLastCalledWith('system');
  });

  it('persists across launches — loadUIState reads the mode back from settings and applies it', async () => {
    const { loadUIState } = await import('./ui.svelte');
    getSettings.mockResolvedValueOnce({ theme: 'light' });

    await loadUIState();

    expect(getUiState().themeMode).toBe('light');
    expect(setModeWatcherModeSpy).toHaveBeenCalledWith('light');
  });

  it('falls back to system on a missing or corrupt stored value', async () => {
    const { loadUIState } = await import('./ui.svelte');
    getSettings.mockResolvedValueOnce({ theme: 'sepia' });

    await loadUIState();

    expect(getUiState().themeMode).toBe('system');
    expect(setModeWatcherModeSpy).toHaveBeenCalledWith('system');
  });
});
