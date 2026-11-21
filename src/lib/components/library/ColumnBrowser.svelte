<script lang="ts">
  import { untrack } from 'svelte';
  import {
    getColumnBrowserData,
    getColumnBrowserState,
    selectGenre,
    selectArtist,
    selectAlbum,
    clearGenreSelection,
    clearArtistSelection,
    clearAlbumSelection,
  } from '#lib/stores/columnBrowser.svelte.js';
  import {
    getUiState,
    setColumnBrowserHeight,
    toggleColumnBrowserColumn,
    MIN_COLUMN_BROWSER_HEIGHT,
    MAX_COLUMN_BROWSER_HEIGHT,
    type ColumnBrowserColumn as ColumnType,
  } from '#lib/stores/ui.svelte.js';
  import ColumnBrowserColumn from './ColumnBrowserColumn.svelte';
  import Check from '@lucide/svelte/icons/check';

  interface Props {
    visible?: boolean;
    onGenrePlay?: (genre: string) => void;
    onArtistPlay?: (artist: string) => void;
    onAlbumPlay?: (albumId: string) => void;
  }

  let { visible = true, onGenrePlay, onArtistPlay, onAlbumPlay }: Props = $props();

  // Context menu state
  let showContextMenu = $state(false);
  let contextMenuX = $state(0);
  let contextMenuY = $state(0);

  // Resize drag state
  let isDragging = $state(false);
  let dragStartY = $state(0);
  let dragStartHeight = $state(0);

  function onResizePointerDown(e: PointerEvent): void {
    isDragging = true;
    dragStartY = e.clientY;
    dragStartHeight = getUiState().columnBrowserHeight;

    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);

    // Prevent text selection while dragging
    document.body.style.userSelect = 'none';
    document.body.style.cursor = 'row-resize';
  }

  function onResizePointerMove(e: PointerEvent): void {
    if (!isDragging) return;

    const delta = e.clientY - dragStartY;
    const newHeight = Math.max(
      MIN_COLUMN_BROWSER_HEIGHT,
      Math.min(MAX_COLUMN_BROWSER_HEIGHT, dragStartHeight + delta)
    );
    setColumnBrowserHeight(newHeight);
  }

  function onResizePointerUp(): void {
    if (!isDragging) return;
    isDragging = false;
    document.body.style.userSelect = '';
    document.body.style.cursor = '';
  }

  /** Event with modifier keys for multi-select detection */
  interface ModifierKeyEvent {
    metaKey: boolean;
    ctrlKey: boolean;
  }

  function handleGenreClick(genre: string, event: ModifierKeyEvent): void {
    const multiSelect = event.metaKey || event.ctrlKey;
    selectGenre(genre, multiSelect);
  }

  function handleArtistClick(artist: string, event: ModifierKeyEvent): void {
    const multiSelect = event.metaKey || event.ctrlKey;
    selectArtist(artist, multiSelect);
  }

  function handleAlbumClick(albumId: string, event: ModifierKeyEvent): void {
    const multiSelect = event.metaKey || event.ctrlKey;
    selectAlbum(albumId, multiSelect);
  }

  function handleContextMenu(e: MouseEvent): void {
    e.preventDefault();
    contextMenuX = e.clientX;
    contextMenuY = e.clientY;
    showContextMenu = true;
  }

  function closeContextMenu(): void {
    showContextMenu = false;
  }

  function handleToggleColumn(column: ColumnType): void {
    toggleColumnBrowserColumn(column);
    closeContextMenu();
  }

  function handleKeyDown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      closeContextMenu();
    }
  }

  // Visible columns
  const visibleColumns = $derived(getUiState().columnBrowserColumns);
  const showGenres = $derived(visibleColumns.includes('genres'));
  const showArtists = $derived(visibleColumns.includes('artists'));
  const showAlbums = $derived(visibleColumns.includes('albums'));

  const cbd = $derived(getColumnBrowserData());
  const genreItems = $derived(
    cbd.genres.map((genre) => ({
      id: genre,
      label: genre,
      selected: getColumnBrowserState().selectedGenres.has(genre),
    }))
  );
  // Track the array + selection-Set refs, but map the (up to a few thousand)
  // deeply-reactive items under untrack() to avoid the O(n^2) dep-tracking
  // cliff. Selection Sets are replaced wholesale, so the ref read stays reactive.
  const artistItems = $derived.by(() => {
    const artists = cbd.artists;
    const selected = getColumnBrowserState().selectedArtists;
    return untrack(() =>
      artists.map((artist) => ({
        id: artist,
        label: artist,
        selected: selected.has(artist),
      }))
    );
  });
  const albumItems = $derived.by(() => {
    const albums = cbd.albums;
    const selected = getColumnBrowserState().selectedAlbums;
    return untrack(() =>
      albums.map((album) => ({
        id: album.id,
        label: album.name,
        sublabel: album.artistName,
        selected: selected.has(album.id),
      }))
    );
  });
</script>

<svelte:window on:click={closeContextMenu} on:keydown={handleKeyDown} />

{#if visible}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="column-browser"
    style:height="{getUiState().columnBrowserHeight}px"
    oncontextmenu={handleContextMenu}
  >
    {#if showGenres}
      <div class="column">
        <ColumnBrowserColumn
          title="GENRES"
          items={genreItems}
          allCount={cbd.totalTrackCount}
          allSelected={getColumnBrowserState().selectedGenres.size === 0}
          onItemClick={handleGenreClick}
          onItemDoubleClick={onGenrePlay}
          onAllClick={clearGenreSelection}
        />
      </div>
    {/if}

    {#if showArtists}
      <div class="column">
        <ColumnBrowserColumn
          title="ARTISTS"
          items={artistItems}
          allCount={cbd.artists.length}
          allSelected={getColumnBrowserState().selectedArtists.size === 0}
          onItemClick={handleArtistClick}
          onItemDoubleClick={onArtistPlay}
          onAllClick={clearArtistSelection}
        />
      </div>
    {/if}

    {#if showAlbums}
      <div class="column">
        <ColumnBrowserColumn
          title="ALBUMS"
          items={albumItems}
          allCount={cbd.albums.length}
          allSelected={getColumnBrowserState().selectedAlbums.size === 0}
          onItemClick={handleAlbumClick}
          onItemDoubleClick={onAlbumPlay}
          onAllClick={clearAlbumSelection}
          showSublabel
        />
      </div>
    {/if}
  </div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="resize-handle"
    class:active={isDragging}
    onpointerdown={onResizePointerDown}
    onpointermove={onResizePointerMove}
    onpointerup={onResizePointerUp}
    onpointercancel={onResizePointerUp}
  ></div>
{/if}

{#if showContextMenu}
  <div
    class="context-menu"
    role="menu"
    tabindex="-1"
    style:left="{contextMenuX}px"
    style:top="{contextMenuY}px"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => {
      if (e.key === 'Escape') showContextMenu = false;
    }}
  >
    <div class="context-menu-header">Show Columns</div>
    <button
      class="context-menu-item"
      onclick={() => handleToggleColumn('genres')}
      disabled={showGenres && visibleColumns.length === 1}
    >
      <span class="check-icon" class:visible={showGenres}>
        <Check size={14} />
      </span>
      Genres
    </button>
    <button
      class="context-menu-item"
      onclick={() => handleToggleColumn('artists')}
      disabled={showArtists && visibleColumns.length === 1}
    >
      <span class="check-icon" class:visible={showArtists}>
        <Check size={14} />
      </span>
      Artists
    </button>
    <button
      class="context-menu-item"
      onclick={() => handleToggleColumn('albums')}
      disabled={showAlbums && visibleColumns.length === 1}
    >
      <span class="check-icon" class:visible={showAlbums}>
        <Check size={14} />
      </span>
      Albums
    </button>
  </div>
{/if}

<style>
  .column-browser {
    display: flex;
    background-color: var(--p11-bg-surface);
    border-radius: var(--p11-radius-md);
    overflow: hidden;
  }

  .column {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--p11-border);
    overflow: hidden;
  }

  .column:last-child {
    border-right: none;
  }

  .resize-handle {
    height: 6px;
    cursor: row-resize;
    position: relative;
    flex-shrink: 0;
  }

  .resize-handle::after {
    content: '';
    position: absolute;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    width: 32px;
    height: 3px;
    border-radius: var(--radius-full);
    background-color: var(--p11-border);
    transition: background-color var(--transition-fast);
  }

  .resize-handle:hover::after,
  .resize-handle.active::after {
    background-color: var(--p11-text-muted);
  }

  /* Context menu */
  .context-menu {
    position: fixed;
    z-index: var(--z-dropdown);
    min-width: 160px;
    background-color: var(--p11-bg-elevated);
    border: 1px solid var(--p11-border);
    border-radius: var(--p11-radius-md);
    box-shadow: var(--p11-shadow-lg);
    padding: var(--space-xs) 0;
  }

  .context-menu-header {
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
    color: var(--p11-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: var(--space-xs) var(--space-md);
    margin-bottom: var(--space-xs);
  }

  .context-menu-item {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    width: 100%;
    padding: var(--space-xs) var(--space-md);
    background: none;
    border: none;
    font-family: var(--font-family);
    font-size: var(--font-size-sm);
    color: var(--p11-text-primary);
    cursor: pointer;
    text-align: left;
    transition: background-color var(--transition-fast);
  }

  .context-menu-item:hover:not(:disabled) {
    background-color: var(--p11-bg-hover);
  }

  .context-menu-item:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .check-icon {
    width: 14px;
    height: 14px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--p11-accent);
    opacity: 0;
  }

  .check-icon.visible {
    opacity: 1;
  }
</style>
