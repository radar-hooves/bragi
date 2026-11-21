<!--
  ArtistHeader.svelte

  The artist name, its "N albums, M songs" line and the Play / Shuffle / Download
  actions, shared by ArtistView (full page and the right
  pane of the list layout). The header sizes itself from its own width, not the
  window's: the name clamps to two lines and the actions drop below it when the
  row runs out of room.

  Key the instance by artist so the "Queued" state resets when the artist changes.
-->
<script lang="ts">
  import { queueAlbumDownload } from '#lib/stores/downloads.svelte.js';
  import { Spinner } from '#lib/components/common/index.js';
  import CheckCircle from '@lucide/svelte/icons/check-circle';
  import Download from '@lucide/svelte/icons/download';
  import Play from '@lucide/svelte/icons/play';
  import Shuffle from '@lucide/svelte/icons/shuffle';

  interface Props {
    name: string;
    albumIds: string[];
    totalTracks: number;
    downloadedTracks: number;
    hasTracks: boolean;
    onPlayAll: () => void;
    onShuffleAll: () => void;
  }

  let { name, albumIds, totalTracks, downloadedTracks, hasTracks, onPlayAll, onShuffleAll }: Props =
    $props();

  let isDownloadingArtist = $state(false);
  let isQueued = $state(false);

  const albumCount = $derived(albumIds.length);
  const isFullyDownloaded = $derived(totalTracks > 0 && downloadedTracks >= totalTracks);
  const isPartiallyDownloaded = $derived(downloadedTracks > 0 && downloadedTracks < totalTracks);

  async function handleDownloadArtist(event: MouseEvent): Promise<void> {
    event.stopPropagation();
    if (isDownloadingArtist || isQueued || isFullyDownloaded) return;
    isDownloadingArtist = true;
    try {
      for (const id of albumIds) {
        await queueAlbumDownload(id);
      }
      isQueued = true;
    } catch (e) {
      console.error('Failed to queue artist download:', e);
    } finally {
      isDownloadingArtist = false;
    }
  }
</script>

<div class="hero">
  <div class="hero-content">
    <div class="hero-left">
      <h1 class="artist-name" title={name}>{name}</h1>
      <span class="artist-stats">
        {#if albumCount > 0}{albumCount}
          {albumCount === 1 ? 'album' : 'albums'}{/if}{#if totalTracks > 0}, {totalTracks}
          {totalTracks === 1 ? 'song' : 'songs'}{/if}
      </span>
    </div>

    <div class="artist-actions">
      {#if hasTracks}
        <button class="action-btn primary" onclick={onPlayAll} title="Play all tracks">
          <Play size={18} fill="currentColor" />
          <span>Play</span>
        </button>
        <button class="action-btn" onclick={onShuffleAll} title="Shuffle all tracks">
          <Shuffle size={18} />
          <span>Shuffle</span>
        </button>
      {/if}
      {#if isFullyDownloaded}
        <span class="action-btn downloaded" title="All {totalTracks} tracks downloaded">
          <CheckCircle size={18} />
          <span>Downloaded</span>
        </span>
      {:else if isPartiallyDownloaded}
        <button
          class="action-btn partial"
          class:queued={isQueued}
          onclick={handleDownloadArtist}
          disabled={isDownloadingArtist || isQueued}
          title="{downloadedTracks} of {totalTracks} tracks downloaded"
        >
          {#if isDownloadingArtist}
            <Spinner size={18} />
            <span>Queueing...</span>
          {:else if isQueued}
            <CheckCircle size={18} />
            <span>Queued</span>
          {:else}
            <Download size={18} />
            <span>{downloadedTracks}/{totalTracks}</span>
          {/if}
        </button>
      {:else if albumCount > 0}
        <button
          class="action-btn"
          class:queued={isQueued}
          onclick={handleDownloadArtist}
          disabled={isDownloadingArtist || isQueued}
          title="Download all albums"
        >
          {#if isDownloadingArtist}
            <Spinner size={18} />
            <span>Queueing...</span>
          {:else if isQueued}
            <CheckCircle size={18} />
            <span>Queued</span>
          {:else}
            <Download size={18} />
            <span>Download</span>
          {/if}
        </button>
      {/if}
    </div>
  </div>
</div>

<style>
  .hero {
    position: relative;
    z-index: 1;
    flex-shrink: 0;
    container-type: inline-size;
    /* On a very short pane the header scrolls rather than cut off its actions. */
    max-height: 60%;
    overflow-y: auto;
  }

  /* The identity block keeps the name and count on one line when the pane allows;
     the actions then wrap below it rather than squeeze it. */
  .hero-content {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-md) var(--space-lg);
    padding: var(--space-lg) var(--space-xl) var(--space-xl);
  }

  .hero-left {
    display: flex;
    flex: 1 0 auto;
    flex-wrap: wrap;
    max-width: 100%;
    align-items: baseline;
    gap: var(--space-xs) var(--space-md);
    min-width: 0;
  }

  .artist-name {
    font-size: var(--font-size-3xl);
    font-weight: var(--font-weight-bold);
    color: var(--p11-text-primary);
    margin: 0;
    max-width: 100%;
    letter-spacing: -0.02em;
    text-shadow: 0 2px 12px rgb(var(--p11-shade-rgb) / 0.4);
    /* Artist names can be arbitrarily long and need not contain spaces. */
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
  }

  .artist-stats {
    font-size: var(--font-size-md);
    color: var(--p11-text-secondary);
    text-shadow: 0 2px 8px rgb(var(--p11-shade-rgb) / 0.3);
    white-space: nowrap;
  }

  .artist-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-sm);
  }

  .action-btn {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 10px var(--space-lg);
    font-size: var(--font-size-sm);
    font-family: var(--font-family);
    font-weight: var(--font-weight-medium);
    color: var(--p11-text-primary);
    background: rgb(var(--p11-overlay-rgb) / 0.1);
    border: none;
    border-radius: var(--radius-full);
    cursor: pointer;
    transition:
      background-color var(--transition-fast),
      transform var(--transition-spring);
  }

  .action-btn:hover:not(:disabled) {
    background: rgb(var(--p11-overlay-rgb) / 0.15);
  }

  .action-btn:active:not(:disabled) {
    transform: scale(0.97);
  }

  .action-btn.primary {
    background: var(--p11-accent);
    color: rgb(var(--p11-overlay-rgb));
  }

  .action-btn.primary:hover:not(:disabled) {
    background: var(--p11-accent-hover);
  }

  .action-btn.downloaded {
    color: var(--p11-success);
    cursor: default;
  }

  .action-btn.partial {
    color: var(--p11-warning);
  }

  .action-btn.queued {
    color: var(--p11-accent);
  }

  .action-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .action-btn.queued:disabled {
    opacity: 1;
  }

  /* Container queries in rem: WebKit compares px against the unzoomed window,
     so only rem follows UI zoom. */
  @container (width < 36rem) {
    .artist-name {
      font-size: var(--font-size-2xl);
    }
  }

  /* A narrow pane keeps the actions on one row as icons; the labels stay
     readable to assistive tech. */
  @container (width < 24rem) {
    .hero-content {
      padding: var(--space-md);
    }

    .artist-name {
      -webkit-line-clamp: 1;
      line-clamp: 1;
    }

    .artist-stats {
      white-space: normal;
    }

    .action-btn {
      padding: 10px var(--space-md);
    }

    .action-btn span {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }
  }
</style>
