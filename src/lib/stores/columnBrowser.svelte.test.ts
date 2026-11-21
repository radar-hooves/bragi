// The column browser used to follow only album clicks, because each call
// site pasted its own clearAllSelections()/selectX() pair. These cover the one
// helper every navigation entry point now goes through, and the navigation
// history's restore of what the browser was actually showing.
import { describe, it, expect, beforeEach, vi } from 'vitest';

vi.mock('#lib/services/tauri.js', () => ({
  queryLibraryView: vi.fn(),
  getSettings: vi.fn(),
  // ui.svelte.ts's layout effect debounce-saves via .catch() on this 300ms
  // after any test that touches ui state; a bare vi.fn() hands back undefined
  // instead of a promise, which crashes if that leaked timer fires while THIS
  // file is the active test (see the identical fix in ui.svelte.test.ts).
  updateSettings: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: vi.fn(() => ({ setDecorations: vi.fn() })),
}));

const { refreshLibraryView, getColumnBrowserState, syncColumnBrowserToSelection, selectGenre, selectArtist, snapshotColumnBrowser, restoreColumnBrowser } = await import('./columnBrowser.svelte');

const { navigateTo, navigateBack, initNavigation } = await import('./navigation.svelte');
const { setNetworkReachable } = await import('./offlineMode.svelte');
const { queryLibraryView } = await import('#lib/services/tauri.js');

function selection(): { genres: string[]; artists: string[]; albums: string[] } {
  const s = getColumnBrowserState();
  return {
    genres: [...s.selectedGenres],
    artists: [...s.selectedArtists],
    albums: [...s.selectedAlbums],
  };
}

describe('syncColumnBrowserToSelection', () => {
  beforeEach(() => {
    syncColumnBrowserToSelection();
  });

  it('selects the artist and album an album-open lands on', () => {
    syncColumnBrowserToSelection({ artist: 'Amorphis', albumId: 'album-1' });
    expect(selection()).toEqual({ genres: [], artists: ['Amorphis'], albums: ['album-1'] });
  });

  it('selects only the artist for an artist-open', () => {
    syncColumnBrowserToSelection({ artist: 'Amorphis' });
    expect(selection()).toEqual({ genres: [], artists: ['Amorphis'], albums: [] });
  });

  it('replaces a previous selection rather than accumulating', () => {
    syncColumnBrowserToSelection({ genre: 'Heavy Metal', artist: 'A', albumId: 'x' });
    syncColumnBrowserToSelection({ artist: 'B' });
    expect(selection()).toEqual({ genres: [], artists: ['B'], albums: [] });
  });

  it('resets to All / All / All when called with nothing', () => {
    syncColumnBrowserToSelection({ genre: 'Heavy Metal', artist: 'A', albumId: 'x' });
    syncColumnBrowserToSelection();
    expect(selection()).toEqual({ genres: [], artists: [], albums: [] });
  });

  it('ignores empty strings so a blank artist name cannot filter to nothing', () => {
    syncColumnBrowserToSelection({ artist: '', albumId: 'album-1' });
    expect(selection()).toEqual({ genres: [], artists: [], albums: ['album-1'] });
  });
});

describe('navigation history restores the column browser', () => {
  beforeEach(() => {
    syncColumnBrowserToSelection();
    initNavigation('albums');
  });

  it('returns an unfiltered entry to All / All / All', () => {
    navigateTo({ section: 'albums', selectedArtistId: null, selectedAlbumId: null });
    syncColumnBrowserToSelection({ artist: 'Amorphis', albumId: 'album-1' });
    navigateTo({
      section: 'tracks',
      selectedArtistId: null,
      selectedAlbumId: 'album-1',
      columnBrowserVisible: true,
    });

    navigateBack();
    expect(selection()).toEqual({ genres: [], artists: [], albums: [] });
  });

  it('restores a genre drill-down, which the album-only restore used to drop', () => {
    selectGenre('Heavy Metal', false);
    navigateTo({
      section: 'tracks',
      selectedArtistId: null,
      selectedAlbumId: null,
      columnBrowserVisible: true,
    });
    syncColumnBrowserToSelection({ artist: 'Amorphis', albumId: 'album-1' });
    navigateTo({
      section: 'tracks',
      selectedArtistId: null,
      selectedAlbumId: 'album-1',
      columnBrowserVisible: true,
    });

    navigateBack();
    expect(selection()).toEqual({ genres: ['Heavy Metal'], artists: [], albums: [] });
  });

  it('round-trips a multi-select snapshot', () => {
    selectArtist('A', false);
    selectArtist('B', true);
    const snap = snapshotColumnBrowser();
    syncColumnBrowserToSelection();
    restoreColumnBrowser(snap);
    expect(selection().artists).toEqual(['A', 'B']);
  });
});

// A server out of reach used to ask the backend for downloaded tracks only, so
// an album cached but not downloaded read "No songs found".
describe('refreshLibraryView when the server is unreachable', () => {
  it('still asks for the whole cached library', async () => {
    vi.mocked(queryLibraryView).mockResolvedValue({
      columnBrowser: { genres: [], artists: [], albums: [] },
      tracks: [],
    } as never);
    setNetworkReachable(false);
    try {
      await refreshLibraryView();
    } finally {
      setNetworkReachable(true);
    }
    expect(vi.mocked(queryLibraryView).mock.calls.at(-1)?.[0]).not.toHaveProperty('offlineOnly');
  });
});
