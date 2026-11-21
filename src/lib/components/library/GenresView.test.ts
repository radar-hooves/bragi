// Selection + keyboard wiring. The click/⌘/Shift/
// arrow/Return/type-to-find CONTRACT itself is tested once, generically, in
// gridSelection.svelte.test.ts — this file only pins that GenresView's grid
// wires its own DOM (data-genre-id, the viewport's live column-stride
// formula, its "open" handler) to that shared controller correctly.
import { describe, it, expect, vi, beforeAll, afterAll, afterEach } from 'vitest';
import { tick } from 'svelte';
import { render, cleanup, waitFor } from '@testing-library/svelte';
import GenresView from './GenresView.svelte';
import { clearScrollMemory } from '#lib/utils/scrollMemory.svelte.js';
import type { GenreStat } from '#lib/services/tauri.js';

const { genres } = vi.hoisted(() => {
  function makeGenre(i: number): GenreStat {
    return {
      name: `Genre ${i}`,
      trackCount: 10,
      artistCount: 2,
      albumCount: 1,
      durationMs: 1_000_000,
      yearMin: 2000,
      yearMax: 2000,
      albumIds: [],
    };
  }
  return { genres: Array.from({ length: 5 }, (_, i) => makeGenre(i)) as GenreStat[] };
});

vi.mock('#lib/services/tauri.js', async (importOriginal) => ({
  ...(await importOriginal<typeof import('#lib/services/tauri.js')>()),
  getGenreStats: vi.fn().mockResolvedValue(genres),
  // clicking a tile flows into ui.svelte's debounced settings autosave — never
  // let the real invoke() through, it has nothing to talk to under jsdom.
  updateSettings: vi.fn().mockResolvedValue(undefined),
}));

const syncColumnBrowserToSelection = vi.fn();
vi.mock('#lib/stores/columnBrowser.svelte.js', async (importOriginal) => ({
  ...(await importOriginal<typeof import('#lib/stores/columnBrowser.svelte.js')>()),
  syncColumnBrowserToSelection: (...args: unknown[]) => syncColumnBrowserToSelection(...args),
}));

// tile 200 + gap 12 (GenresView's own GAP_PX): (412 + 12) / 212 = 2 columns
// exactly, so arrow-key stride is predictable without touching tile-size state.
const VIEWPORT_WIDTH = 412;
const VIEWPORT_HEIGHT = 400;

let widthDescriptor: PropertyDescriptor | undefined;
let heightDescriptor: PropertyDescriptor | undefined;

beforeAll(() => {
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

afterAll(() => {
  if (widthDescriptor) Object.defineProperty(HTMLElement.prototype, 'clientWidth', widthDescriptor);
  if (heightDescriptor) Object.defineProperty(HTMLElement.prototype, 'clientHeight', heightDescriptor);
});

describe('GenresView grid selection and keyboard wiring', () => {
  afterEach(() => {
    cleanup();
    clearScrollMemory();
    syncColumnBrowserToSelection.mockClear();
  });

  function tileFor(container: HTMLElement, name: string): HTMLElement {
    return container.querySelector<HTMLElement>(`[data-genre-id="${name}"]`)!;
  }

  /** getGenreStats() resolves async, past the mount effect — wait for the first tile. */
  async function renderGrid(): Promise<{ container: HTMLElement }> {
    const { container } = render(GenresView, { props: { viewMode: 'grid', sort: 'name-asc', onShuffle: vi.fn() } });
    await waitFor(() => expect(container.querySelector('.genre-tile')).toBeTruthy());
    return { container };
  }

  async function click(el: HTMLElement, opts: Partial<MouseEventInit> = {}): Promise<void> {
    el.querySelector<HTMLElement>('.tile-open')!.dispatchEvent(new MouseEvent('click', { bubbles: true, ...opts }));
    await tick();
  }

  async function press(el: HTMLElement, key: string, opts: Partial<KeyboardEventInit> = {}): Promise<void> {
    el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...opts }));
    await tick();
  }

  it('a plain click opens the genre — Apple Music/Spotify, not Finder-style select-then-open', async () => {
    const { container } = await renderGrid();
    await click(tileFor(container, 'Genre 1'));
    expect(syncColumnBrowserToSelection).toHaveBeenCalledWith({ genre: 'Genre 1' });
    expect(tileFor(container, 'Genre 1').classList.contains('selected')).toBe(false);
  });

  it('⌘-click selects the tile WITHOUT opening it', async () => {
    const { container } = await renderGrid();
    await click(tileFor(container, 'Genre 1'), { metaKey: true });
    expect(tileFor(container, 'Genre 1').classList.contains('selected')).toBe(true);
    expect(syncColumnBrowserToSelection).not.toHaveBeenCalled();
  });

  it("ArrowDown moves the active tile by this grid's own live column stride (2 columns at this width)", async () => {
    const { container } = await renderGrid();
    const grid = container.querySelector<HTMLElement>('.genres-grid')!;
    await press(grid, 'ArrowDown'); // nothing active yet -> lands on index 0
    expect(tileFor(container, 'Genre 0').classList.contains('active')).toBe(true);
    await press(grid, 'ArrowDown'); // +2 (one row down, 2 columns)
    expect(tileFor(container, 'Genre 2').classList.contains('active')).toBe(true);
  });
});
