<script lang="ts">
  import type { Artist } from '#lib/types/models.js';
  import { getGridTileSize } from '#lib/stores/ui.svelte.js';
  import ArtistGridCard from './ArtistGridCard.svelte';
  import { createVirtualList } from '#lib/utils/virtualList.svelte.js';
  import { createGridSelection } from '#lib/utils/gridSelection.svelte.js';
  import { GRID_GAP_PX, DEFAULT_GRID_TILE_PX, gridColumnsForWidth } from '#lib/utils/gridTiles.js';

  interface Props {
    artists: Artist[];
    /** Keys this grid's remembered scroll position. */
    viewKey?: string;
    /** Opens the artist (see gridSelection.svelte.ts). */
    onArtistClick?: (artist: Artist) => void;
    /** Play, Play Next, Add to Queue and Add to Playlist all act on the whole
     * grid selection when the acted-on tile is selected — never just the one
     * tile under the pointer (the same rule TrackList's
     * own tracksForAction uses for a track row). */
    onArtistPlay?: (artists: Artist[]) => void;
    onArtistQueue?: (artists: Artist[], position: 'next' | 'end') => void;
    onArtistAddToPlaylist?: (artists: Artist[]) => void;
  }

  let {
    artists,
    viewKey = 'artists-grid',
    onArtistClick,
    onArtistPlay,
    onArtistQueue,
    onArtistAddToPlaylist,
  }: Props = $props();

  // --- Virtual grid ---------------------------------------------------------
  // Only the cards near the viewport are mounted, through the shared
  // virtualiser every long list routes through.
  //
  // ROW_HEIGHT assumption: artist artwork is a square that fills the column
  // width, so a row's height is (column width + label/padding/gap). We seed
  // that estimate (LABEL_OVERHEAD ~= tile+56, matching the card's
  // contain-intrinsic-size of ~220 at the default 200px tile) and the
  // virtualiser corrects it by measuring the real rendered stride, so the
  // spacer height and scroll mapping stay pixel-accurate across tile sizes
  // and UI zoom.
  const LABEL_OVERHEAD = 56; // column-width -> row-height estimate before measuring

  let viewportEl = $state<HTMLDivElement>();
  let innerGridEl = $state<HTMLDivElement>();

  function currentTile(): number {
    const size = getGridTileSize('artists');
    return size > 0 ? size : DEFAULT_GRID_TILE_PX;
  }

  function columnsForWidth(width: number): number {
    return gridColumnsForWidth(width, currentTile());
  }

  const virtual = createVirtualList({
    count: () => artists.length,
    columns: columnsForWidth,
    estimateRowHeight: (width, cols) => {
      const colWidth = (width - (cols - 1) * GRID_GAP_PX) / cols;
      return Math.max(1, colWidth + LABEL_OVERHEAD);
    },
    overscan: 3,
    bypassBelow: 250,
  });

  let visibleArtists = $derived(artists.slice(virtual.start, virtual.end));

  // Selection + keyboard: see gridSelection.svelte.ts
  const grid = createGridSelection<Artist>({
    getItems: () => artists,
    getId: (a) => a.jellyfinId,
    getLabel: (a) => a.name,
    getViewport: () => viewportEl,
    idAttribute: 'data-artist-id',
    columns: () => (viewportEl ? columnsForWidth(viewportEl.clientWidth) : 1),
    onActivate: (a) => onArtistClick?.(a),
    virtual,
  });

  const artistById = $derived.by(() => new Map(artists.map((a) => [a.jellyfinId, a])));

  // A menu on a tile that's part of a multi-selection acts on the WHOLE
  // selection, matching TrackList's own tracksForAction — a tile outside the current selection still acts on just itself
  // (`selection.targetIds`).
  function artistsForAction(artist: Artist): Artist[] {
    return grid.selection.targetIds(artist.jellyfinId).map((id) => artistById.get(id)).filter((a): a is Artist => !!a);
  }

  function handlePlay(artist: Artist): void {
    onArtistPlay?.(artistsForAction(artist));
  }

  function handleQueue(artist: Artist, position: 'next' | 'end'): void {
    onArtistQueue?.(artistsForAction(artist), position);
  }

  function handleAddToPlaylist(artist: Artist): void {
    onArtistAddToPlaylist?.(artistsForAction(artist));
  }

  // Own the viewport's resize handling and remember its scroll position.
  $effect(() => virtual.attachViewport(viewportEl));
  $effect(() => virtual.bindScrollMemory(viewKey, viewportEl));

  // Recompute the range when the data or tile size change.
  $effect(() => {
    void artists.length;
    void getGridTileSize('artists');
    virtual.refresh();
  });

  // Re-measure after the windowed cards (re)render or the layout changes.
  $effect(() => {
    void virtual.columns;
    void virtual.windowing;
    void getGridTileSize('artists');
    void virtual.end;
    virtual.remeasure(innerGridEl);
  });
</script>

{#if artists.length === 0}
  <div class="empty-state">
    <p class="empty-title">No artists found</p>
    <p class="empty-description">Your artist library is empty.</p>
  </div>
{:else}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="artist-grid-viewport"
    style:--grid-tile-size="{currentTile()}px"
    bind:this={viewportEl}
    onscroll={virtual.handleScroll}
    tabindex="0"
    onkeydown={grid.handleKeyDown}
    aria-activedescendant={grid.selection.activeId ? `artist-card-${grid.selection.activeId}` : undefined}
  >
    {#if virtual.windowing}
      <div class="artist-grid-spacer" style:height="{virtual.totalSize}px">
        <div
          class="artist-grid is-windowed"
          bind:this={innerGridEl}
          style:transform="translateY({virtual.padStart}px)"
        >
          {#each visibleArtists as artist (artist.jellyfinId)}
            <ArtistGridCard
              {artist}
              selected={grid.selection.isSelected(artist.jellyfinId)}
              active={grid.selection.activeId === artist.jellyfinId}
              onClick={grid.handleTileClick}
              onPlay={handlePlay}
              onQueue={handleQueue}
              onAddToPlaylist={handleAddToPlaylist}
            />
          {/each}
        </div>
      </div>
    {:else}
      <div class="artist-grid">
        {#each artists as artist (artist.jellyfinId)}
          <ArtistGridCard
            {artist}
            selected={grid.selection.isSelected(artist.jellyfinId)}
            active={grid.selection.activeId === artist.jellyfinId}
            onClick={grid.handleTileClick}
            onPlay={handlePlay}
            onQueue={handleQueue}
            onAddToPlaylist={handleAddToPlaylist}
          />
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .artist-grid-viewport {
    height: 100%;
    overflow-y: auto;
    overflow-x: hidden;
  }

  /* Roving keyboard focus — see gridSelection.svelte.ts. */
  .artist-grid-viewport:focus {
    outline: none;
  }

  .artist-grid-spacer {
    position: relative;
    width: 100%;
  }

  .artist-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(var(--grid-tile-size), 100%), 1fr));
    gap: var(--space-md);
  }

  /* The windowed grid is positioned inside the full-height spacer. */
  .artist-grid.is-windowed {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
  }

  /* Windowing already culls off-screen cards, so disable the per-card
     content-visibility optimisation for windowed cards. Left enabled it would
     collapse buffered rows to their intrinsic size, corrupting row-height
     measurement and card positioning. */
  .artist-grid.is-windowed :global(.artist-card) {
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
