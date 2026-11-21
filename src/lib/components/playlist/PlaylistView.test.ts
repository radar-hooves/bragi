import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { tick } from 'svelte';
import { render, cleanup, fireEvent, screen } from '@testing-library/svelte';
import PlaylistView from './PlaylistView.svelte';
import { clearScrollMemory } from '#lib/utils/scrollMemory.svelte.js';
import type { JellyfinPlaylistDetail, JellyfinPlaylistTrack } from '#lib/types/models.js';

vi.mock('#lib/services/tauri.js', () => ({
  getJellyfinPlaylists: vi.fn().mockResolvedValue([]),
  createJellyfinPlaylist: vi.fn(),
  getJellyfinPlaylist: vi.fn().mockRejectedValue(new Error('no network in tests')),
  getCachedJellyfinPlaylist: vi.fn(),
  deleteJellyfinPlaylist: vi.fn(),
  renameJellyfinPlaylist: vi.fn(),
  addTracksToJellyfinPlaylist: vi.fn(),
  removeTracksFromJellyfinPlaylist: vi.fn(),
  moveJellyfinPlaylistItem: vi.fn().mockResolvedValue(true),
  // PlaylistView now reads/writes per-playlist column state through
  // $stores/ui.svelte, which persists through these
  // — a bare vi.fn() would hand back `undefined` and the layout effect's
  // `updateSettings(current).catch(...)` would throw once a column changes.
  getSettings: vi.fn().mockResolvedValue({}),
  updateSettings: vi.fn().mockResolvedValue(undefined),
}));

const { getCachedJellyfinPlaylist, getJellyfinPlaylist, getJellyfinPlaylists, removeTracksFromJellyfinPlaylist, deleteJellyfinPlaylist, renameJellyfinPlaylist, moveJellyfinPlaylistItem } =
  await import('#lib/services/tauri.js');
const {
  selectJellyfinPlaylist,
  loadJellyfinPlaylists,
  getJellyfinPlaylistState,
  requestPlaylistRename,
  requestPlaylistDelete,
  getPendingPlaylistIntent,
  clearPendingPlaylistIntent,
} = await import('#lib/stores/jellyfinPlaylist.svelte.js');
const { getPlaylistColumns, getPlaylistColumnWidths } = await import('#lib/stores/ui.svelte.js');

const queueTrackNext = vi.fn();
const queueTrackAtEnd = vi.fn();
vi.mock('#lib/utils/queueActions.js', () => ({
  queueTrackNext: (...args: unknown[]) => queueTrackNext(...args),
  queueTrackAtEnd: (...args: unknown[]) => queueTrackAtEnd(...args),
}));

const VIEWPORT_WIDTH = 800;
const VIEWPORT_HEIGHT = 400;
let widthDescriptor: PropertyDescriptor | undefined;
let heightDescriptor: PropertyDescriptor | undefined;

function makeTrack(i: number): JellyfinPlaylistTrack {
  return {
    jellyfinId: `track-${i}`,
    playlistItemId: `item-${i}`,
    name: `Song ${i}`,
    artistName: 'Artist',
    albumName: 'Album',
    albumJellyfinId: 'album-1',
    durationMs: 200_000,
  };
}

function makePlaylist(id: string, trackCount: number): JellyfinPlaylistDetail {
  return { jellyfinId: id, tracks: Array.from({ length: trackCount }, (_, i) => makeTrack(i)) };
}

async function renderWithPlaylist(id: string, trackCount: number) {
  vi.mocked(getCachedJellyfinPlaylist).mockResolvedValue(makePlaylist(id, trackCount));
  await selectJellyfinPlaylist(id);
  return render(PlaylistView, { props: {} });
}

/** Also seeds the playlist SUMMARY (name lives there, not in the detail payload). */
async function renderWithNamedPlaylist(id: string, name: string, trackCount: number) {
  vi.mocked(getJellyfinPlaylists).mockResolvedValue([{ jellyfinId: id, name, itemCount: trackCount }]);
  await loadJellyfinPlaylists();
  return renderWithPlaylist(id, trackCount);
}

describe('PlaylistView', () => {
  beforeEach(() => {
    widthDescriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientWidth');
    heightDescriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientHeight');
    Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
      configurable: true,
      get: () => VIEWPORT_WIDTH,
    });
    Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
      configurable: true,
      get: () => VIEWPORT_HEIGHT,
    });
  });

  afterEach(() => {
    cleanup();
    clearScrollMemory();
    if (widthDescriptor) Object.defineProperty(HTMLElement.prototype, 'clientWidth', widthDescriptor);
    if (heightDescriptor) Object.defineProperty(HTMLElement.prototype, 'clientHeight', heightDescriptor);
  });

  it('windows a large playlist: fewer rows mount than tracks exist', async () => {
    const { container } = await renderWithPlaylist('pl-1', 500);
    const rows = container.querySelectorAll('.track-row');
    expect(rows.length).toBeLessThan(500);
    expect(rows.length).toBeGreaterThan(0);
  });

  it('keeps the visible track numbers correct under windowing (absolute index, not the slice index)', async () => {
    const { container } = await renderWithPlaylist('pl-2', 500);
    const viewport = container.querySelector<HTMLElement>('.track-list')!;

    viewport.scrollTop = 200 * 40;
    viewport.dispatchEvent(new Event('scroll'));
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

    const numbers = Array.from(container.querySelectorAll('.track-row .col-number')).map((el) => parseInt(el.textContent ?? '', 10));
    // Track "#" is 1-based absolute position — every visible row's number must
    // still match its real place in the playlist, not its position within the
    // rendered slice (which would restart at 1 every time the window moved).
    for (let i = 1; i < numbers.length; i++) {
      expect(numbers[i]).toBe(numbers[i - 1]! + 1);
    }
    expect(numbers[0]).toBeGreaterThan(1);
  });

  it('remembers scroll position per playlist id and resets for a different playlist', async () => {
    const first = await renderWithPlaylist('pl-a', 500);
    const viewport1 = first.container.querySelector<HTMLElement>('.track-list')!;
    viewport1.scrollTop = 150 * 40;
    first.unmount();

    const second = await renderWithPlaylist('pl-a', 500);
    // The restore is queued a microtask out, so it can never re-enter
    // Svelte's own scheduler mid-effect — give it one turn.
    await Promise.resolve();
    const viewport2 = second.container.querySelector<HTMLElement>('.track-list')!;
    expect(viewport2.scrollTop).toBe(150 * 40);
    second.unmount();

    const third = await renderWithPlaylist('pl-b', 500);
    await Promise.resolve();
    const viewport3 = third.container.querySelector<HTMLElement>('.track-list')!;
    expect(viewport3.scrollTop).toBe(0); // a different playlist starts fresh
  });

  it('keeps the dragged row mounted if autoscroll carries it out of the virtualised window mid-drag', async () => {
    // Both WebKitGTK and WKWebView implement the HTML DnD spec's recommended
    // autoscroll: holding a drag near the scroll container's edge scrolls it
    // natively, outside any handler this app writes. That can carry the
    // dragged row out of the virtualised window; the keyed {#each} would then
    // destroy its DOM node, and a browser drag session dies the instant its
    // source node leaves the document.
    const { container } = await renderWithPlaylist('pl-drag', 500);
    const viewport = container.querySelector<HTMLElement>('.track-list')!;
    const firstRow = container.querySelectorAll<HTMLElement>('.track-row')[0]!;
    expect(container.querySelector('.track-row.keep-alive')).toBeFalsy(); // nothing dragging yet

    await fireEvent.dragStart(firstRow);

    // Simulate the webview auto-scrolling deep during the drag.
    viewport.scrollTop = 300 * 40;
    viewport.dispatchEvent(new Event('scroll'));
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

    // The window has moved on without it, but the dragged row's own DOM node
    // must still exist, kept out of flow so it doesn't displace the rows
    // actually on screen.
    expect(container.querySelector('.track-row.keep-alive')).toBeTruthy();

    await fireEvent.dragEnd(firstRow);
    expect(container.querySelector('.track-row.keep-alive')).toBeFalsy(); // cleared once the drag ends
  });

  describe('selection and keyboard', () => {
    function rowFor(container: HTMLElement, itemId: string): HTMLElement {
      return container.querySelector<HTMLElement>(`[data-playlist-item-id="${itemId}"]`)!;
    }

    async function press(el: HTMLElement, key: string, opts: Partial<KeyboardEventInit> = {}): Promise<void> {
      el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...opts }));
      await tick();
    }

    it('a plain click selects exactly that row', async () => {
      const { container } = await renderWithPlaylist('pl-sel-1', 5);
      await fireEvent.click(rowFor(container, 'item-1'));
      expect(rowFor(container, 'item-1').classList.contains('selected')).toBe(true);
      expect(rowFor(container, 'item-0').classList.contains('selected')).toBe(false);
    });

    it('Shift-click selects the range from the last click', async () => {
      const { container } = await renderWithPlaylist('pl-sel-2', 5);
      await fireEvent.click(rowFor(container, 'item-0'));
      await fireEvent.click(rowFor(container, 'item-2'), { shiftKey: true });
      for (const id of ['item-0', 'item-1', 'item-2']) {
        expect(rowFor(container, id).classList.contains('selected')).toBe(true);
      }
      expect(rowFor(container, 'item-3').classList.contains('selected')).toBe(false);
    });

    it('ArrowDown moves the active row and Return plays it, starting playback there', async () => {
      const onTrackPlay = vi.fn();
      vi.mocked(getCachedJellyfinPlaylist).mockResolvedValue(makePlaylist('pl-sel-3', 3));
      await selectJellyfinPlaylist('pl-sel-3');
      const { container } = render(PlaylistView, { props: { onTrackPlay } });
      const viewport = container.querySelector<HTMLElement>('.track-list')!;

      await press(viewport, 'ArrowDown');
      expect(rowFor(container, 'item-0').classList.contains('active')).toBe(true);
      await press(viewport, 'Enter');
      expect(onTrackPlay).toHaveBeenCalledWith(expect.objectContaining({ playlistItemId: 'item-0' }));
    });

    async function selectTwoAndPressDelete(id: string): Promise<void> {
      const playlist = makePlaylist(id, 5);
      vi.mocked(removeTracksFromJellyfinPlaylist).mockClear();
      vi.mocked(removeTracksFromJellyfinPlaylist).mockImplementation(async (pid, entryIds) => ({
        jellyfinId: pid,
        tracks: playlist.tracks.filter((t) => !entryIds.includes(t.playlistItemId)),
      }));
      vi.mocked(getCachedJellyfinPlaylist).mockResolvedValue(playlist);
      await selectJellyfinPlaylist(id);
      const { container } = render(PlaylistView, { props: {} });
      const viewport = container.querySelector<HTMLElement>('.track-list')!;

      await fireEvent.click(rowFor(container, 'item-1'));
      await fireEvent.click(rowFor(container, 'item-2'), { shiftKey: true });
      await press(viewport, 'Delete');
    }

    it('Delete asks first, then removes the selected tracks from THIS playlist, not the library', async () => {
      await selectTwoAndPressDelete('pl-sel-4');

      // Removing edits the playlist in Jellyfin, so nothing happens until the
      // confirmation is accepted.
      expect(removeTracksFromJellyfinPlaylist).not.toHaveBeenCalled();
      await fireEvent.click(await screen.findByRole('button', { name: 'Remove' }));

      expect(removeTracksFromJellyfinPlaylist).toHaveBeenCalledWith('pl-sel-4', ['item-1', 'item-2']);
    });

    it('cancelling the confirmation removes nothing', async () => {
      await selectTwoAndPressDelete('pl-sel-cancel');

      await fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }));
      await tick();

      expect(removeTracksFromJellyfinPlaylist).not.toHaveBeenCalled();
    });

    it('selection survives a row scrolling out of the virtualised window and back', async () => {
      const { container } = await renderWithPlaylist('pl-sel-5', 300);
      const viewport = container.querySelector<HTMLElement>('.track-list')!;

      async function scrollTo(top: number): Promise<void> {
        viewport.scrollTop = top;
        viewport.dispatchEvent(new Event('scroll'));
        await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
        await tick();
      }

      // The initial mount restores a remembered scroll position on its own
      // deferred microtask; scroll once first so that one-shot restore
      // is spent before this test starts asserting on scroll state of its own.
      await scrollTo(1);
      await scrollTo(250 * 40);

      const deepRow = container.querySelector<HTMLElement>('[data-playlist-item-id]')!;
      const deepId = deepRow.dataset.playlistItemId!;
      // A row well past the always-mounted top rows — otherwise scrolling back
      // to 0 below wouldn't actually unmount it, and the test would prove nothing.
      expect(Number(deepId.replace('item-', ''))).toBeGreaterThan(30);

      await fireEvent.click(deepRow);
      expect(deepRow.classList.contains('selected')).toBe(true);

      await scrollTo(0);
      expect(container.querySelector(`[data-playlist-item-id="${deepId}"]`)).toBeFalsy();

      await scrollTo(250 * 40);
      expect(rowFor(container, deepId).classList.contains('selected')).toBe(true);
    }, 15000);
  });

  describe('rename in place (Apple Music reference: click the name, type, commit)', () => {
    it('clicking the name enters edit mode, and Enter commits the rename', async () => {
      vi.mocked(renameJellyfinPlaylist).mockResolvedValue(undefined);
      const { container } = await renderWithNamedPlaylist('pl-rename-1', 'Old Name', 2);

      await fireEvent.click(screen.getByTitle('Click to rename'));
      const input = container.querySelector<HTMLInputElement>('.playlist-name-input')!;
      expect(input).toBeTruthy();

      await fireEvent.input(input, { target: { value: 'New Name' } });
      await fireEvent.keyDown(input, { key: 'Enter' });

      expect(renameJellyfinPlaylist).toHaveBeenCalledWith('pl-rename-1', 'New Name');
      expect(container.querySelector('.playlist-name-input')).toBeFalsy(); // back to display mode
      expect(getJellyfinPlaylistState().playlists.find((p) => p.jellyfinId === 'pl-rename-1')?.name).toBe('New Name');
    });

    it('Escape cancels without renaming', async () => {
      vi.mocked(renameJellyfinPlaylist).mockClear();
      const { container } = await renderWithNamedPlaylist('pl-rename-2', 'Keep Me', 2);

      await fireEvent.click(screen.getByTitle('Click to rename'));
      const input = container.querySelector<HTMLInputElement>('.playlist-name-input')!;
      await fireEvent.input(input, { target: { value: 'Nope' } });
      await fireEvent.keyDown(input, { key: 'Escape' });

      expect(renameJellyfinPlaylist).not.toHaveBeenCalled();
      expect(container.querySelector('.playlist-name-input')).toBeFalsy();
      expect(screen.getByText('Keep Me')).toBeTruthy();
    });

    it('blurring the field also commits the rename', async () => {
      vi.mocked(renameJellyfinPlaylist).mockResolvedValue(undefined);
      const { container } = await renderWithNamedPlaylist('pl-rename-3', 'Blur Me', 2);

      await fireEvent.click(screen.getByTitle('Click to rename'));
      const input = container.querySelector<HTMLInputElement>('.playlist-name-input')!;
      await fireEvent.input(input, { target: { value: 'Blurred Name' } });
      await fireEvent.blur(input);

      expect(renameJellyfinPlaylist).toHaveBeenCalledWith('pl-rename-3', 'Blurred Name');
    });

    it('an unchanged or blank name is never submitted', async () => {
      vi.mocked(renameJellyfinPlaylist).mockClear();
      const { container } = await renderWithNamedPlaylist('pl-rename-4', 'Same Name', 2);

      await fireEvent.click(screen.getByTitle('Click to rename'));
      const input = container.querySelector<HTMLInputElement>('.playlist-name-input')!;
      await fireEvent.input(input, { target: { value: '   ' } });
      await fireEvent.keyDown(input, { key: 'Enter' });

      expect(renameJellyfinPlaylist).not.toHaveBeenCalled();
    });
  });

  describe('delete confirmation is a proper dialog, not inline Yes/No', () => {
    it('does nothing until the dialog is confirmed, and the trigger reads "Delete" (not an inline Yes/No pair)', async () => {
      vi.mocked(deleteJellyfinPlaylist).mockResolvedValue(undefined);
      const { container } = await renderWithNamedPlaylist('pl-del-1', 'Doomed', 2);

      const trigger = container.querySelector<HTMLElement>('.playlist-actions > .action-btn.danger')!;
      expect(trigger.textContent?.trim()).toBe('Delete');
      await fireEvent.click(trigger);

      // The inline "Delete playlist? Yes/No" strip must be gone — a real
      // dialog with its own title renders instead.
      expect(await screen.findByText('Delete playlist?')).toBeTruthy();
      expect(screen.queryByText('Yes')).toBeFalsy();
      expect(screen.queryByText('No')).toBeFalsy();
      expect(deleteJellyfinPlaylist).not.toHaveBeenCalled();

      await fireEvent.click(document.querySelector<HTMLElement>('.confirm-delete-btn')!);
      expect(deleteJellyfinPlaylist).toHaveBeenCalledWith('pl-del-1');
    });

    it('Cancel in the dialog deletes nothing', async () => {
      vi.mocked(deleteJellyfinPlaylist).mockClear();
      const { container } = await renderWithNamedPlaylist('pl-del-2', 'Spared', 2);
      const trigger = container.querySelector<HTMLElement>('.playlist-actions > .action-btn.danger')!;
      await fireEvent.click(trigger);

      await screen.findByText('Delete playlist?');
      await fireEvent.click(document.querySelector<HTMLElement>('.cancel-btn')!);

      expect(deleteJellyfinPlaylist).not.toHaveBeenCalled();
    });
  });

  describe('a pending rename/delete intent from the sidebar row menu', () => {
    afterEach(() => clearPendingPlaylistIntent());

    it('enters rename mode once this playlist has loaded, and clears the intent', async () => {
      vi.mocked(getCachedJellyfinPlaylist).mockResolvedValue(makePlaylist('pl-intent-1', 2));
      requestPlaylistRename('pl-intent-1');
      await selectJellyfinPlaylist('pl-intent-1');
      const { container } = render(PlaylistView, { props: {} });

      const input = await screen.findByLabelText('Playlist name');
      expect(input).toBeTruthy();
      expect(container.querySelector('.playlist-name-input')).toBeTruthy();
      expect(getPendingPlaylistIntent()).toBeNull();
    });

    it('opens the delete-confirmation dialog once this playlist has loaded, and clears the intent', async () => {
      vi.mocked(getCachedJellyfinPlaylist).mockResolvedValue(makePlaylist('pl-intent-2', 2));
      requestPlaylistDelete('pl-intent-2');
      await selectJellyfinPlaylist('pl-intent-2');
      render(PlaylistView, { props: {} });

      expect(await screen.findByText('Delete playlist?')).toBeTruthy();
      expect(getPendingPlaylistIntent()).toBeNull();
    });

    it('never acts on an intent that does not target the playlist actually on screen', async () => {
      // A same-tick race in principle (selectJellyfinPlaylist itself clears a
      // superseded intent — see below), but this is the consuming side's own
      // guard, kept as a second line of defence.
      vi.mocked(getCachedJellyfinPlaylist).mockResolvedValue(makePlaylist('pl-intent-3', 2));
      requestPlaylistDelete('some-other-playlist-id');
      await selectJellyfinPlaylist('pl-intent-3');
      render(PlaylistView, { props: {} });

      expect(screen.queryByText('Delete playlist?')).toBeNull();
    });

    // Right-click "Workout", choose Delete, then — before it loads — open
    // "Chill" instead: the Workout intent must not survive to ambush whoever
    // opens Workout normally later.
    it('a superseded intent (a different playlist selected before it acts) never fires later, even once its original target loads normally', async () => {
      vi.mocked(getCachedJellyfinPlaylist).mockImplementation(async (id: string) =>
        id === 'pl-workout' ? makePlaylist('pl-workout', 2) : makePlaylist('pl-chill', 1)
      );

      requestPlaylistDelete('pl-workout');
      expect(getPendingPlaylistIntent()).toEqual({ type: 'delete', playlistId: 'pl-workout' });

      // Selects Chill before Workout's intent is ever consumed — supersedes it.
      await selectJellyfinPlaylist('pl-chill');
      expect(getPendingPlaylistIntent()).toBeNull();

      // Later, an ordinary open of Workout — no menu, no fresh intent.
      await selectJellyfinPlaylist('pl-workout');
      render(PlaylistView, { props: {} });

      expect(screen.queryByText('Delete playlist?')).toBeNull();
      expect(getPendingPlaylistIntent()).toBeNull();
    });

    it('drops the intent if its own target playlist fails to load, rather than leaving it for a later successful open', async () => {
      vi.mocked(getCachedJellyfinPlaylist).mockResolvedValue(null);
      vi.mocked(getJellyfinPlaylist).mockRejectedValueOnce(new Error('offline'));

      requestPlaylistRename('pl-flaky');
      await selectJellyfinPlaylist('pl-flaky'); // fails to load — see mock above
      expect(getPendingPlaylistIntent()).toBeNull();

      // A later, successful open of the very same playlist raises no rename.
      vi.mocked(getJellyfinPlaylist).mockRejectedValue(new Error('no network in tests'));
      vi.mocked(getCachedJellyfinPlaylist).mockResolvedValue(makePlaylist('pl-flaky', 1));
      await selectJellyfinPlaylist('pl-flaky');
      render(PlaylistView, { props: {} });

      expect(screen.queryByLabelText('Playlist name')).toBeNull();
    });
  });

  describe('track row menu — the one TrackContextMenu definition', () => {
    function rowFor(container: HTMLElement, itemId: string): HTMLElement {
      return container.querySelector<HTMLElement>(`[data-playlist-item-id="${itemId}"]`)!;
    }

    it('pins the item set, ending with Remove from Playlist', async () => {
      vi.mocked(getCachedJellyfinPlaylist).mockResolvedValue(makePlaylist('pl-menu-1', 2));
      await selectJellyfinPlaylist('pl-menu-1');
      const { container } = render(PlaylistView, { props: { onAddToPlaylist: vi.fn() } });
      const trigger = rowFor(container, 'item-0').querySelector<HTMLElement>('.track-menu-trigger')!;
      await fireEvent.click(trigger);

      const items = screen.getAllByRole('menuitem').map((el) => el.textContent?.trim());
      expect(items).toEqual(['Play', 'Play Next', 'Add to Queue', 'Add to Playlist', 'Show in Library', 'Remove from Playlist']);
    });

    it('Remove from Playlist asks first, in the same confirmation the row used to gate on', async () => {
      vi.mocked(removeTracksFromJellyfinPlaylist).mockClear();
      const { container } = await renderWithPlaylist('pl-menu-2', 2);
      const trigger = rowFor(container, 'item-0').querySelector<HTMLElement>('.track-menu-trigger')!;
      await fireEvent.click(trigger);
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Remove from Playlist' }));

      expect(await screen.findByText('Remove from playlist?')).toBeTruthy();
      expect(removeTracksFromJellyfinPlaylist).not.toHaveBeenCalled();

      await fireEvent.click(await screen.findByRole('button', { name: 'Remove' }));
      expect(removeTracksFromJellyfinPlaylist).toHaveBeenCalledWith('pl-menu-2', ['item-0']);
    });

    it('Add to Playlist on the row calls onAddToPlaylist with the Track it maps to', async () => {
      const onAddToPlaylist = vi.fn();
      vi.mocked(getCachedJellyfinPlaylist).mockResolvedValue(makePlaylist('pl-menu-3', 1));
      await selectJellyfinPlaylist('pl-menu-3');
      const { container } = render(PlaylistView, { props: { onAddToPlaylist } });

      const trigger = rowFor(container, 'item-0').querySelector<HTMLElement>('.track-menu-trigger')!;
      await fireEvent.click(trigger);
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Add to Playlist' }));

      expect(onAddToPlaylist).toHaveBeenCalledWith([expect.objectContaining({ jellyfinId: 'track-0' })]);
    });

    it('a right-click on the row opens the same menu the trigger button opens', async () => {
      const { container } = await renderWithPlaylist('pl-menu-4', 2);
      const row = rowFor(container, 'item-0');

      await fireEvent.contextMenu(row);
      expect(await screen.findByRole('menuitem', { name: 'Play' })).toBeTruthy();
    });

    it('selecting five and right-clicking one queues all five with Add to Queue — not just the clicked row', async () => {
      queueTrackAtEnd.mockClear();
      const { container } = await renderWithPlaylist('pl-menu-5', 5);
      await fireEvent.click(rowFor(container, 'item-0'));
      await fireEvent.click(rowFor(container, 'item-4'), { shiftKey: true }); // selects item-0..4

      await fireEvent.contextMenu(rowFor(container, 'item-2')); // right-click a row INSIDE the selection
      await fireEvent.click(await screen.findByRole('menuitem', { name: 'Add to Queue' }));

      expect(queueTrackAtEnd).toHaveBeenCalledTimes(1);
      const queued = queueTrackAtEnd.mock.calls[0]![0] as { jellyfinId: string }[];
      expect(queued.map((t) => t.jellyfinId)).toEqual(['track-0', 'track-1', 'track-2', 'track-3', 'track-4']);
    });

    it('right-clicking a row OUTSIDE the selection queues only that row', async () => {
      queueTrackNext.mockClear();
      const { container } = await renderWithPlaylist('pl-menu-6', 5);
      await fireEvent.click(rowFor(container, 'item-0'));
      await fireEvent.click(rowFor(container, 'item-1'), { shiftKey: true }); // selects item-0..1

      await fireEvent.contextMenu(rowFor(container, 'item-4')); // not selected
      await fireEvent.click(await screen.findByRole('menuitem', { name: 'Play Next' }));

      expect(queueTrackNext).toHaveBeenCalledTimes(1);
      const queued = queueTrackNext.mock.calls[0]![0] as { jellyfinId: string }[];
      expect(queued.map((t) => t.jellyfinId)).toEqual(['track-4']);
    });
  });

  describe('drag carries the one shared payload, not just the internal reorder index', () => {
    it('dragging a row also sets application/x-bragi-playlist so it can drop onto another sidebar playlist', async () => {
      const { container } = await renderWithPlaylist('pl-drag-payload', 3);
      const rowsInOrder = () => Array.from(container.querySelectorAll<HTMLElement>('.track-row:not(.keep-alive)'));
      const firstRow = rowsInOrder()[0]!;

      const setData = vi.fn();
      const event = new Event('dragstart', { bubbles: true, cancelable: true });
      Object.defineProperty(event, 'dataTransfer', { value: { setData, effectAllowed: '' } });
      firstRow.dispatchEvent(event);

      const [, payload] = setData.mock.calls.find(([type]) => type === 'application/x-bragi-playlist')!;
      expect(JSON.parse(payload)).toEqual({ type: 'tracks', trackIds: ['track-0'] });
    });
  });

  describe('drag-to-reorder persistence', () => {
    it('the server order becomes authoritative once the move is confirmed', async () => {
      const playlist = makePlaylist('pl-reorder-1', 4);
      vi.mocked(moveJellyfinPlaylistItem).mockResolvedValue({
        jellyfinId: 'pl-reorder-1',
        tracks: [playlist.tracks[1]!, playlist.tracks[0]!, playlist.tracks[2]!, playlist.tracks[3]!],
      });
      const { container } = await renderWithPlaylist('pl-reorder-1', 4);

      const rowsInOrder = () => Array.from(container.querySelectorAll<HTMLElement>('.track-row:not(.keep-alive)'));
      const firstRow = rowsInOrder()[0]!;
      // jsdom has no DragEvent (and testing-library's dragover init is dropped
      // as a result — see TrackList's dragstart tests for the same limitation),
      // so `clientY` never reaches onDragOver and the pointer-half math always
      // reads "before" the hovered row. Target the THIRD row so the drop still
      // lands somewhere other than the dragged row's own current position.
      const thirdRow = rowsInOrder()[2]!;

      await fireEvent.dragStart(firstRow);
      await fireEvent.dragOver(thirdRow);
      await fireEvent.drop(thirdRow);

      // Jellyfin's Move endpoint takes the ENTRY id and the final 0-based index.
      expect(moveJellyfinPlaylistItem).toHaveBeenCalledWith('pl-reorder-1', 'item-0', 1);

      const titles = () => Array.from(container.querySelectorAll('.track-title-text')).map((el) => el.textContent);
      expect(titles()).toEqual(['Song 1', 'Song 0', 'Song 2', 'Song 3']); // the server's order won
    });

    it('a failed move reverts the optimistic reorder', async () => {
      vi.mocked(moveJellyfinPlaylistItem).mockRejectedValue(new Error('offline'));
      const { container } = await renderWithPlaylist('pl-reorder-2', 4);

      const rowsInOrder = () => Array.from(container.querySelectorAll<HTMLElement>('.track-row:not(.keep-alive)'));
      const firstRow = rowsInOrder()[0]!;
      const thirdRow = rowsInOrder()[2]!;

      await fireEvent.dragStart(firstRow);
      await fireEvent.dragOver(thirdRow);
      await fireEvent.drop(thirdRow);

      const titles = () => Array.from(container.querySelectorAll('.track-title-text')).map((el) => el.textContent);
      expect(titles()).toEqual(['Song 0', 'Song 1', 'Song 2', 'Song 3']); // reverted, not left half-moved
    });
  });

  // Playlists use the same column model the other
  // track lists have — show/hide, reorder, resize, remembered — scoped per
  // playlist (confirmed live against Apple Music: "the options you select
  // will apply to that playlist only, and have to be re-selected for each
  // playlist").
  describe('columns per playlist', () => {
    function headerCells(container: HTMLElement): HTMLElement[] {
      return Array.from(container.querySelectorAll<HTMLElement>('.col-header-cell'));
    }

    it('defaults to title, artist, album, time', async () => {
      const { container } = await renderWithPlaylist('pl-cols-default', 2);
      expect(headerCells(container).map((el) => el.dataset.col)).toEqual(['title', 'artist', 'album', 'time']);
      expect(getPlaylistColumns('pl-cols-default')).toEqual(['title', 'artist', 'album', 'time']);
    });

    it('the column menu offers only what a playlist entry can actually supply — no track/disc number, genre or year', async () => {
      const { container } = await renderWithPlaylist('pl-cols-menu', 2);
      await fireEvent.contextMenu(container.querySelector('.track-header')!);

      const labels = Array.from(document.querySelectorAll('.menu-label')).map((el) => el.textContent);
      expect(labels).toEqual(['Artist', 'Album', 'Last Played', 'Plays', 'Skips']);
    });

    it('hiding a column via the menu removes it from this playlist only, and remembers the choice', async () => {
      const { container } = await renderWithPlaylist('pl-cols-hide', 2);
      await fireEvent.contextMenu(container.querySelector('.track-header')!);
      const artistCheckbox = Array.from(document.querySelectorAll<HTMLInputElement>('.menu-item input')).find(
        (input) => input.closest('.menu-item')?.textContent?.includes('Artist')
      )!;
      await fireEvent.click(artistCheckbox);

      expect(headerCells(container).map((el) => el.dataset.col)).not.toContain('artist');
      expect(getPlaylistColumns('pl-cols-hide')).toEqual(['title', 'album', 'time']);
    });

    it('dragging a column header to a new position reorders it for this playlist', async () => {
      const { container } = await renderWithPlaylist('pl-cols-reorder', 2);
      const albumHeader = container.querySelector<HTMLElement>('.col-header-cell[data-col="album"]')!;
      const artistHeader = container.querySelector<HTMLElement>('.col-header-cell[data-col="artist"]')!;

      await fireEvent.dragStart(albumHeader);
      await fireEvent.dragOver(artistHeader);
      await fireEvent.drop(artistHeader);

      expect(headerCells(container).map((el) => el.dataset.col)).toEqual(['title', 'album', 'artist', 'time']);
      expect(getPlaylistColumns('pl-cols-reorder')).toEqual(['title', 'album', 'artist', 'time']);
    });

    it('the identity column (Title) is never draggable', async () => {
      const { container } = await renderWithPlaylist('pl-cols-pinned', 2);
      const titleHeader = container.querySelector<HTMLElement>('.col-header-cell[data-col="title"]')!;
      expect(titleHeader.draggable).toBe(false);
    });

    it("double-clicking a column's resize edge sets an explicit width for this playlist only", async () => {
      const { container } = await renderWithPlaylist('pl-cols-fit', 2);
      const albumHeader = container.querySelector<HTMLElement>('.col-header-cell[data-col="album"]')!;
      // jsdom's getBoundingClientRect is all zeros, so a clientX within 6px of
      // 0 reads as grabbing the resize edge (same trick TrackList's own
      // dblclick tests use); jsdom also has no canvas backend, so the fit
      // measurement falls back to the column's own minWidth (80 for album).
      await fireEvent.dblClick(albumHeader, { clientX: -3 });

      expect(getPlaylistColumnWidths('pl-cols-fit')).toEqual({ album: 80 });
    });

    it("customising one playlist's columns never touches another playlist's", async () => {
      const { container } = await renderWithPlaylist('pl-cols-a', 2);
      await fireEvent.contextMenu(container.querySelector('.track-header')!);
      const artistCheckbox = Array.from(document.querySelectorAll<HTMLInputElement>('.menu-item input')).find(
        (input) => input.closest('.menu-item')?.textContent?.includes('Artist')
      )!;
      await fireEvent.click(artistCheckbox);

      expect(getPlaylistColumns('pl-cols-a')).toEqual(['title', 'album', 'time']);
      expect(getPlaylistColumns('pl-cols-b')).toEqual(['title', 'artist', 'album', 'time']); // untouched default
    });

    // Fresh-eyes review of the first playlist-columns cut: `gridTemplate` only ever
    // read the COMMITTED width, so a resize in progress never moved the
    // column until mouseup, then jumped straight to the final size.
    it('a resize in progress moves the grid track immediately, not only once it commits on mouseup', async () => {
      const { container } = await renderWithPlaylist('pl-cols-live-resize', 2);
      const albumHeader = container.querySelector<HTMLElement>('.col-header-cell[data-col="album"]')!;
      albumHeader.getBoundingClientRect = () =>
        ({ left: 300, right: 460, width: 160, top: 0, bottom: 0, height: 0, x: 300, y: 0, toJSON: () => ({}) }) as DOMRect;

      const trackHeader = container.querySelector<HTMLElement>('.track-header')!;
      const beforeTemplate = trackHeader.style.gridTemplateColumns;

      albumHeader.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, clientX: 459 })); // within 6px of the 460 edge
      document.dispatchEvent(new MouseEvent('mousemove', { clientX: 559 })); // +100px
      await tick();

      const duringTemplate = trackHeader.style.gridTemplateColumns;
      expect(duringTemplate).not.toBe(beforeTemplate);
      expect(duringTemplate).toContain('260px'); // album's live width: startWidth(160) + 100

      document.dispatchEvent(new MouseEvent('mouseup'));
    });

    // Fresh-eyes review: a deleted playlist must not leave its column
    // customisation behind forever.
    it('a successful delete drops the column customisation this playlist had', async () => {
      vi.mocked(deleteJellyfinPlaylist).mockResolvedValue(undefined);
      const { container } = await renderWithNamedPlaylist('pl-cols-delete', 'Doomed', 2);

      await fireEvent.contextMenu(container.querySelector('.track-header')!);
      const artistCheckbox = Array.from(document.querySelectorAll<HTMLInputElement>('.menu-item input')).find(
        (input) => input.closest('.menu-item')?.textContent?.includes('Artist')
      )!;
      await fireEvent.click(artistCheckbox);
      expect(getPlaylistColumns('pl-cols-delete')).toEqual(['title', 'album', 'time']);

      const trigger = container.querySelector<HTMLElement>('.playlist-actions > .action-btn.danger')!;
      await fireEvent.click(trigger);
      await screen.findByText('Delete playlist?');
      await fireEvent.click(document.querySelector<HTMLElement>('.confirm-delete-btn')!);
      await tick();

      expect(deleteJellyfinPlaylist).toHaveBeenCalledWith('pl-cols-delete');
      expect(getPlaylistColumns('pl-cols-delete')).toEqual(['title', 'artist', 'album', 'time']); // back to default
    });
  });
});
