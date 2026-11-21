<script lang="ts">
  import type { Album, Track } from '#lib/types/models.js';
  import { cachedArtworkUrl, handleArtworkError, resetArtwork } from '#lib/utils/artworkFallback.js';
  import { getPlaybackState } from '#lib/stores/player.svelte.js';
  import { DownloadButton } from '#lib/components/downloads/index.js';
  import Pause from '@lucide/svelte/icons/pause';
  import CheckCircle from '@lucide/svelte/icons/check-circle';
  import Play from '@lucide/svelte/icons/play';
  import { slide } from 'svelte/transition';

  interface Props {
    album: Album;
    isExpanded: boolean;
    tracks: Track[];
    isLoading: boolean;
    discNumbers: number[];
    tracksByDisc: Map<number, Track[]>;
    hasMultipleDiscs: boolean;
    onToggle: (album: Album) => void;
    onAlbumPlay?: (album: Album) => void;
    onTrackPlay?: (track: Track, albumTracks: Track[]) => void;
  }

  let {
    album,
    isExpanded,
    tracks,
    isLoading,
    discNumbers,
    tracksByDisc,
    hasMultipleDiscs,
    onToggle,
    onAlbumPlay,
    onTrackPlay,
  }: Props = $props();

  // Return expands/collapses the row — the same action a click does. Space is
  // left alone: it's the global play/pause shortcut (globalShortcuts.ts),
  // never a per-row action.
  function handleRowKeyDown(event: KeyboardEvent): void {
    if (event.key === 'Enter') {
      event.preventDefault();
      onToggle(album);
    }
  }

  function formatAlbumDuration(ms: number): string {
    const minutes = Math.floor(ms / 60000);
    if (minutes >= 60) {
      const hours = Math.floor(minutes / 60);
      const remainingMinutes = minutes % 60;
      return `${hours} hr ${remainingMinutes} min`;
    }
    return `${minutes} min`;
  }

  function formatTrackDuration(ms: number): string {
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

  function handleDownloadClick(event: MouseEvent): void {
    event.stopPropagation();
  }

  function handlePlayClick(event: MouseEvent): void {
    event.stopPropagation();
    onAlbumPlay?.(album);
  }
</script>

<div class="album-item" class:expanded={isExpanded}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="album-row"
    onclick={() => onToggle(album)}
    ondblclick={() => onAlbumPlay?.(album)}
    onkeydown={handleRowKeyDown}
    tabindex="0"
    role="button"
    aria-expanded={isExpanded}
  >
    <div class="artwork-container">
      {#if album.artworkUrl}
        <img
          src={album.artworkUrl}
          use:resetArtwork={album.artworkUrl}
          alt="{album.name} artwork"
          class="artwork"
          loading="lazy"
          onerror={(e) => handleArtworkError(e, cachedArtworkUrl(album.jellyfinId))}
        />
        <div class="artwork-placeholder" style="display:none">
          <span class="artwork-icon">♫</span>
        </div>
      {:else}
        <div class="artwork-placeholder">
          <span class="artwork-icon">♫</span>
        </div>
      {/if}
      <button
        type="button"
        class="play-overlay"
        onclick={handlePlayClick}
        aria-label="Play {album.name}"
      >
        <Play size={18} fill="currentColor" />
      </button>
    </div>
    <div class="album-info">
      <span class="album-name">{album.name}</span>
      <span class="artist-name">{album.artistName}</span>
    </div>
    <span class="album-year">{album.year ?? ''}</span>
    <span class="album-tracks">{album.trackCount} tracks</span>
    <span class="album-duration">{formatAlbumDuration(album.durationMs)}</span>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <span
      class="download-cell"
      onclick={handleDownloadClick}
      onkeydown={(e) => e.stopPropagation()}
    >
      <DownloadButton
        size="sm"
        albumId={album.jellyfinId}
        isDownloaded={album.downloadedTrackCount >= album.trackCount && album.trackCount > 0}
      />
    </span>
  </div>

  {#if isExpanded}
    <div class="track-panel" transition:slide={{ duration: 200 }}>
      {#if isLoading && tracks.length === 0}
        <div class="loading-tracks">Loading tracks...</div>
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
                ondblclick={() => handleTrackDblClick(track)}
              >
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
                {/if}
                <span
                  class="track-cached-icon"
                  class:visible={!!track.cachedFilePath}
                  title={track.cachedFilePath ? 'Downloaded' : ''}
                >
                  {#if track.cachedFilePath}<CheckCircle size={10} />{/if}
                </span>
                <span class="track-number">{track.trackNumber}</span>
                <span class="track-name">{track.name}</span>
                <span class="track-duration">{formatTrackDuration(track.durationMs)}</span>
              </div>
            {/each}
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .album-item { border-bottom: 1px solid rgb(var(--p11-overlay-rgb) / 0.03); }
  .album-item:last-child { border-bottom: none; }
  .album-item.expanded { background-color: rgb(var(--p11-overlay-rgb) / 0.02); }

  .album-row {
    display: grid; grid-template-columns: var(--list-artwork-size) 1fr auto auto auto 36px;
    gap: var(--space-md); align-items: center; padding: var(--space-sm) var(--space-md);
    background: transparent; cursor: pointer; transition: background-color var(--transition-fast);
  }

  .album-row:hover { background-color: rgb(var(--p11-overlay-rgb) / 0.04); }
  .album-row:focus-visible { outline: 2px solid var(--p11-accent); outline-offset: -2px; }

  .artwork-container {
    position: relative; width: var(--list-artwork-size); height: var(--list-artwork-size);
    border-radius: var(--p11-radius-sm); overflow: hidden; flex-shrink: 0; box-shadow: var(--p11-shadow-sm);
  }

  .artwork { width: 100%; height: 100%; object-fit: cover; }
  .artwork-placeholder { width: 100%; height: 100%; display: flex; align-items: center; justify-content: center; background: linear-gradient(135deg, var(--p11-bg-elevated), var(--p11-bg-surface)); }
  .artwork-icon { font-size: var(--font-size-xl); color: var(--p11-text-muted); opacity: 0.5; }

  .play-overlay {
    position: absolute; inset: 0; display: flex; align-items: center; justify-content: center;
    background-color: rgb(var(--p11-shade-rgb) / 0.5); color: rgb(var(--p11-overlay-rgb)); border: none; cursor: pointer;
    opacity: 0; transition: opacity var(--transition-fast);
  }

  .album-row:hover .play-overlay { opacity: 1; }
  .play-overlay:focus-visible { opacity: 1; outline: 2px solid var(--p11-accent); outline-offset: -2px; }
  .album-info { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .album-name { font-size: var(--font-size-md); font-weight: var(--font-weight-medium); color: var(--p11-text-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .artist-name { font-size: var(--font-size-sm); color: var(--p11-text-secondary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .album-year, .album-tracks, .album-duration { font-size: var(--font-size-sm); color: var(--p11-text-muted); white-space: nowrap; font-variant-numeric: tabular-nums; }
  .download-cell { display: flex; align-items: center; justify-content: center; }
  .track-panel { padding: 0 var(--space-md) var(--space-md); }
  .loading-tracks, .no-tracks { font-size: var(--font-size-sm); color: var(--p11-text-muted); padding: var(--space-sm) 0; }
  .track-table { display: flex; flex-direction: column; }
  .disc-divider { display: flex; align-items: center; gap: var(--space-sm); padding: var(--space-xs) var(--space-sm); margin-top: var(--space-xs); }
  .disc-divider:first-child { margin-top: 0; }
  .disc-label { font-size: var(--font-size-xs); font-weight: var(--font-weight-semibold); color: var(--p11-text-muted); text-transform: uppercase; letter-spacing: 0.05em; white-space: nowrap; }
  .disc-divider::after { content: ''; flex: 1; height: 1px; background-color: rgb(var(--p11-overlay-rgb) / 0.06); }

  .track-row {
    display: flex; align-items: center; gap: var(--space-sm); height: 28px;
    padding: 0 var(--space-sm); background: transparent; border-radius: var(--p11-radius-sm);
    cursor: pointer; transition: background-color var(--transition-fast);
  }

  .track-row:hover { background-color: rgb(var(--p11-overlay-rgb) / 0.04); }
  .track-row.now-playing { background-color: var(--p11-now-playing); }
  .track-row.now-playing:hover { background-color: var(--p11-accent-subtle); }
  .track-row.now-playing .track-name { color: var(--p11-accent); }

  .playing-indicator { display: flex; align-items: flex-end; justify-content: center; gap: 1.5px; width: 20px; height: 20px; padding-bottom: 4px; }
  .eq-bar { width: 2.5px; background-color: var(--p11-accent); border-radius: 1px; animation: eq-bounce 0.8s ease-in-out infinite; }
  .eq-bar:nth-child(1) { height: 7px; animation-delay: 0s; }
  .eq-bar:nth-child(2) { height: 10px; animation-delay: 0.2s; }
  .eq-bar:nth-child(3) { height: 5px; animation-delay: 0.4s; }
  @keyframes eq-bounce { 0%, 100% { transform: scaleY(0.4); } 50% { transform: scaleY(1); } }

  .paused-indicator { display: flex; align-items: center; justify-content: center; width: 20px; height: 20px; color: var(--p11-accent); }
  .track-number { width: 24px; text-align: right; font-size: var(--font-size-sm); color: var(--p11-text-muted); flex-shrink: 0; font-variant-numeric: tabular-nums; }
  .track-name { flex: 1; min-width: 0; font-size: var(--font-size-sm); color: var(--p11-text-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .track-cached-icon { display: inline-flex; align-items: center; justify-content: center; width: 12px; flex-shrink: 0; color: var(--p11-success); opacity: 0; }
  .track-cached-icon.visible { opacity: 1; }
  .track-duration { font-size: var(--font-size-xs); color: var(--p11-text-muted); flex-shrink: 0; font-variant-numeric: tabular-nums; width: 40px; text-align: right; }

  @media (max-width: 768px) {
    .album-row { grid-template-columns: var(--list-artwork-size) 1fr 36px; }
    .album-year, .album-tracks, .album-duration { display: none; }
  }
</style>
