import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, cleanup, fireEvent, screen } from '@testing-library/svelte';
import SharedPlaylistView from './SharedPlaylistView.svelte';
import { clearScrollMemory } from '#lib/utils/scrollMemory.svelte.js';
import type { ImportedPlaylistDetail, ImportedPlaylistTrack } from '#lib/types/models.js';

vi.mock('#lib/services/tauri.js', () => ({
  getImportedPlaylists: vi.fn().mockResolvedValue([]),
  getImportedPlaylist: vi.fn(),
  deleteImportedPlaylist: vi.fn(),
  importPlaylist: vi.fn(),
  importPlaylistFromJellyfinLink: vi.fn(),
}));

const { getImportedPlaylist, deleteImportedPlaylist } = await import('#lib/services/tauri.js');
const { selectImportedPlaylist } = await import('#lib/stores/importedPlaylists.svelte.js');

const VIEWPORT_WIDTH = 800;
const VIEWPORT_HEIGHT = 400;
let widthDescriptor: PropertyDescriptor | undefined;
let heightDescriptor: PropertyDescriptor | undefined;

function makeTrack(i: number): ImportedPlaylistTrack {
  return {
    title: `Song ${i}`,
    artistName: 'Artist',
    albumName: 'Album',
    durationMs: 200_000,
    resolvedJellyfinId: `track-${i}`,
  };
}

function makePlaylist(id: string, trackCount: number): ImportedPlaylistDetail {
  return {
    id,
    name: `Playlist ${id}`,
    importedAt: Date.now(),
    tracks: Array.from({ length: trackCount }, (_, i) => makeTrack(i)),
  };
}

async function renderWithPlaylist(id: string, trackCount: number) {
  vi.mocked(getImportedPlaylist).mockResolvedValue(makePlaylist(id, trackCount));
  await selectImportedPlaylist(id);
  return render(SharedPlaylistView, { props: {} });
}

describe('SharedPlaylistView', () => {
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

  it('windows a large imported playlist: fewer rows mount than tracks exist', async () => {
    const { container } = await renderWithPlaylist('shared-1', 400);
    const rows = container.querySelectorAll('.track-row');
    expect(rows.length).toBeLessThan(400);
    expect(rows.length).toBeGreaterThan(0);
  });

  it('remembers scroll position per playlist id and resets for a different playlist', async () => {
    const first = await renderWithPlaylist('shared-a', 400);
    const viewport1 = first.container.querySelector<HTMLElement>('.track-list')!;
    viewport1.scrollTop = 120 * 40;
    first.unmount();

    const second = await renderWithPlaylist('shared-a', 400);
    // The restore is queued a microtask out, so it can never re-enter
    // Svelte's own scheduler mid-effect — give it one turn.
    await Promise.resolve();
    const viewport2 = second.container.querySelector<HTMLElement>('.track-list')!;
    expect(viewport2.scrollTop).toBe(120 * 40);
    second.unmount();

    const third = await renderWithPlaylist('shared-b', 400);
    await Promise.resolve();
    const viewport3 = third.container.querySelector<HTMLElement>('.track-list')!;
    expect(viewport3.scrollTop).toBe(0);
  });

  describe('delete confirmation is a proper dialog, not inline Yes/No', () => {
    it('does nothing until confirmed, with no inline Yes/No pair', async () => {
      vi.mocked(deleteImportedPlaylist).mockResolvedValue(undefined);
      const { container } = await renderWithPlaylist('shared-del-1', 2);

      const trigger = container.querySelector<HTMLElement>('.playlist-actions > .action-btn.danger')!;
      expect(trigger.textContent?.trim()).toBe('Remove');
      await fireEvent.click(trigger);

      expect(await screen.findByText('Remove this import?')).toBeTruthy();
      expect(screen.queryByText('Yes')).toBeFalsy();
      expect(screen.queryByText('No')).toBeFalsy();
      expect(deleteImportedPlaylist).not.toHaveBeenCalled();

      await fireEvent.click(document.querySelector<HTMLElement>('.confirm-delete-btn')!);
      expect(deleteImportedPlaylist).toHaveBeenCalledWith('shared-del-1');
    });

    it('Cancel removes nothing', async () => {
      vi.mocked(deleteImportedPlaylist).mockClear();
      const { container } = await renderWithPlaylist('shared-del-2', 2);
      const trigger = container.querySelector<HTMLElement>('.playlist-actions > .action-btn.danger')!;
      await fireEvent.click(trigger);

      await screen.findByText('Remove this import?');
      await fireEvent.click(document.querySelector<HTMLElement>('.cancel-btn')!);

      expect(deleteImportedPlaylist).not.toHaveBeenCalled();
    });
  });
});
