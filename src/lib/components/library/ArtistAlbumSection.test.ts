// Menus and drag-and-drop: the artist detail view's own track rows go through the ONE
// TrackContextMenu definition and the one shared drag payload every other
// track row uses — pinned here so neither can drift back into a local copy.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent, screen } from '@testing-library/svelte';
import ArtistAlbumSection from './ArtistAlbumSection.svelte';
import type { Album, Track } from '#lib/types/models.js';

vi.mock('#lib/services/tauri.js', () => ({
  getArtworkUrl: vi.fn().mockResolvedValue('http://example/art.jpg'),
}));

vi.mock('#lib/stores/player.svelte.js', () => ({
  getPlaybackState: () => ({ currentTrack: null, isPlaying: false }),
}));

const queueTrackNext = vi.fn();
const queueTrackAtEnd = vi.fn();
vi.mock('#lib/utils/queueActions.js', () => ({
  queueTrackNext: (...args: unknown[]) => queueTrackNext(...args),
  queueTrackAtEnd: (...args: unknown[]) => queueTrackAtEnd(...args),
}));

vi.mock('#lib/utils/revealInLibrary.svelte.js', () => ({
  revealTrackInLibrary: vi.fn(),
}));

function makeAlbum(): Album {
  return {
    jellyfinId: 'album-1',
    name: 'Test Album',
    artistName: 'Artist',
    artistJellyfinId: 'artist-1',
    year: 2020,
    trackCount: 1,
    downloadedTrackCount: 0,
  } as Album;
}

function makeTrack(): Track {
  return {
    jellyfinId: 'track-1',
    name: 'Track One',
    artistName: 'Artist',
    albumName: 'Test Album',
    albumJellyfinId: 'album-1',
    trackNumber: 1,
    discNumber: 1,
    durationMs: 180_000,
  };
}

function dragStartWith(setData: (type: string, data: string) => void): Event {
  const event = new Event('dragstart', { bubbles: true, cancelable: true });
  Object.defineProperty(event, 'dataTransfer', { value: { setData, effectAllowed: '' } });
  return event;
}

describe('ArtistAlbumSection', () => {
  afterEach(() => {
    cleanup();
    queueTrackNext.mockClear();
    queueTrackAtEnd.mockClear();
  });

  it('carries the one shared drag payload for its track row', () => {
    const { container } = render(ArtistAlbumSection, {
      props: { album: makeAlbum(), tracks: [makeTrack()], isLoadingTracks: false },
    });
    const row = container.querySelector<HTMLElement>('.track-row')!;
    expect(row.getAttribute('draggable')).toBe('true');

    const setData = vi.fn();
    row.dispatchEvent(dragStartWith(setData));
    const [, payload] = setData.mock.calls.find(([type]) => type === 'application/x-bragi-playlist')!;
    expect(JSON.parse(payload)).toEqual({ type: 'tracks', trackIds: ['track-1'] });
  });

  it('mounts the one TrackContextMenu definition with its own item set', async () => {
    render(ArtistAlbumSection, {
      props: {
        album: makeAlbum(),
        tracks: [makeTrack()],
        isLoadingTracks: false,
        onAddToPlaylist: vi.fn(),
      },
    });
    const trigger = screen.getByRole('button', { name: /Track actions/ });
    await fireEvent.click(trigger);

    const items = screen.getAllByRole('menuitem').map((el) => el.textContent?.trim());
    expect(items).toEqual(['Play', 'Play Next', 'Add to Queue', 'Add to Playlist', 'Show in Library']);
  });

  it('Play Next and Add to Queue queue this single row through the shared queueActions mechanism', async () => {
    const track = makeTrack();
    render(ArtistAlbumSection, {
      props: { album: makeAlbum(), tracks: [track], isLoadingTracks: false },
    });
    await fireEvent.click(screen.getByRole('button', { name: /Track actions/ }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Play Next' }));
    expect(queueTrackNext).toHaveBeenCalledWith([track]);

    await fireEvent.click(screen.getByRole('button', { name: /Track actions/ }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Add to Queue' }));
    expect(queueTrackAtEnd).toHaveBeenCalledWith([track]);
  });

  it("Play calls onTrackPlay with the track and its album's track list", async () => {
    const onTrackPlay = vi.fn();
    const track = makeTrack();
    render(ArtistAlbumSection, {
      props: { album: makeAlbum(), tracks: [track], isLoadingTracks: false, onTrackPlay },
    });
    await fireEvent.click(screen.getByRole('button', { name: /Track actions/ }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Play' }));
    expect(onTrackPlay).toHaveBeenCalledWith(track, [track]);
  });

  it('a right-click on the row opens the same menu as the trigger button', async () => {
    const { container } = render(ArtistAlbumSection, {
      props: { album: makeAlbum(), tracks: [makeTrack()], isLoadingTracks: false },
    });
    const row = container.querySelector<HTMLElement>('.track-row')!;
    await fireEvent.contextMenu(row);
    expect(await screen.findByRole('menuitem', { name: 'Play' })).toBeTruthy();
  });
});
