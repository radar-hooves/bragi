<script lang="ts">
  import type { Album } from '#lib/types/models.js';
  import { getGridTileSize, type GridViewId } from '#lib/stores/ui.svelte.js';
  import AlbumCard from './AlbumCard.svelte';
  import { createVirtualList } from '#lib/utils/virtualList.svelte.js';
  import { createGridSelection } from '#lib/utils/gridSelection.svelte.js';
  import { GRID_GAP_PX, DEFAULT_GRID_TILE_PX, gridColumnsForWidth } from '#lib/utils/gridTiles.js';

  interface Props {
    albums: Album[];
    /** Keys this grid's remembered scroll position (albums, recently-added, ...). */
    viewKey?: string;
    /** Which view's tile size this grid reads and displays (each grid view remembers its own size, the way Apple Music's View
     * Options does per view rather than sharing one global setting). */
    sizeView?: GridViewId;
    /** Opens the album (see gridSelection.svelte.ts). */
    onAlbumClick?: (album: Album) => void;
    /** Play, Play Next, Add to Queue and Add to Playlist all act on the whole
     * grid selection when the acted-on tile is selected — never just the one
     * tile under the pointer (the same rule TrackList's
     * own tracksForAction uses for a track row). */
    onAlbumsPlay?: (albums: Album[]) => void;
    onAlbumQueue?: (albums: Album[], position: 'next' | 'end') => void;
    onAlbumAddToPlaylist?: (albums: Album[]) => void;
  }

  let {
    albums,
    viewKey = 'albums',
    sizeView = 'albums',
    onAlbumClick,
    onAlbumsPlay,
    onAlbumQueue,
    onAlbumAddToPlaylist,
  }: Props = $props();

  // --- Virtual grid ---------------------------------------------------------
  // Only the cards near the viewport are mounted, through the shared
  // virtualiser every long list routes through.
  //
  // ROW_HEIGHT assumption: album artwork is a square that fills the column
  // width, so a row's height is (column width + label/padding/gap). We seed
  // that estimate (LABEL_OVERHEAD ~= tile+64, matching the card's
  // contain-intrinsic-size of ~240 at the default 200px tile) and the
  // virtualiser corrects it by measuring the real rendered stride, so the
  // spacer height and scroll mapping stay pixel-accurate across tile sizes
  // and UI zoom.
  const LABEL_OVERHEAD = 64; // column-width -> row-height estimate before measuring

  let viewportEl = $state<HTMLDivElement>();
  let innerGridEl = $state<HTMLDivElement>();

  function currentTile(): number {
    const size = getGridTileSize(sizeView);
    return size > 0 ? size : DEFAULT_GRID_TILE_PX;
  }

  function columnsForWidth(width: number): number {
    return gridColumnsForWidth(width, currentTile());
  }

  const virtual = createVirtualList({
    count: () => albums.length,
    columns: columnsForWidth,
    estimateRowHeight: (width, cols) => {
      const colWidth = (width - (cols - 1) * GRID_GAP_PX) / cols;
      return Math.max(1, colWidth + LABEL_OVERHEAD);
    },
    overscan: 3,
    bypassBelow: 250,
  });

  let visibleAlbums = $derived(albums.slice(virtual.start, virtual.end));

  // Selection + keyboard: see gridSelection.svelte.ts
  const grid = createGridSelection<Album>({
    getItems: () => albums,
    getId: (a) => a.jellyfinId,
    getLabel: (a) => a.name,
    getViewport: () => viewportEl,
    idAttribute: 'data-album-id',
    columns: () => (viewportEl ? columnsForWidth(viewportEl.clientWidth) : 1),
    onActivate: (a) => onAlbumClick?.(a),
    virtual,
  });

  const albumById = $derived.by(() => new Map(albums.map((a) => [a.jellyfinId, a])));

  // A menu (or the overlay Play button) on a tile that's part of a
  // multi-selection acts on the WHOLE selection, matching TrackList's own
  // tracksForAction — a tile outside the current
  // selection still acts on just itself (`selection.targetIds`).
  function albumsForAction(album: Album): Album[] {
    return grid.selection.targetIds(album.jellyfinId).map((id) => albumById.get(id)).filter((a): a is Album => !!a);
  }

  function handlePlay(album: Album): void {
    onAlbumsPlay?.(albumsForAction(album));
  }

  function handleQueue(album: Album, position: 'next' | 'end'): void {
    onAlbumQueue?.(albumsForAction(album), position);
  }

  function handleAddToPlaylist(album: Album): void {
    onAlbumAddToPlaylist?.(albumsForAction(album));
  }

  // Own the viewport's resize handling and remember its scroll position, keyed
  // by the calling view (Albums, Recently Added, ...).
  $effect(() => virtual.attachViewport(viewportEl));
  $effect(() => virtual.bindScrollMemory(viewKey, viewportEl));

  // Recompute the range when the data or tile size change.
  $effect(() => {
    void albums.length;
    void getGridTileSize(sizeView);
    virtual.refresh();
  });

  // Re-measure after the windowed cards (re)render or the layout changes.
  $effect(() => {
    void virtual.columns;
    void virtual.windowing;
    void getGridTileSize(sizeView);
    void virtual.end;
    virtual.remeasure(innerGridEl);
  });
</script>

{#if albums.length === 0}
  <div class="empty-state">
    <p class="empty-title">No albums found</p>
    <p class="empty-description">Your album library is empty.</p>
  </div>
{:else}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="album-grid-viewport"
    style:--grid-tile-size="{currentTile()}px"
    bind:this={viewportEl}
    onscroll={virtual.handleScroll}
    tabindex="0"
    onkeydown={grid.handleKeyDown}
    aria-activedescendant={grid.selection.activeId ? `album-card-${grid.selection.activeId}` : undefined}
  >
    {#if virtual.windowing}
      <div class="album-grid-spacer" style:height="{virtual.totalSize}px">
        <div
          class="album-grid is-windowed"
          bind:this={innerGridEl}
          style:transform="translateY({virtual.padStart}px)"
        >
          {#each visibleAlbums as album (album.jellyfinId)}
            <AlbumCard
              {album}
              selected={grid.selection.isSelected(album.jellyfinId)}
              active={grid.selection.activeId === album.jellyfinId}
              onPlay={handlePlay}
              onClick={grid.handleTileClick}
              onQueue={handleQueue}
              onAddToPlaylist={handleAddToPlaylist}
            />
          {/each}
        </div>
      </div>
    {:else}
      <div class="album-grid">
        {#each albums as album (album.jellyfinId)}
          <AlbumCard
            {album}
            selected={grid.selection.isSelected(album.jellyfinId)}
            active={grid.selection.activeId === album.jellyfinId}
            onPlay={handlePlay}
            onClick={grid.handleTileClick}
            onQueue={handleQueue}
            onAddToPlaylist={handleAddToPlaylist}
          />
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .album-grid-viewport {
    height: 100%;
    overflow-y: auto;
    overflow-x: hidden;
  }

  /* Roving keyboard focus — see gridSelection.svelte.ts. */
  .album-grid-viewport:focus {
    outline: none;
  }

  .album-grid-spacer {
    position: relative;
    width: 100%;
  }

  .album-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(var(--grid-tile-size), 100%), 1fr));
    gap: var(--space-md);
  }

  /* The windowed grid is positioned inside the full-height spacer. */
  .album-grid.is-windowed {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
  }

  /* Windowing already culls off-screen cards, so disable the per-card
     content-visibility optimisation for windowed cards. Left enabled it would
     collapse buffered rows to their intrinsic size, corrupting row-height
     measurement and card positioning. */
  .album-grid.is-windowed :global(.album-card) {
    content-visibility: visible;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: var(--space-3xl);
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
