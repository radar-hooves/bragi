<script lang="ts">
  import LayoutGrid from '@lucide/svelte/icons/layout-grid';
  import List from '@lucide/svelte/icons/list';
  import Columns3 from '@lucide/svelte/icons/columns-3';
  import ChevronLeft from '@lucide/svelte/icons/chevron-left';
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import ArrowUpDown from '@lucide/svelte/icons/arrow-up-down';
  import type { SortOption } from '#lib/utils/sorting.js';
  import type { ViewMode } from '#lib/stores/ui.svelte.js';

  interface Props {
    showToolbar: boolean;
    currentSection: string;
    isShowingSearchResults: boolean;
    sectionTitle: string;
    viewMode: ViewMode;
    artistViewMode: ViewMode;
    genreViewMode: ViewMode;
    onViewModeChange: (mode: ViewMode) => void;
    onArtistViewModeChange: (mode: ViewMode) => void;
    onGenreViewModeChange: (mode: ViewMode) => void;
    columnBrowserVisible: boolean;
    onToggleColumnBrowser: () => void;
    /** null hides the slider — no grid-shaped view is active (e.g. Artists in
     * split/list view, or a non-grid section). */
    gridTileSize: number | null;
    minGridTileSize: number;
    maxGridTileSize: number;
    gridTileSizeStep: number;
    onGridTileSizeChange: (event: Event) => void;
    currentSortOptions: SortOption[];
    currentSort: string;
    onSortChange: (event: Event) => void;
    canGoBack: boolean;
    canGoForward: boolean;
    onNavigateBack: () => void;
    onNavigateForward: () => void;
  }

  let {
    showToolbar,
    currentSection,
    isShowingSearchResults,
    sectionTitle,
    viewMode,
    artistViewMode,
    genreViewMode,
    onViewModeChange,
    onArtistViewModeChange,
    onGenreViewModeChange,
    columnBrowserVisible,
    onToggleColumnBrowser,
    gridTileSize,
    minGridTileSize,
    maxGridTileSize,
    gridTileSizeStep,
    onGridTileSizeChange,
    currentSortOptions,
    currentSort,
    onSortChange,
    canGoBack,
    canGoForward,
    onNavigateBack,
    onNavigateForward,
  }: Props = $props();
</script>

<header class="toolbar" class:minimal={!showToolbar}>
  <div class="toolbar-left">
    <div class="nav-buttons">
      <button
        class="nav-btn"
        onclick={onNavigateBack}
        disabled={!canGoBack}
        aria-label="Go back"
        title="Go back (Cmd+Left)"
      >
        <ChevronLeft size={18} />
      </button>
      <button
        class="nav-btn"
        onclick={onNavigateForward}
        disabled={!canGoForward}
        aria-label="Go forward"
        title="Go forward (Cmd+Right)"
      >
        <ChevronRight size={18} />
      </button>
    </div>
    {#if showToolbar}
      <span class="view-label"
        >{isShowingSearchResults ? 'Search Results' : sectionTitle}</span
      >
    {/if}
  </div>
  {#if showToolbar}
    <div class="toolbar-right">
      {#if currentSection === 'tracks'}
        <button
          class="toolbar-icon-btn"
          class:active={columnBrowserVisible}
          onclick={onToggleColumnBrowser}
          aria-label="Toggle column browser"
          title="Toggle column browser (Cmd+B)"
        >
          <Columns3 size={18} />
        </button>
      {/if}
      {#if gridTileSize !== null}
        <input
          type="range"
          class="grid-size-slider"
          min={minGridTileSize}
          max={maxGridTileSize}
          step={gridTileSizeStep}
          value={gridTileSize}
          oninput={onGridTileSizeChange}
          aria-label="Tile size"
          title="Tile size: {gridTileSize}px"
        />
      {/if}
      {#if currentSection === 'albums'}
        <div class="view-toggle">
          <button
            class="view-toggle-btn"
            class:active={viewMode === 'grid'}
            onclick={() => onViewModeChange('grid')}
            aria-label="Grid view"
            title="Grid view"
          >
            <LayoutGrid size={16} />
          </button>
          <button
            class="view-toggle-btn"
            class:active={viewMode === 'list'}
            onclick={() => onViewModeChange('list')}
            aria-label="List view"
            title="List view"
          >
            <List size={16} />
          </button>
        </div>
      {/if}
      {#if currentSection === 'artists'}
        <div class="view-toggle">
          <button
            class="view-toggle-btn"
            class:active={artistViewMode === 'grid'}
            onclick={() => onArtistViewModeChange('grid')}
            aria-label="Grid view"
            title="Grid view"
          >
            <LayoutGrid size={16} />
          </button>
          <button
            class="view-toggle-btn"
            class:active={artistViewMode === 'list'}
            onclick={() => onArtistViewModeChange('list')}
            aria-label="List view"
            title="List view"
          >
            <List size={16} />
          </button>
        </div>
      {/if}
      {#if currentSection === 'genres'}
        <div class="view-toggle">
          <button
            class="view-toggle-btn"
            class:active={genreViewMode === 'grid'}
            onclick={() => onGenreViewModeChange('grid')}
            aria-label="Grid view"
            title="Grid view"
          >
            <LayoutGrid size={16} />
          </button>
          <button
            class="view-toggle-btn"
            class:active={genreViewMode === 'list'}
            onclick={() => onGenreViewModeChange('list')}
            aria-label="List view"
            title="List view"
          >
            <List size={16} />
          </button>
        </div>
      {/if}
      {#if currentSortOptions.length > 0}
        <span class="toolbar-separator"></span>
        <div class="sort-control">
          <ArrowUpDown size={14} class="sort-icon" />
          <div class="select-wrapper">
            <select value={currentSort} onchange={onSortChange} class="sort-select">
              {#each currentSortOptions as option (option.value)}
                <option value={option.value}>{option.label}</option>
              {/each}
            </select>
            <ChevronDown size={14} class="select-icon" />
          </div>
        </div>
      {/if}
    </div>
  {/if}
</header>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-md);
    padding: var(--space-sm) var(--space-lg) 0;
    margin-bottom: var(--space-sm);
    min-height: 36px;
  }

  .toolbar.minimal {
    margin-bottom: var(--space-xs);
  }

  .toolbar-left {
    display: flex;
    align-items: baseline;
    gap: var(--space-sm);
    flex-shrink: 0;
  }

  .nav-buttons {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .nav-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: var(--p11-radius-sm);
    color: var(--p11-text-secondary);
    cursor: pointer;
    transition:
      background-color var(--transition-fast),
      color var(--transition-fast);
  }

  .nav-btn:hover:not(:disabled) {
    background-color: rgb(var(--p11-overlay-rgb) / 0.06);
    color: var(--p11-text-primary);
  }

  .nav-btn:disabled {
    color: var(--p11-text-muted);
    opacity: 0.3;
    cursor: default;
  }

  .view-label {
    font-size: var(--font-size-xl);
    font-weight: var(--font-weight-bold);
    color: var(--p11-text-primary);
    letter-spacing: -0.02em;
  }

  .toolbar-right {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }

  @media (max-width: 640px) {
    .toolbar {
      flex-wrap: wrap;
    }

    .toolbar-right {
      justify-content: space-between;
    }
  }

  .toolbar-icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    background-color: transparent;
    border: none;
    border-radius: var(--p11-radius-sm);
    color: var(--p11-text-muted);
    cursor: pointer;
    transition:
      background-color var(--transition-fast),
      color var(--transition-fast);
  }

  .toolbar-icon-btn:hover {
    color: var(--p11-text-primary);
    background-color: rgb(var(--p11-overlay-rgb) / 0.06);
  }

  .toolbar-icon-btn.active {
    background-color: var(--p11-accent-subtle);
    color: var(--p11-accent);
  }

  .view-toggle {
    display: flex;
    background-color: rgb(var(--p11-overlay-rgb) / 0.06);
    border-radius: var(--p11-radius-sm);
    padding: 2px;
  }

  .view-toggle-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: var(--p11-radius-sm);
    color: var(--p11-text-muted);
    cursor: pointer;
    transition:
      background-color var(--transition-fast),
      color var(--transition-fast);
  }

  .view-toggle-btn:hover {
    color: var(--p11-text-primary);
  }

  .view-toggle-btn.active {
    background-color: rgb(var(--p11-overlay-rgb) / 0.1);
    color: var(--p11-text-primary);
  }

  .sort-control {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
  }

  .sort-control :global(.sort-icon) {
    color: var(--p11-text-muted);
    flex-shrink: 0;
    margin-right: 8px;
  }

  .select-wrapper {
    position: relative;
    display: inline-flex;
    align-items: center;
  }

  .sort-select {
    appearance: none;
    background: rgb(var(--p11-overlay-rgb) / 0.06);
    border: none;
    border-radius: var(--p11-radius-sm);
    padding: 6px 28px 6px 10px;
    font-size: var(--font-size-xs);
    font-family: var(--font-family);
    color: var(--p11-text-primary);
    cursor: pointer;
    transition: background-color var(--transition-fast);
  }

  .sort-select:hover {
    background: rgb(var(--p11-overlay-rgb) / 0.1);
  }

  .sort-select:focus {
    outline: none;
    background: rgb(var(--p11-overlay-rgb) / 0.1);
  }

  .select-wrapper :global(.select-icon) {
    position: absolute;
    right: 8px;
    pointer-events: none;
    color: var(--p11-text-muted);
  }

  .toolbar-separator {
    width: 1px;
    height: 20px;
    background-color: rgb(var(--p11-overlay-rgb) / 0.12);
    margin: 0 var(--space-xs);
  }

  .grid-size-slider {
    width: 80px;
    height: 4px;
    appearance: none;
    background: rgb(var(--p11-overlay-rgb) / 0.12);
    border-radius: var(--radius-full);
    cursor: pointer;
    outline: none;
  }

  .grid-size-slider::-webkit-slider-thumb {
    appearance: none;
    width: 14px;
    height: 14px;
    background: var(--p11-text-secondary);
    border-radius: var(--radius-full);
    cursor: pointer;
    transition: background-color var(--transition-fast);
  }

  .grid-size-slider::-moz-range-thumb {
    width: 14px;
    height: 14px;
    background: var(--p11-text-secondary);
    border: none;
    border-radius: var(--radius-full);
    cursor: pointer;
    transition: background-color var(--transition-fast);
  }

  .grid-size-slider:hover::-webkit-slider-thumb {
    background: var(--p11-text-primary);
  }

  .grid-size-slider:hover::-moz-range-thumb {
    background: var(--p11-text-primary);
  }
</style>
