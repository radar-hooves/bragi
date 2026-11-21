<script lang="ts">
  import type { Album, Track } from '#lib/types/models.js';
  import { cachedArtworkUrl, handleArtworkError, resetArtwork } from '#lib/utils/artworkFallback.js';
  import { getArtworkUrl } from '#lib/services/tauri.js';
  import { getPlaybackState } from '#lib/stores/player.svelte.js';
  import { queueTrackNext, queueTrackAtEnd } from '#lib/utils/queueActions.js';
  import TrackContextMenu from './TrackContextMenu.svelte';
  import Pause from '@lucide/svelte/icons/pause';
  import Music from '@lucide/svelte/icons/music';
  import CheckCircle from '@lucide/svelte/icons/check-circle';
  import { DownloadButton } from '#lib/components/downloads/index.js';

  interface Props {
    album: Album;
    tracks: Track[];
    isLoadingTracks: boolean;
    loadFailed?: boolean;
    onTrackPlay?: (track: Track, albumTracks: Track[]) => void;
    onAddToPlaylist?: (tracks: Track[]) => void;
  }

  let {
    album,
    tracks,
    isLoadingTracks,
    loadFailed = false,
    onTrackPlay,
    onAddToPlaylist,
  }: Props = $props();

  const isFullyDownloaded = $derived(
    album.downloadedTrackCount >= album.trackCount && album.trackCount > 0
  );
  const isPartiallyDownloaded = $derived(
    album.downloadedTrackCount > 0 && album.downloadedTrackCount < album.trackCount
  );

  let artworkSrc = $state<string | null>(null);
  let artworkError = $state(false);

  $effect(() => {
    artworkError = false;
    artworkSrc = null;
    if (album.artworkUrl) {
      artworkSrc = album.artworkUrl;
    } else {
      getArtworkUrl(album.jellyfinId, 240)
        .then((url) => {
          artworkSrc = url;
        })
        .catch(() => {
          artworkError = true;
        });
    }
  });

  function formatDuration(ms: number): string {
    const totalSeconds = Math.floor(ms / 1000);
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;
    return `${minutes}:${seconds.toString().padStart(2, '0')}`;
  }

  function handleTrackDblClick(track: Track): void {
    onTrackPlay?.(track, tracks);
  }

  function isTrackPlaying(track: Track): boolean {
    return getPlaybackState().currentTrack?.jellyfinId === track.jellyfinId && getPlaybackState().isPlaying;
  }

  function isTrackCurrent(track: Track): boolean {
    return getPlaybackState().currentTrack?.jellyfinId === track.jellyfinId;
  }

  /** Distinct disc numbers present in the track list (sorted). */
  const discNumbers = $derived.by(() => {
    const discs = new Set(tracks.map((t) => t.discNumber));
    return [...discs].sort((a, b) => a - b);
  });

  const hasMultipleDiscs = $derived(discNumbers.length > 1);

  /** Tracks grouped by disc number, preserving track order within each disc. */
  const tracksByDisc = $derived.by(() => {
    const map = new Map<number, Track[]>();
    for (const track of tracks) {
      const disc = track.discNumber;
      if (!map.has(disc)) map.set(disc, []);
      map.get(disc)!.push(track);
    }
    return map;
  });

  // One right-click-anywhere-on-the-row opener per track menu (bind:open on
  // TrackContextMenu), keyed by id — the same pattern TrackRow uses.
  let menuOpenFor = $state<string | null>(null);

  function handleTrackContextMenu(event: MouseEvent, track: Track): void {
    event.preventDefault();
    menuOpenFor = track.jellyfinId;
  }

  function handleDragStart(event: DragEvent, track: Track): void {
    if (!event.dataTransfer) return;
    event.dataTransfer.setData('application/x-bragi-playlist', JSON.stringify({ type: 'tracks', trackIds: [track.jellyfinId] }));
    event.dataTransfer.setData('text/plain', `Track: ${track.name}`);
    event.dataTransfer.effectAllowed = 'copyMove';
  }
</script>

<div class="album-section">
  <div class="album-artwork-container">
    <div class="album-artwork">
      {#if artworkSrc && !artworkError}
        <img
          src={artworkSrc}
          use:resetArtwork={artworkSrc}
          alt="{album.name} artwork"
          loading="lazy"
          onerror={(e) => handleArtworkError(e, cachedArtworkUrl(album.jellyfinId))}
        />
      {:else}
        <div class="artwork-placeholder">
          <Music size={24} />
        </div>
      {/if}
    </div>
  </div>

  <div class="album-content">
    <div class="album-header">
      <span class="album-name">{album.name}</span>
      {#if album.year}
        <span class="album-year">{album.year}</span>
      {/if}
      {#if isFullyDownloaded}
        <span class="album-cached-badge" title="All {album.trackCount} tracks downloaded">
          <CheckCircle size={14} />
        </span>
      {:else if isPartiallyDownloaded}
        <span
          class="album-cached-badge partial"
          title="{album.downloadedTrackCount} of {album.trackCount} tracks downloaded"
        >
          <CheckCircle size={14} />
        </span>
        <span class="album-download">
          <DownloadButton size="sm" albumId={album.jellyfinId} />
        </span>
      {:else}
        <span class="album-download">
          <DownloadButton size="sm" albumId={album.jellyfinId} />
        </span>
      {/if}
    </div>

    {#if isLoadingTracks}
      <div class="loading-tracks">Loading tracks...</div>
    {:else if loadFailed}
      <div class="no-tracks unavailable">Tracks unavailable — sync library to load</div>
    {:else if tracks.length === 0}
      <div class="no-tracks">No tracks</div>
    {:else}
      <div class="track-table" role="list">
        {#each discNumbers as disc (disc)}
          {#if hasMultipleDiscs}
            <div class="disc-divider">
              <span class="disc-label">Disc {disc}</span>
            </div>
          {/if}
          {#each tracksByDisc.get(disc) ?? [] as track (track.jellyfinId)}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="track-row"
              class:now-playing={isTrackCurrent(track)}
              draggable="true"
              ondblclick={() => handleTrackDblClick(track)}
              oncontextmenu={(e) => handleTrackContextMenu(e, track)}
              ondragstart={(e) => handleDragStart(e, track)}
            >
              <span class="track-indicator">
                {#if isTrackPlaying(track)}
                  <span class="playing-indicator" title="Now playing">
                    <span class="eq-bar"></span>
                    <span class="eq-bar"></span>
                    <span class="eq-bar"></span>
                  </span>
                {:else if isTrackCurrent(track)}
                  <span class="paused-indicator">
                    <Pause size={10} fill="currentColor" />
                  </span>
                {:else}
                  <span class="track-number">{track.trackNumber}/{album.trackCount}</span>
                {/if}
              </span>
              <span class="track-name">{track.name}</span>
              <span class="track-duration">{formatDuration(track.durationMs)}</span>
              <span class="track-menu">
                <TrackContextMenu
                  {track}
                  showAddToPlaylist={!!onAddToPlaylist}
                  bind:open={
                    () => menuOpenFor === track.jellyfinId,
                    (v) => { menuOpenFor = v ? track.jellyfinId : null; }
                  }
                  onPlay={() => onTrackPlay?.(track, tracks)}
                  onQueueNext={() => queueTrackNext([track])}
                  onQueueEnd={() => queueTrackAtEnd([track])}
                  onAddToPlaylist={() => onAddToPlaylist?.([track])}
                />
              </span>
            </div>
          {/each}
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .album-section {
    display: grid;
    grid-template-columns: var(--sidebar-width) 1fr;
    gap: var(--space-lg);
    padding-bottom: var(--space-xl);
    margin-bottom: var(--space-xl);
    border-bottom: 1px solid rgb(var(--p11-overlay-rgb) / 0.04);
  }

  .album-section:last-child {
    border-bottom: none;
    margin-bottom: 0;
    padding-bottom: 0;
  }

  .album-artwork-container {
    position: relative;
    width: var(--sidebar-width);
    /* Stretch to match content height - critical for sticky to work */
    align-self: stretch;
  }

  .album-artwork {
    position: sticky;
    top: 0;
    width: var(--sidebar-width);
    height: var(--sidebar-width);
    border-radius: var(--p11-radius-sm);
    overflow: hidden;
    box-shadow: var(--p11-shadow-sm);
  }

  .album-artwork img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
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

  .album-content {
    min-width: 0;
  }

  .album-header {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    margin-bottom: var(--space-sm);
  }

  .album-name {
    font-size: var(--font-size-md);
    font-weight: var(--font-weight-semibold);
    color: var(--p11-text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .album-year {
    font-size: var(--font-size-xs);
    color: var(--p11-text-muted);
    flex-shrink: 0;
  }

  .album-cached-badge {
    display: inline-flex;
    align-items: center;
    color: var(--p11-success);
    flex-shrink: 0;
  }

  .album-cached-badge.partial {
    color: var(--p11-warning);
  }

  .album-download {
    flex-shrink: 0;
  }

  .loading-tracks,
  .no-tracks {
    font-size: var(--font-size-sm);
    color: var(--p11-text-muted);
    padding: var(--space-xs) 0;
  }

  .no-tracks.unavailable {
    font-style: italic;
  }

  .track-table {
    display: flex;
    flex-direction: column;
  }

  .disc-divider {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-xs) var(--space-sm);
    margin-top: var(--space-xs);
  }

  .disc-divider:first-child {
    margin-top: 0;
  }

  .disc-label {
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
    color: var(--p11-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    white-space: nowrap;
  }

  .disc-divider::after {
    content: '';
    flex: 1;
    height: 1px;
    background-color: rgb(var(--p11-overlay-rgb) / 0.06);
  }

  .track-row {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
    height: 28px;
    padding: 0 var(--space-xs) 0 var(--space-md);
    background: transparent;
    border-radius: var(--p11-radius-sm);
    cursor: pointer;
    transition: background-color var(--transition-fast);
  }

  .track-row:hover {
    background-color: rgb(var(--p11-overlay-rgb) / 0.04);
  }

  .track-row:focus-visible {
    outline: 2px solid var(--p11-accent);
    outline-offset: -2px;
  }

  .track-row.now-playing {
    background-color: var(--p11-now-playing);
  }

  .track-row.now-playing:hover {
    background-color: var(--p11-accent-subtle);
  }

  .track-row.now-playing .track-name {
    color: var(--p11-accent);
  }

  .track-indicator {
    width: 24px;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    flex-shrink: 0;
  }

  .track-number {
    font-size: var(--font-size-sm);
    color: var(--p11-text-muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .playing-indicator {
    display: flex;
    align-items: flex-end;
    justify-content: center;
    gap: 1.5px;
    width: 20px;
    height: 20px;
    padding-bottom: 4px;
  }

  .eq-bar {
    width: 2.5px;
    background-color: var(--p11-accent);
    border-radius: 1px;
    animation: eq-bounce 0.8s ease-in-out infinite;
  }

  .eq-bar:nth-child(1) {
    height: 7px;
    animation-delay: 0s;
  }

  .eq-bar:nth-child(2) {
    height: 10px;
    animation-delay: 0.2s;
  }

  .eq-bar:nth-child(3) {
    height: 5px;
    animation-delay: 0.4s;
  }

  @keyframes eq-bounce {
    0%,
    100% {
      transform: scaleY(0.4);
    }
    50% {
      transform: scaleY(1);
    }
  }

  .paused-indicator {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    color: var(--p11-accent);
  }

  .track-name {
    flex: 1;
    min-width: 0;
    font-size: var(--font-size-sm);
    color: var(--p11-text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding-left: var(--space-xs);
  }

  .track-duration {
    font-size: var(--font-size-xs);
    color: var(--p11-text-muted);
    flex-shrink: 0;
    font-variant-numeric: tabular-nums;
    width: 40px;
    text-align: right;
  }

  .track-menu {
    flex-shrink: 0;
    opacity: 0;
    transition: opacity var(--transition-fast);
  }

  .track-row:hover .track-menu,
  .track-row:focus-within .track-menu {
    opacity: 1;
  }
</style>
