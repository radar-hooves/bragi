<script lang="ts">
  import { queueTrackDownload, queueAlbumDownload } from '#lib/stores/downloads.svelte.js';
  import { Spinner } from '#lib/components/common/index.js';
  import CheckCircle from '@lucide/svelte/icons/check-circle';
  import ArrowDownToLine from '@lucide/svelte/icons/arrow-down-to-line';

  interface Props {
    /** Track ID for single track download */
    trackId?: string;
    /** Track name for display */
    trackName?: string;
    /** Artist name for display */
    artistName?: string;
    /** Album ID for full album download */
    albumId?: string;
    /** Whether the track is already downloaded */
    isDownloaded?: boolean;
    /** Size variant */
    size?: 'sm' | 'md' | 'icon';
  }

  let {
    trackId,
    trackName = '',
    artistName = '',
    albumId,
    isDownloaded = false,
    size = 'md',
  }: Props = $props();

  let isLoading = $state(false);
  let isQueued = $state(false);
  let error = $state<string | null>(null);

  const iconSize = $derived(size === 'sm' ? 12 : size === 'icon' ? 14 : 16);

  async function handleDownload(event: MouseEvent): Promise<void> {
    event.stopPropagation();
    event.preventDefault();
    if (isLoading || isDownloaded || isQueued) return;

    isLoading = true;
    error = null;

    try {
      if (albumId) {
        await queueAlbumDownload(albumId);
      } else if (trackId) {
        await queueTrackDownload(trackId, trackName, artistName);
      }
      isQueued = true;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      console.error('Download failed:', e);
    } finally {
      isLoading = false;
    }
  }
</script>

<button
  class="download-btn"
  class:downloaded={isDownloaded}
  class:queued={isQueued}
  class:size-sm={size === 'sm'}
  class:size-icon={size === 'icon'}
  onclick={handleDownload}
  disabled={isLoading || isDownloaded || isQueued}
  title={isDownloaded
    ? 'Downloaded'
    : isQueued
      ? 'Queued for download'
      : albumId
        ? 'Download album'
        : 'Download track'}
>
  {#if isLoading}
    <Spinner />
  {:else if isDownloaded}
    <CheckCircle size={iconSize} />
  {:else if isQueued}
    <CheckCircle size={iconSize} />
  {:else}
    <ArrowDownToLine size={iconSize} />
  {/if}
</button>

{#if error}
  <span class="download-error" title={error}>!</span>
{/if}

<style>
  .download-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    padding: 0;
    background: transparent;
    border: 1px solid var(--p11-border);
    border-radius: var(--radius-full);
    color: var(--p11-text-secondary);
    cursor: pointer;
    transition:
      color var(--transition-fast),
      border-color var(--transition-fast),
      background-color var(--transition-fast);
  }

  .download-btn.size-sm {
    width: 24px;
    height: 24px;
  }

  .download-btn.size-icon {
    width: auto;
    height: auto;
    border: none;
    background: none;
  }

  .download-btn.size-icon:disabled {
    opacity: 1;
  }

  .download-btn:hover:not(:disabled) {
    color: var(--p11-accent);
    border-color: var(--p11-accent);
  }

  .download-btn.downloaded {
    color: var(--p11-success);
    border: none;
    cursor: default;
  }

  .download-btn.queued {
    color: var(--p11-accent);
    border: none;
    cursor: default;
  }

  .download-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .download-error {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    background-color: var(--p11-accent);
    color: rgb(var(--p11-overlay-rgb));
    font-size: var(--font-size-2xs);
    font-weight: bold;
    border-radius: var(--radius-full);
    cursor: help;
  }
</style>
