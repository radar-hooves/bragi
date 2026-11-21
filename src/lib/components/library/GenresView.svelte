<script lang="ts">
  import { untrack } from 'svelte';
  import Play from '@lucide/svelte/icons/play';
  import Shuffle from '@lucide/svelte/icons/shuffle';
  import { getGenreStats, type GenreStat } from '#lib/services/tauri.js';
  import { getLibraryState, getLibraryVersion } from '#lib/stores/library.svelte.js';
  import { syncColumnBrowserToSelection } from '#lib/stores/columnBrowser.svelte.js';
  import { setCurrentSection, toggleColumnBrowser, getUiState, getGridTileSize } from '#lib/stores/ui.svelte.js';
  import type { GenresSort, ViewMode } from '#lib/stores/ui.svelte.js';
  import { navigateTo } from '#lib/stores/navigation.svelte.js';
  import { cachedArtworkUrl, handleArtworkError } from '#lib/utils/artworkFallback.js';
  import { compareFolded } from '#lib/utils/ordering.js';
  import { StatusBar, type StatusBarData } from '#lib/components/common/index.js';
  import { bindScrollMemory } from '#lib/utils/scrollMemory.svelte.js';
  import { createGridSelection } from '#lib/utils/gridSelection.svelte.js';
  import { DEFAULT_GRID_TILE_PX, gridColumnsForWidth } from '#lib/utils/gridTiles.js';

  interface Props {
    viewMode: ViewMode;
    sort: GenresSort;
    onShuffle: (genre: string) => void;
  }

  let { viewMode, sort, onShuffle }: Props = $props();

  /** Covers a full mosaic needs; fewer means the hash-colour tile instead. */
  const MOSAIC_TILES = 4;

  // Counts come from a SQL GROUP BY over the genre join table, not
  // a client-side walk of every cached track — which both mis-counted
  // multi-value tags and needed an untrack() workaround for Svelte 5's O(n^2)
  // dependency-tracking cliff.
  let genres = $state<GenreStat[]>([]);
  let loadedVersion = -1;

  // Genres whose mosaic covers turned out not to be in the local artwork cache.
  // The backend picks the four albums by track count and cannot know what is
  // cached, so a fully offline genre would otherwise render four blank panels
  // instead of its designed tile.
  let mosaicFailures = $state<Record<string, number>>({});

  function noteMosaicFailure(genreName: string): void {
    mosaicFailures = { ...mosaicFailures, [genreName]: (mosaicFailures[genreName] ?? 0) + 1 };
  }

  function hasMosaic(genre: GenreStat): boolean {
    return (
      genre.albumIds.length >= MOSAIC_TILES &&
      (mosaicFailures[genre.name] ?? 0) < MOSAIC_TILES
    );
  }

  async function load(): Promise<void> {
    try {
      genres = await getGenreStats();
      mosaicFailures = {};
    } catch (e) {
      console.error('Failed to load genre stats:', e);
    }
  }

  // No onMount(load) beside this: loadedVersion starts at -1, so the effect
  // fires once on mount and again only when a sync changes what the library
  // holds. Having both ran get_genre_stats twice on every entry to the view.
  $effect(() => {
    const version = getLibraryVersion();
    if (version !== loadedVersion) {
      loadedVersion = version;
      load();
    }
  });

  // List view sorts by its own column header; the grid uses the toolbar's sort.
  type ListColumn = 'name' | 'trackCount' | 'albumCount' | 'artistCount' | 'durationMs' | 'years';
  let listColumn = $state<ListColumn>('name');
  let listAscending = $state(true);

  function toggleListSort(column: ListColumn): void {
    if (listColumn === column) {
      listAscending = !listAscending;
    } else {
      listColumn = column;
      listAscending = column === 'name';
    }
  }

  function compareByColumn(a: GenreStat, b: GenreStat, column: ListColumn): number {
    switch (column) {
      case 'name':
        return compareFolded(a.name, b.name);
      case 'trackCount':
        return a.trackCount - b.trackCount;
      case 'albumCount':
        return a.albumCount - b.albumCount;
      case 'artistCount':
        return a.artistCount - b.artistCount;
      case 'durationMs':
        return a.durationMs - b.durationMs;
      case 'years':
        return (a.yearMin ?? 0) - (b.yearMin ?? 0);
    }
  }

  const displayedGenres = $derived.by((): GenreStat[] => {
    const list = [...genres];
    if (viewMode === 'list') {
      const column = listColumn;
      const direction = listAscending ? 1 : -1;
      // Name is the tie-break everywhere, so equal counts keep a stable order.
      return list.sort((a, b) => compareByColumn(a, b, column) * direction || compareFolded(a.name, b.name));
    }
    switch (sort) {
      case 'name-desc':
        return list.sort((a, b) => compareFolded(b.name, a.name));
      case 'tracks-desc':
        return list.sort((a, b) => b.trackCount - a.trackCount || compareFolded(a.name, b.name));
      case 'time-desc':
        return list.sort((a, b) => b.durationMs - a.durationMs || compareFolded(a.name, b.name));
      default:
        return list.sort((a, b) => compareFolded(a.name, b.name));
    }
  });

  function handleGenreClick(genre: string): void {
    syncColumnBrowserToSelection({ genre });
    setCurrentSection('tracks');
    if (!getUiState().columnBrowserVisible) {
      toggleColumnBrowser();
    }
    navigateTo({
      section: 'tracks',
      selectedArtistId: null,
      selectedAlbumId: null,
      columnBrowserVisible: true,
    });
  }

  function handleShuffleClick(event: MouseEvent, genre: string): void {
    event.stopPropagation();
    onShuffle(genre);
  }

  /**
   * Hue for the fallback tile, from the genre name.
   *
   * A plain FNV-style walk, so a given genre is always the same colour — the
   * tile must not change between launches.
   */
  function hashHue(name: string): number {
    let hash = 0;
    for (let i = 0; i < name.length; i++) {
      hash = (hash * 31 + name.charCodeAt(i)) >>> 0;
    }
    return hash % 360;
  }

  function formatDuration(ms: number): string {
    const minutes = Math.floor(ms / 60000);
    if (minutes < 60) return `${minutes} min`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return `${hours} hr ${minutes % 60} min`;
    const days = Math.floor(hours / 24);
    return `${days} ${days === 1 ? 'day' : 'days'} ${hours % 24} hr`;
  }

  function formatYears(genre: GenreStat): string {
    if (genre.yearMin === null || genre.yearMax === null) return '—';
    return genre.yearMin === genre.yearMax
      ? `${genre.yearMin}`
      : `${genre.yearMin}–${genre.yearMax}`;
  }

  const statusData = $derived.by((): StatusBarData | null => {
    if (genres.length === 0) return null;
    const count = genres.length;
    // Totals are summed over DISTINCT tracks, never over the per-genre rows: a
    // track tagged with three genres would otherwise be counted three times.
    // Summed under untrack() — reading 23k deeply-reactive tracks inside
    // a $derived registers a dependency per property and hits Svelte 5's O(n^2)
    // dependency-tracking cliff.
    const tracks = getLibraryState().tracks;
    return untrack(() => ({
      itemCount: count,
      itemLabel: 'genre',
      durationMs: tracks.reduce((sum, t) => sum + t.durationMs, 0),
      sizeBytes: tracks.reduce((sum, t) => sum + (t.fileSizeBytes ?? 0), 0),
    }));
  });

  // Distinct genre tags in a real library run to dozens or low hundreds, never
  // the thousands the Songs table, playlists or the column browser can reach
  // — nowhere near where windowing would earn its keep — so this remembers
  // scroll position without virtualising rows that never need it. The
  // grid and list views are mutually exclusive, so one viewport element covers
  // both.
  let viewportEl = $state<HTMLDivElement>();
  $effect(() => bindScrollMemory(`genres-${viewMode}`, viewportEl));

  // Selection + keyboard: see gridSelection.svelte.ts.
  // The list view keeps its own row click/Enter handling; this is the grid's.
  function columnsForWidth(width: number): number {
    const size = getGridTileSize('genres');
    return gridColumnsForWidth(width, size > 0 ? size : DEFAULT_GRID_TILE_PX);
  }

  const grid = createGridSelection<GenreStat>({
    getItems: () => displayedGenres,
    getId: (g) => g.name,
    getLabel: (g) => g.name,
    getViewport: () => viewportEl,
    idAttribute: 'data-genre-id',
    columns: () => (viewportEl ? columnsForWidth(viewportEl.clientWidth) : 1),
    onActivate: (g) => handleGenreClick(g.name),
  });

  /** HTML ids can't contain whitespace, and a user-tagged genre name can. */
  function tileId(name: string): string {
    return `genre-tile-${encodeURIComponent(name)}`;
  }
</script>

{#snippet sortableHeader(column: ListColumn, label: string, alignRight: boolean)}
  <th class="col-header" class:align-right={alignRight} class:sorted={listColumn === column}>
    <button class="col-header-btn" onclick={() => toggleListSort(column)}>
      <span>{label.toUpperCase()}</span>
      {#if listColumn === column}
        <span class="sort-indicator">{listAscending ? '▲' : '▼'}</span>
      {/if}
    </button>
  </th>
{/snippet}

<div class="genres-view" style:--grid-tile-size="{getGridTileSize('genres')}px">
  {#if genres.length === 0}
    <div class="empty-state">
      <p class="empty-title">No genres found</p>
      <p class="empty-description">Genre information will appear once your library is synced.</p>
    </div>
  {:else if viewMode === 'list'}
    <div class="genres-table-scroll" bind:this={viewportEl}>
      <table class="genres-table">
        <thead>
          <tr>
            {@render sortableHeader('name', 'Genre', false)}
            {@render sortableHeader('trackCount', 'Songs', true)}
            {@render sortableHeader('albumCount', 'Albums', true)}
            {@render sortableHeader('artistCount', 'Artists', true)}
            {@render sortableHeader('durationMs', 'Total time', true)}
            <th class="col-header align-right col-years" class:sorted={listColumn === 'years'}>
              <button class="col-header-btn" onclick={() => toggleListSort('years')}>
                <span>YEARS</span>
                {#if listColumn === 'years'}
                  <span class="sort-indicator">{listAscending ? '▲' : '▼'}</span>
                {/if}
              </button>
            </th>
          </tr>
        </thead>
        <tbody>
          {#each displayedGenres as genre (genre.name)}
            <tr
              class="genre-row"
              tabindex="0"
              onclick={() => handleGenreClick(genre.name)}
              onkeydown={(e) => {
                if (e.key === 'Enter') handleGenreClick(genre.name);
              }}
            >
              <td class="genre-cell" title={genre.name}>
                <span class="genre-cell-name">{genre.name}</span>
                <button
                  class="row-shuffle"
                  onclick={(e) => handleShuffleClick(e, genre.name)}
                  aria-label="Shuffle {genre.name}"
                  title="Shuffle {genre.name}"
                >
                  <Shuffle size={13} />
                </button>
              </td>
              <td class="genre-cell align-right">{genre.trackCount}</td>
              <td class="genre-cell align-right">{genre.albumCount}</td>
              <td class="genre-cell align-right">{genre.artistCount}</td>
              <td class="genre-cell align-right">{formatDuration(genre.durationMs)}</td>
              <td class="genre-cell align-right col-years">{formatYears(genre)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      class="genres-grid"
      bind:this={viewportEl}
      tabindex="0"
      onkeydown={grid.handleKeyDown}
      aria-activedescendant={grid.selection.activeId ? tileId(grid.selection.activeId) : undefined}
    >
      {#each displayedGenres as genre (genre.name)}
        <div
          class="genre-tile"
          id={tileId(genre.name)}
          data-genre-id={genre.name}
          class:selected={grid.selection.isSelected(genre.name)}
          class:active={grid.selection.activeId === genre.name}
        >
          <button
            class="tile-open"
            title={genre.name}
            tabindex="-1"
            onclick={(e) => grid.handleTileClick(genre, e)}
            aria-label="Show {genre.name}"
          ></button>
          <div class="tile-art">
            {#if hasMosaic(genre)}
              <div class="mosaic">
                {#each genre.albumIds.slice(0, MOSAIC_TILES) as albumId (albumId)}
                  <div class="mosaic-cell">
                    <img
                      src={cachedArtworkUrl(albumId)}
                      alt=""
                      loading="lazy"
                      onerror={(e) => {
                        handleArtworkError(e);
                        noteMosaicFailure(genre.name);
                      }}
                    />
                    <span class="mosaic-blank" style="display: none"></span>
                  </div>
                {/each}
              </div>
            {:else}
              <div
                class="flat-tile"
                style="--tile-hue: {hashHue(genre.name)}"
                aria-hidden="true"
              >
                <span class="flat-tile-glyph">{genre.name.slice(0, 2).toUpperCase()}</span>
              </div>
            {/if}
            <div class="tile-scrim"></div>
            <div class="tile-caption">
              <span class="tile-name">{genre.name}</span>
              <span class="tile-meta">
                {genre.trackCount}
                {genre.trackCount === 1 ? 'song' : 'songs'}
                &middot;
                {genre.artistCount}
                {genre.artistCount === 1 ? 'artist' : 'artists'}
              </span>
            </div>
          </div>
          <button
            class="tile-shuffle"
            aria-label="Shuffle {genre.name}"
            title="Shuffle {genre.name}"
            onclick={(e) => handleShuffleClick(e, genre.name)}
          >
            <Shuffle size={18} />
          </button>
        </div>
      {/each}
    </div>
  {/if}

  <StatusBar data={statusData} />
</div>

<style>
  .genres-view {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  /* ---------------------------------------------------------------- grid */

  .genres-grid {
    flex: 1;
    overflow-y: auto;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(var(--grid-tile-size, 200px), 100%), 1fr));
    /* Square tiles, so a row never takes its height from its longest name. */
    grid-auto-rows: min-content;
    gap: var(--space-md);
    padding: 0 var(--space-lg) var(--space-md);
    align-content: start;
  }

  /* Roving keyboard focus — see gridSelection.svelte.ts. */
  .genres-grid:focus {
    outline: none;
  }

  .genre-tile {
    position: relative;
    display: block;
    min-width: 0;
    border-radius: var(--p11-radius-md);
  }

  /* Multi-selected. */
  .genre-tile.selected {
    outline: 2px solid var(--p11-accent);
    outline-offset: 4px;
  }

  /* The keyboard cursor. */
  .genre-tile.active .tile-art {
    outline: 2px solid var(--p11-border-focus);
    outline-offset: -2px;
  }

  /* The whole tile is one click target, laid over the art rather than wrapping
     it: a <button> may not contain the shuffle <button>. */
  .tile-open {
    position: absolute;
    inset: 0;
    z-index: 1;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--p11-radius-md);
    cursor: pointer;
  }

  .tile-art {
    position: relative;
    aspect-ratio: 1;
    width: 100%;
    overflow: hidden;
    border-radius: var(--p11-radius-md);
    background-color: var(--p11-bg-surface);
    box-shadow: var(--p11-shadow-card);
    transition:
      transform var(--transition-fast),
      box-shadow var(--transition-fast);
  }

  .genre-tile:hover .tile-art,
  .tile-open:focus-visible ~ .tile-art {
    transform: translateY(-2px);
    box-shadow: var(--p11-shadow-card-hover);
  }

  .tile-open:active ~ .tile-art {
    transform: translateY(0);
  }

  .mosaic {
    display: grid;
    grid-template-columns: 1fr 1fr;
    grid-template-rows: 1fr 1fr;
    width: 100%;
    height: 100%;
  }

  .mosaic-cell {
    position: relative;
    overflow: hidden;
    background-color: var(--p11-bg-elevated);
  }

  .mosaic-cell img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .mosaic-blank {
    position: absolute;
    inset: 0;
    background-color: var(--p11-bg-elevated);
  }

  .flat-tile {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 100%;
    background: linear-gradient(145deg, var(--p11-tile-ramp-top), var(--p11-tile-ramp-bottom));
  }

  .flat-tile-glyph {
    font-size: var(--font-size-2xl);
    font-weight: var(--font-weight-bold);
    color: var(--p11-text-on-media-muted);
    letter-spacing: 0.04em;
  }

  .tile-scrim {
    position: absolute;
    inset: 0;
    background: linear-gradient(
      to top,
      rgb(var(--p11-shade-rgb) / 0.82) 0%,
      rgb(var(--p11-shade-rgb) / 0.45) 38%,
      rgb(var(--p11-shade-rgb) / 0) 68%
    );
    pointer-events: none;
  }

  .tile-caption {
    position: absolute;
    right: 0;
    bottom: 0;
    left: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    padding: var(--space-sm) var(--space-md);
    pointer-events: none;
  }

  .tile-name {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    /* User-tagged genres can be arbitrarily long and need not contain spaces. */
    overflow-wrap: anywhere;
    min-width: 0;
    font-size: var(--font-size-md);
    font-weight: var(--font-weight-semibold);
    line-height: var(--line-height-tight);
    color: var(--p11-text-on-media);
  }

  .tile-meta {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: var(--font-size-xs);
    color: var(--p11-text-on-media-muted);
  }

  .tile-shuffle {
    position: absolute;
    top: var(--space-sm);
    right: var(--space-sm);
    z-index: 2;
    padding: 0;
    border: none;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    border-radius: var(--radius-full);
    background-color: var(--p11-accent);
    color: var(--p11-text-on-media);
    opacity: 0;
    cursor: pointer;
    transition:
      opacity var(--transition-fast),
      background-color var(--transition-fast);
  }

  .genre-tile:hover .tile-shuffle,
  .tile-shuffle:focus-visible {
    opacity: 1;
  }

  .tile-shuffle:hover {
    background-color: var(--p11-accent-hover);
  }

  /* ---------------------------------------------------------------- list */

  .genres-table-scroll {
    flex: 1;
    /* Six columns do not fit a narrow window. Scroll the table inside its own
       container rather than clipping the last column out of reach — the page
       itself must never scroll horizontally. */
    overflow: auto;
    padding: 0 var(--space-lg) var(--space-md);
  }

  .genres-table {
    width: 100%;
    min-width: 520px;
    border-collapse: collapse;
  }

  /* Years is the first thing to go in a narrow window: six columns cannot fit,
     and a half-visible column reads as broken where a dropped one does not. */
  @media (width <= 1100px) {
    .col-years {
      display: none;
    }
  }

  /* The name column takes the slack so the figures stay hard against the right. */
  .genres-table th:first-child,
  .genres-table td:first-child {
    width: 100%;
  }

  .genres-table thead th {
    position: sticky;
    top: 0;
    z-index: 1;
    background-color: var(--p11-bg-primary);
  }

  .col-header {
    padding: 0;
    border-bottom: 1px solid var(--p11-border);
    text-align: left;
  }

  .col-header.sorted .col-header-btn {
    color: var(--p11-text-primary);
  }

  .col-header-btn {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    padding: var(--space-xs) var(--space-md);
    background: none;
    border: none;
    color: var(--p11-text-muted);
    font-family: var(--font-family);
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
    letter-spacing: 0.04em;
    cursor: pointer;
  }

  .col-header.align-right .col-header-btn {
    justify-content: flex-end;
  }

  .sort-indicator {
    font-size: var(--font-size-2xs);
  }

  .genre-row {
    cursor: pointer;
  }

  .genre-row:nth-child(even) {
    background-color: rgb(var(--p11-overlay-rgb) / 0.02);
  }

  .genre-row:hover {
    background-color: var(--p11-bg-hover);
  }

  .genre-cell {
    padding: 6px var(--space-md);
    font-size: var(--font-size-md);
    color: var(--p11-text-secondary);
    white-space: nowrap;
  }

  .genre-cell.align-right {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  td.genre-cell:first-child {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    color: var(--p11-text-primary);
  }

  .genre-cell-name {
    overflow: hidden;
    min-width: 0;
    text-overflow: ellipsis;
  }

  .row-shuffle {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 22px;
    height: 22px;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius-full);
    color: var(--p11-text-muted);
    opacity: 0;
    cursor: pointer;
  }

  .genre-row:hover .row-shuffle,
  .row-shuffle:focus-visible {
    opacity: 1;
    color: var(--p11-accent);
  }

  /* --------------------------------------------------------------- empty */

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: var(--space-3xl) var(--space-xl);
    text-align: center;
  }

  .empty-title {
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-medium);
    color: var(--p11-text-primary);
    margin: 0 0 var(--space-sm);
  }

  .empty-description {
    font-size: var(--font-size-md);
    color: var(--p11-text-secondary);
    margin: 0;
  }
</style>
