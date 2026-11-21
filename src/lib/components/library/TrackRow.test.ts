// Sync/offline/downloads polish: offline with no local copy must grey the row out and refuse
// to play it — the Spotify reference behaviour ("anything you haven't
// downloaded is now grayed out and unavailable") — rather than let a click
// silently attempt (and fail) a stream with no network to reach.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { cleanup, render, fireEvent, screen } from '@testing-library/svelte';
import TrackRow from './TrackRow.svelte';
import type { ColumnDef } from './trackColumns';
import type { Track } from '#lib/types/models.js';

let offline = false;

vi.mock('#lib/stores/offlineMode.svelte.js', () => ({
  getOfflineMode: () => offline,
}));

vi.mock('#lib/stores/player.svelte.js', () => ({
  getPlaybackState: () => ({ currentTrack: null, isPlaying: false }),
}));

const columns: ColumnDef[] = [
  {
    id: 'title',
    label: 'Title',
    align: 'left',
    minWidth: 100,
    render: (t: Track) => t.name,
  },
];

function makeTrack(cachedFilePath?: string): Track {
  return {
    jellyfinId: 'track-1',
    name: 'Song',
    artistName: 'Artist',
    albumName: 'Album',
    albumJellyfinId: 'album-1',
    trackNumber: 1,
    discNumber: 1,
    durationMs: 200_000,
    cachedFilePath,
  };
}

function renderRow(track: Track, onClick = vi.fn()) {
  const utils = render(TrackRow, {
    props: { track, activeColumns: columns, showAddToPlaylist: true, striped: false, onClick },
  });
  return { ...utils, onClick };
}

describe('TrackRow offline availability', () => {
  afterEach(() => {
    cleanup();
    offline = false;
  });

  it('is not greyed out online, downloaded or not', () => {
    offline = false;
    const { container } = renderRow(makeTrack(undefined));
    expect(container.querySelector('.track-row')?.classList.contains('unavailable')).toBe(false);
  });

  it('is not greyed out offline when the track has a local copy', () => {
    offline = true;
    const { container } = renderRow(makeTrack('/cache/track-1.flac'));
    expect(container.querySelector('.track-row')?.classList.contains('unavailable')).toBe(false);
  });

  it('is greyed out, disabled, and titled offline with no local copy', () => {
    offline = true;
    const { container } = renderRow(makeTrack(undefined));
    const row = container.querySelector('.track-row');
    expect(row?.classList.contains('unavailable')).toBe(true);
    expect(row?.getAttribute('aria-disabled')).toBe('true');
    expect(row?.getAttribute('title')).toBe('Not available offline');
    expect(row?.getAttribute('draggable')).toBe('false');
  });

  it('never fires onClick for an unavailable row — never a silent stream attempt', async () => {
    offline = true;
    const { container, onClick } = renderRow(makeTrack(undefined));
    await fireEvent.click(container.querySelector('.track-row')!);
    expect(onClick).not.toHaveBeenCalled();
  });

  it('still fires onClick for an available row', async () => {
    offline = false;
    const { container, onClick } = renderRow(makeTrack(undefined));
    await fireEvent.click(container.querySelector('.track-row')!);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('hides the download button for an unavailable row (there is no network to fetch it)', () => {
    offline = true;
    const { container } = renderRow(makeTrack(undefined));
    expect(container.querySelector('button.download-btn')).toBeNull();
  });
});

describe('TrackRow context menu', () => {
  afterEach(cleanup);

  it('a right-click opens the same menu the "..." trigger opens', async () => {
    const { container } = renderRow(makeTrack(undefined));
    const row = container.querySelector('.track-row')!;
    expect(screen.queryByRole('menuitem')).toBeNull();

    await fireEvent.contextMenu(row);
    expect(await screen.findByRole('menuitem', { name: 'Play' })).toBeTruthy();
  });

  it('a right-click on an unselected row selects it, matching a plain click', async () => {
    const onClick = vi.fn();
    const { container } = renderRow(makeTrack(undefined), onClick);
    const row = container.querySelector('.track-row')!;

    await fireEvent.contextMenu(row);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('a right-click on an already-selected row leaves the selection untouched', async () => {
    const onClick = vi.fn();
    const utils = render(TrackRow, {
      props: { track: makeTrack(undefined), activeColumns: columns, showAddToPlaylist: true, striped: false, selected: true, onClick },
    });
    const row = utils.container.querySelector('.track-row')!;

    await fireEvent.contextMenu(row);
    expect(onClick).not.toHaveBeenCalled();
  });

  it('never opens the menu for an unavailable row', async () => {
    offline = true;
    const { container } = renderRow(makeTrack(undefined));
    const row = container.querySelector('.track-row')!;

    await fireEvent.contextMenu(row);
    expect(screen.queryByRole('menuitem')).toBeNull();
  });
});
