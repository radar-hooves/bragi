import { describe, it, expect, vi, beforeEach } from 'vitest';

const ui = vi.hoisted(() => ({ query: '', section: 'artists' }));
vi.mock('./ui.svelte', () => ({
  clearSearch: () => { ui.query = ''; },
  getHasSearchQuery: () => !!ui.query.trim(),
  setCurrentSection: (section: string) => { ui.section = section; ui.query = ''; },
  setColumnBrowserVisible: vi.fn(),
}));
vi.mock('./columnBrowser.svelte', () => ({
  snapshotColumnBrowser: () => ({ genres: [], artists: [], albums: [] }),
  restoreColumnBrowser: vi.fn(),
}));
const { initNavigation, navigateTo, navigateBack, navigateForward, getCanGoBack, getCanGoForward } = await import('./navigation.svelte');

beforeEach(() => { ui.query = ''; ui.section = 'artists'; initNavigation('artists'); });

describe('search and navigation', () => {
  it('Back dismisses search even when there is no earlier history entry', () => {
    ui.query = 'ancient';
    expect(getCanGoBack()).toBe(true);
    navigateBack();
    expect(ui.query).toBe('');
    expect(ui.section).toBe('artists');
    expect(getCanGoBack()).toBe(false);
    expect(getCanGoForward()).toBe(false);
  });

  it('dismissing search does not consume the artist history entry', () => {
    const restored = vi.fn();
    window.addEventListener('bragi-nav-restore-artist', restored);
    navigateTo({ section: 'artists', selectedArtistId: 'artist', selectedAlbumId: null });
    navigateTo({ section: 'tracks', selectedArtistId: null, selectedAlbumId: 'album' });
    ui.section = 'tracks'; ui.query = 'ancient';
    navigateBack();
    expect(ui.section).toBe('tracks');
    expect(restored).not.toHaveBeenCalled();
    navigateBack();
    expect(ui.section).toBe('artists');
    expect((restored.mock.calls[0]?.[0] as CustomEvent).detail).toBe('artist');
    navigateForward();
    expect(ui.section).toBe('tracks');
    window.removeEventListener('bragi-nav-restore-artist', restored);
  });
});
