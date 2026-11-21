// Menus and drag-and-drop: the now-playing bar's own track context menu — pinned here so
// it can't silently diverge from the one TrackContextMenu definition.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent, screen } from '@testing-library/svelte';
import LcdPanel from './LcdPanel.svelte';
import type { Track } from '#lib/types/models.js';

let currentTrack: Track | null = null;
let isPlaying = false;

vi.mock('#lib/stores/player.svelte.js', () => ({
  getPlaybackState: () => ({ currentTrack, isPlaying, shuffle: false, repeat: 'off' }),
  getProgress: () => 0,
  getCurrentTimeFormatted: () => '0:00',
  getRemainingTimeFormatted: () => '-0:00',
}));

vi.mock('#lib/stores/fullscreenPlayer.svelte.js', () => ({
  openFullScreenPlayer: vi.fn(),
}));

const tauriSeek = vi.fn().mockResolvedValue(undefined);
const tauriResume = vi.fn().mockResolvedValue(undefined);
vi.mock('#lib/services/tauri.js', () => ({
  seek: (...args: unknown[]) => tauriSeek(...args),
  resume: (...args: unknown[]) => tauriResume(...args),
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

function makeTrack(): Track {
  return {
    jellyfinId: 'track-1',
    name: 'Now Playing Song',
    artistName: 'Artist',
    albumName: 'Album',
    albumJellyfinId: 'album-1',
    trackNumber: 1,
    discNumber: 1,
    durationMs: 200_000,
  };
}

describe('LcdPanel now-playing track menu', () => {
  afterEach(() => {
    cleanup();
    currentTrack = null;
    isPlaying = false;
    tauriSeek.mockClear();
    tauriResume.mockClear();
  });

  it('pins the item set: Play, Play Next, Add to Queue, Add to Playlist, Show in Library', async () => {
    currentTrack = makeTrack();
    render(LcdPanel, { props: { onShuffle: vi.fn(), onRepeat: vi.fn(), onSeek: vi.fn() } });

    await fireEvent.click(screen.getByRole('button', { name: /Track actions/ }));
    const items = screen.getAllByRole('menuitem').map((el) => el.textContent?.trim());
    expect(items).toEqual(['Play', 'Play Next', 'Add to Queue', 'Add to Playlist', 'Show in Library']);
  });

  it('Play restarts the current track in place (seek to 0, resume) rather than rebuilding the queue', async () => {
    currentTrack = makeTrack();
    isPlaying = false;
    render(LcdPanel, { props: { onShuffle: vi.fn(), onRepeat: vi.fn(), onSeek: vi.fn() } });

    await fireEvent.click(screen.getByRole('button', { name: /Track actions/ }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Play' }));

    expect(tauriSeek).toHaveBeenCalledWith(0);
    expect(tauriResume).toHaveBeenCalled();
  });

  it('Play seeks without resuming when already playing', async () => {
    currentTrack = makeTrack();
    isPlaying = true;
    render(LcdPanel, { props: { onShuffle: vi.fn(), onRepeat: vi.fn(), onSeek: vi.fn() } });

    await fireEvent.click(screen.getByRole('button', { name: /Track actions/ }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Play' }));

    expect(tauriSeek).toHaveBeenCalledWith(0);
    expect(tauriResume).not.toHaveBeenCalled();
  });

  it('Play Next and Add to Queue queue the current track through the shared queueActions mechanism', async () => {
    const track = makeTrack();
    currentTrack = track;
    render(LcdPanel, { props: { onShuffle: vi.fn(), onRepeat: vi.fn(), onSeek: vi.fn() } });

    await fireEvent.click(screen.getByRole('button', { name: /Track actions/ }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Play Next' }));
    expect(queueTrackNext).toHaveBeenCalledWith([track]);

    await fireEvent.click(screen.getByRole('button', { name: /Track actions/ }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Add to Queue' }));
    expect(queueTrackAtEnd).toHaveBeenCalledWith([track]);
  });

  it('Add to Playlist dispatches the cross-tree bridge event with this track', async () => {
    currentTrack = makeTrack();
    render(LcdPanel, { props: { onShuffle: vi.fn(), onRepeat: vi.fn(), onSeek: vi.fn() } });
    const handler = vi.fn();
    window.addEventListener('bragi-add-to-playlist', handler);

    await fireEvent.click(screen.getByRole('button', { name: /Track actions/ }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Add to Playlist' }));

    expect(handler).toHaveBeenCalledTimes(1);
    expect((handler.mock.calls[0]![0] as CustomEvent).detail).toEqual([currentTrack]);
    window.removeEventListener('bragi-add-to-playlist', handler);
  });

  it('a right-click on the artwork opens the same menu the trigger opens', async () => {
    currentTrack = makeTrack();
    const { container } = render(LcdPanel, { props: { onShuffle: vi.fn(), onRepeat: vi.fn(), onSeek: vi.fn() } });
    await fireEvent.contextMenu(container.querySelector('.lcd-artwork')!);
    expect(await screen.findByRole('menuitem', { name: 'Play' })).toBeTruthy();
  });

  it('renders no menu trigger when nothing is playing', () => {
    currentTrack = null;
    render(LcdPanel, { props: { onShuffle: vi.fn(), onRepeat: vi.fn(), onSeek: vi.fn() } });
    expect(screen.queryByRole('button', { name: /Track actions/ })).toBeNull();
  });
});
