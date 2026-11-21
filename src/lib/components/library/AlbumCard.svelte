<script lang="ts">
  import type { Album } from '#lib/types/models.js';
  import { cachedArtworkUrl, handleArtworkError, resetArtwork } from '#lib/utils/artworkFallback.js';
  import { DownloadButton } from '#lib/components/downloads/index.js';
  import CollectionCardMenu from './CollectionCardMenu.svelte';
  import CheckCircle from '@lucide/svelte/icons/check-circle';

  interface Props {
    album: Album;
    /** Multi-selected (the shared selection model in AlbumGrid). */
    selected?: boolean;
    /** The keyboard cursor — the grid viewport's own `aria-activedescendant` points here. */
    active?: boolean;
    onPlay?: (album: Album) => void;
    /** A click: opens the album, unless ⌘/Shift is held, in which case
     * AlbumGrid selects instead (AlbumGrid owns the selection model, not
     * this card). */
    onClick?: (album: Album, event: MouseEvent) => void;
    /** A double-click: also opens the album — same action Return triggers at the grid level. */
    /** Play Next / Add to Queue share one call, parameterised like the
     * backend's own `addToQueue(tracks, position)` — never a scalar variant
     * beside a plural one. */
    onQueue?: (album: Album, position: 'next' | 'end') => void;
    onAddToPlaylist?: (album: Album) => void;
  }

  let { album, selected = false, active = false, onPlay, onClick, onQueue, onAddToPlaylist }: Props = $props();

  let menuOpen = $state(false);

  const isFullyDownloaded = $derived(
    album.downloadedTrackCount > 0 && album.downloadedTrackCount >= album.trackCount
  );
  const isPartiallyDownloaded = $derived(
    album.downloadedTrackCount > 0 && album.downloadedTrackCount < album.trackCount
  );

  function handlePlayClick(event: MouseEvent): void {
    event.stopPropagation();
    onPlay?.(album);
  }

  function handleContextMenu(event: MouseEvent): void {
    event.preventDefault();
    menuOpen = true;
  }

  function handleDragStart(event: DragEvent): void {
    if (!event.dataTransfer) return;

    // Store album ID - tracks will be fetched when dropped
    const dragData = {
      type: 'album' as const,
      albumId: album.jellyfinId,
      albumName: album.name,
    };
    event.dataTransfer.setData('application/x-bragi-playlist', JSON.stringify(dragData));
    // Also set text/plain for debugging - some browsers need this
    event.dataTransfer.setData('text/plain', `Album: ${album.name}`);
    event.dataTransfer.effectAllowed = 'copyMove';
  }
</script>

<!-- Keyboard activation lives on the grid viewport (roving tabindex), not here. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  id="album-card-{album.jellyfinId}"
  data-album-id={album.jellyfinId}
  class="album-card collection-card"
  class:selected
  class:active
  onclick={(e) => onClick?.(album, e)}
  oncontextmenu={handleContextMenu}
  tabindex="-1"
  role="button"
  draggable="true"
  ondragstart={handleDragStart}
>
  <div class="artwork-container">
    {#if album.jellyfinId}
      <img
        src={cachedArtworkUrl(album.jellyfinId)}
        use:resetArtwork={cachedArtworkUrl(album.jellyfinId)}
        alt="{album.name} artwork"
        class="artwork"
        loading="lazy"
        decoding="async"
        onerror={(e) => handleArtworkError(e, album.artworkUrl)}
      />
      <div class="artwork-placeholder" style="display:none">
        <span class="artwork-icon">♫</span>
      </div>
    {:else}
      <div class="artwork-placeholder">
        <span class="artwork-icon">♫</span>
      </div>
    {/if}
    <div class="overlay">
      <button class="overlay-play" onclick={handlePlayClick} aria-label="Play {album.name}">
        <span class="play-icon">▶</span>
      </button>
    </div>
  </div>
  <CollectionCardMenu
    item={album}
    ariaLabel="Album actions for {album.name}"
    bind:open={menuOpen}
    {onPlay}
    {onQueue}
    {onAddToPlaylist}
  />
  <div class="album-info">
    <div class="album-row">
      <h3 class="album-name">{album.name}</h3>
      {#if isFullyDownloaded}
        <span
          class="download-indicator downloaded"
          title="All {album.trackCount} tracks downloaded"
        >
          <CheckCircle size={14} />
        </span>
      {:else if isPartiallyDownloaded}
        <span
          class="download-indicator partial"
          title="{album.downloadedTrackCount} of {album.trackCount} tracks downloaded"
        >
          <CheckCircle size={14} />
        </span>
      {:else}
        <span class="download-indicator">
          <DownloadButton size="icon" albumId={album.jellyfinId} />
        </span>
      {/if}
    </div>
    <div class="artist-row">
      <p class="artist-name">{album.artistName}</p>
    </div>
  </div>
</div>

<style>
  .album-card {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    padding: var(--space-sm);
    background-color: transparent;
    border-radius: var(--p11-radius-lg);
    cursor: pointer;
    transition:
      background-color var(--transition-normal),
      transform var(--transition-spring);
    /* Skip rendering when off-screen for better scroll performance */
    content-visibility: auto;
    contain-intrinsic-size: 0 240px; /* Approximate height */
  }

  .album-card:hover {
    background-color: rgb(var(--p11-overlay-rgb) / 0.04);
  }

  .album-card:active {
    transform: scale(0.98);
  }

  .album-card:focus-visible {
    outline: 2px solid var(--p11-accent);
    outline-offset: 2px;
  }

  /* Multi-selected (⌘/Shift-click, or an extended arrow-key range). */
  .album-card.selected {
    background-color: var(--p11-accent-subtle);
  }

  /* The keyboard cursor — the grid viewport (not this card) holds real DOM
     focus and points `aria-activedescendant` here. */
  .album-card.active {
    outline: 2px solid var(--p11-border-focus);
    outline-offset: -2px;
  }

  .artwork-container {
    position: relative;
    width: 100%;
    aspect-ratio: 1;
    border-radius: var(--p11-radius-md);
    overflow: hidden;
    box-shadow: var(--p11-shadow-card);
    transition: box-shadow var(--transition-normal);
  }

  .album-card:hover .artwork-container {
    box-shadow: var(--p11-shadow-card-hover);
  }

  .artwork {
    width: 100%;
    height: 100%;
    object-fit: cover;
    transition: transform var(--transition-slow);
  }

  /* Linux/WebKitGTK performance optimizations */
  :global([data-platform='linux']) .album-card {
    transition: background-color var(--transition-normal);
  }

  :global([data-platform='linux']) .artwork-container {
    box-shadow: none;
    transition: none;
  }

  :global([data-platform='linux']) .album-card:hover .artwork-container {
    box-shadow: none;
  }

  :global([data-platform='linux']) .artwork {
    transition: none;
    image-rendering: auto;
  }

  :global([data-platform='linux']) .album-card:hover .artwork {
    transform: none;
  }

  :global([data-platform='linux']) .overlay {
    transition: none;
  }

  .album-card:hover .artwork {
    transform: scale(1.02);
  }

  .artwork-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--p11-bg-elevated), var(--p11-bg-surface));
  }

  .artwork-icon {
    font-size: var(--font-size-3xl);
    color: var(--p11-text-muted);
    opacity: 0.5;
  }

  .overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    padding: var(--space-sm);
    background: linear-gradient(to top, rgb(var(--p11-shade-rgb) / 0.6) 0%, transparent 60%);
    opacity: 0;
    transition: opacity var(--transition-normal);
    pointer-events: none;
  }

  .album-card:hover .overlay {
    opacity: 1;
  }

  .album-row,
  .artist-row {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
    min-width: 0;
  }

  .artist-row {
    align-items: baseline;
  }

  .download-indicator {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    color: var(--p11-text-muted);
  }

  .download-indicator.downloaded {
    color: var(--p11-success);
  }

  .download-indicator.partial {
    color: var(--p11-warning);
  }

  .overlay-play {
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    pointer-events: auto;
  }

  .play-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    background-color: rgb(var(--p11-overlay-rgb) / 0.95);
    color: rgb(var(--p11-shade-rgb));
    border-radius: var(--radius-full);
    font-size: var(--font-size-lg);
    box-shadow: 0 4px 12px rgb(var(--p11-shade-rgb) / 0.4);
    transition: transform var(--transition-spring);
    padding-left: 2px;
  }

  .overlay-play:hover .play-icon,
  .overlay-play:focus-visible .play-icon {
    transform: scale(1.08);
  }

  .overlay-play:focus-visible {
    outline: 2px solid var(--p11-accent);
    outline-offset: -2px;
  }

  .album-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    padding: 0 var(--space-xs);
  }

  .album-name {
    font-size: var(--font-size-md);
    font-weight: var(--font-weight-medium);
    color: var(--p11-text-primary);
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }

  .artist-name {
    font-size: var(--font-size-sm);
    color: var(--p11-text-secondary);
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }
</style>
