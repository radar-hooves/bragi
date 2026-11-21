<script lang="ts">
  import type { Artist } from '#lib/types/models.js';
  import { cachedArtworkUrl, handleArtworkError, resetArtwork } from '#lib/utils/artworkFallback.js';
  import CollectionCardMenu from './CollectionCardMenu.svelte';
  import User from '@lucide/svelte/icons/user';

  interface Props {
    artist: Artist;
    /** Multi-selected (the shared selection model in ArtistGrid). */
    selected?: boolean;
    /** The keyboard cursor — the grid viewport's own `aria-activedescendant` points here. */
    active?: boolean;
    /** A click: opens the artist, unless ⌘/Shift is held, in which case
     * ArtistGrid selects instead (ArtistGrid owns the selection model, not
     * this card). */
    onClick?: (artist: Artist, event: MouseEvent) => void;
    /** A double-click: also opens the artist — same action Return triggers at the grid level. */
    onPlay?: (artist: Artist) => void;
    /** Play Next / Add to Queue share one call, parameterised like the
     * backend's own `addToQueue(tracks, position)`. */
    onQueue?: (artist: Artist, position: 'next' | 'end') => void;
    onAddToPlaylist?: (artist: Artist) => void;
  }

  let { artist, selected = false, active = false, onClick, onPlay, onQueue, onAddToPlaylist }: Props = $props();

  let menuOpen = $state(false);

  function handleContextMenu(event: MouseEvent): void {
    event.preventDefault();
    menuOpen = true;
  }

  function handleDragStart(event: DragEvent): void {
    if (!event.dataTransfer) return;

    const dragData = {
      type: 'artist' as const,
      artistId: artist.jellyfinId,
      artistName: artist.name,
    };
    event.dataTransfer.setData('application/x-bragi-playlist', JSON.stringify(dragData));
    event.dataTransfer.setData('text/plain', `Artist: ${artist.name}`);
    event.dataTransfer.effectAllowed = 'copyMove';
  }
</script>

<!-- Keyboard activation lives on the grid viewport (roving tabindex), not here. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  id="artist-card-{artist.jellyfinId}"
  data-artist-id={artist.jellyfinId}
  class="artist-card collection-card"
  class:selected
  class:active
  onclick={(e) => onClick?.(artist, e)}
  oncontextmenu={handleContextMenu}
  tabindex="-1"
  role="button"
  draggable="true"
  ondragstart={handleDragStart}
>
  <div class="artwork-container">
    {#if artist.jellyfinId}
      <img
        src={cachedArtworkUrl(artist.jellyfinId)}
        use:resetArtwork={cachedArtworkUrl(artist.jellyfinId)}
        alt={artist.name}
        class="artwork"
        loading="lazy"
        decoding="async"
        onerror={(e) => handleArtworkError(e, artist.artworkUrl)}
      />
      <div class="artwork-placeholder" style="display:none">
        <User size={40} strokeWidth={1.5} />
      </div>
    {:else}
      <div class="artwork-placeholder">
        <User size={40} strokeWidth={1.5} />
      </div>
    {/if}
  </div>
  <CollectionCardMenu
    item={artist}
    ariaLabel="Artist actions for {artist.name}"
    bind:open={menuOpen}
    {onPlay}
    {onQueue}
    {onAddToPlaylist}
  />
  <div class="artist-info">
    <h3 class="artist-name">{artist.name}</h3>
    <!-- Always render this line (blank when there are no albums) so every card
         is exactly the same height. The windowed ArtistGrid measures a single
         row stride and assumes uniform rows; a conditional line makes rows
         differ in height, which oscillates that measurement and thrashes the
         scroll virtualization into freezing the UI. -->
    <p class="album-count">
      {#if artist.albumCount > 0}
        {artist.albumCount} {artist.albumCount === 1 ? 'album' : 'albums'}
      {:else}
        &nbsp;
      {/if}
    </p>
  </div>
</div>

<style>
  .artist-card {
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
    contain-intrinsic-size: 0 220px; /* Approximate height */
  }

  .artist-card:hover {
    background-color: rgb(var(--p11-overlay-rgb) / 0.04);
  }

  .artist-card:active {
    transform: scale(0.98);
  }

  .artist-card:focus-visible {
    outline: 2px solid var(--p11-accent);
    outline-offset: 2px;
  }

  /* Multi-selected (⌘/Shift-click, or an extended arrow-key range). */
  .artist-card.selected {
    background-color: var(--p11-accent-subtle);
  }

  /* The keyboard cursor — the grid viewport (not this card) holds real DOM
     focus and points `aria-activedescendant` here. */
  .artist-card.active {
    outline: 2px solid var(--p11-border-focus);
    outline-offset: -2px;
  }

  .artwork-container {
    position: relative;
    width: 100%;
    aspect-ratio: 1;
    border-radius: var(--radius-full);
    overflow: hidden;
    box-shadow: var(--p11-shadow-card);
    transition: box-shadow var(--transition-normal);
  }

  .artist-card:hover .artwork-container {
    box-shadow: var(--p11-shadow-card-hover);
  }

  .artwork {
    width: 100%;
    height: 100%;
    object-fit: cover;
    transition: transform var(--transition-slow);
  }

  /* Linux/WebKitGTK performance optimizations */
  :global([data-platform='linux']) .artist-card {
    transition: background-color var(--transition-normal);
  }

  :global([data-platform='linux']) .artwork-container {
    box-shadow: none;
    transition: none;
  }

  :global([data-platform='linux']) .artist-card:hover .artwork-container {
    box-shadow: none;
  }

  :global([data-platform='linux']) .artwork {
    transition: none;
  }

  :global([data-platform='linux']) .artist-card:hover .artwork {
    transform: none;
  }

  .artist-card:hover .artwork {
    transform: scale(1.02);
  }

  .artwork-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--p11-bg-elevated), var(--p11-bg-surface));
    color: var(--p11-text-muted);
    opacity: 0.5;
  }

  .artist-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    padding: 0 var(--space-xs);
    text-align: center;
  }

  .artist-name {
    font-size: var(--font-size-md);
    font-weight: var(--font-weight-medium);
    color: var(--p11-text-primary);
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .album-count {
    font-size: var(--font-size-sm);
    color: var(--p11-text-secondary);
    margin: 0;
  }
</style>
