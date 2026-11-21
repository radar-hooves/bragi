<script lang="ts">
  import { cancelDownload, formatBytes } from '#lib/stores/downloads.svelte.js';
  import { Spinner } from '#lib/components/common/index.js';
  import type { QueuedDownload } from '#lib/types/tauri.js';

  interface Props {
    download: QueuedDownload;
  }

  let { download }: Props = $props();

  let isCancelling = $state(false);

  async function handleCancel(): Promise<void> {
    if (isCancelling) return;
    isCancelling = true;
    try {
      await cancelDownload(download.trackId);
    } finally {
      isCancelling = false;
    }
  }

  const statusLabel = $derived(() => {
    switch (download.status) {
      case 'pending':
        return 'Waiting...';
      case 'downloading':
        return `${download.progressPercent}%`;
      case 'completed':
        return 'Downloaded';
      case 'failed':
        return 'Failed';
      case 'cancelled':
        return 'Cancelled';
      default:
        return '';
    }
  });

  const showProgress = $derived(download.status === 'downloading' || download.status === 'pending');

  const showCancel = $derived(download.status === 'pending' || download.status === 'downloading');
</script>

<div
  class="queue-item"
  class:completed={download.status === 'completed'}
  class:failed={download.status === 'failed'}
>
  <div class="item-info">
    <span class="track-name">{download.trackName}</span>
    <span class="artist-name">{download.artistName}</span>
  </div>

  <div class="item-status">
    {#if download.status === 'downloading'}
      <div class="progress-info">
        <span class="progress-text">{statusLabel()}</span>
        {#if download.totalBytes}
          <span class="size-info">
            {formatBytes(download.bytesDownloaded)} / {formatBytes(download.totalBytes)}
          </span>
        {/if}
      </div>
      <div class="progress-bar">
        <div class="progress-fill" style="width: {download.progressPercent}%"></div>
      </div>
    {:else if download.status === 'pending'}
      <div class="pending-indicator">
        <Spinner />
        <span>{statusLabel()}</span>
      </div>
    {:else if download.status === 'completed'}
      <span class="status-completed">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="m9 12 2 2 4-4" />
        </svg>
        Downloaded
      </span>
    {:else if download.status === 'failed'}
      <span class="status-failed" title={download.error}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="12" cy="12" r="10" />
          <line x1="15" y1="9" x2="9" y2="15" />
          <line x1="9" y1="9" x2="15" y2="15" />
        </svg>
        Failed
      </span>
    {:else if download.status === 'cancelled'}
      <span class="status-cancelled">Cancelled</span>
    {/if}
  </div>

  {#if showCancel}
    <button
      class="cancel-btn"
      onclick={handleCancel}
      disabled={isCancelling}
      title="Cancel download"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <line x1="18" y1="6" x2="6" y2="18" />
        <line x1="6" y1="6" x2="18" y2="18" />
      </svg>
    </button>
  {/if}
</div>

<style>
  .queue-item {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    padding: var(--space-md);
    background-color: var(--p11-bg-surface);
    border-radius: var(--p11-radius-md);
  }

  .queue-item.completed {
    opacity: 0.7;
  }

  .queue-item.failed {
    border-left: 3px solid var(--p11-accent);
  }

  .item-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .track-name {
    font-weight: var(--font-weight-medium);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .artist-name {
    font-size: var(--font-size-sm);
    color: var(--p11-text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .item-status {
    flex-shrink: 0;
    min-width: 120px;
    text-align: right;
  }

  .progress-info {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 2px;
    margin-bottom: var(--space-xs);
  }

  .progress-text {
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
  }

  .size-info {
    font-size: var(--font-size-xs);
    color: var(--p11-text-muted);
  }

  .progress-bar {
    width: 100%;
    height: 4px;
    background-color: var(--p11-bg-elevated);
    border-radius: var(--radius-full);
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background-color: var(--p11-accent);
    border-radius: var(--radius-full);
    transition: width 0.2s ease;
  }

  .pending-indicator {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-sm);
    font-size: var(--font-size-sm);
    color: var(--p11-text-secondary);
  }

  .status-completed {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
    font-size: var(--font-size-sm);
    color: var(--p11-success);
  }

  .status-completed svg {
    width: 16px;
    height: 16px;
  }

  .status-failed {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
    font-size: var(--font-size-sm);
    color: var(--p11-accent);
    cursor: help;
  }

  .status-failed svg {
    width: 16px;
    height: 16px;
  }

  .status-cancelled {
    font-size: var(--font-size-sm);
    color: var(--p11-text-muted);
  }

  .cancel-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    background: transparent;
    border: none;
    color: var(--p11-text-muted);
    cursor: pointer;
    border-radius: var(--p11-radius-sm);
    transition:
      color var(--transition-fast),
      background-color var(--transition-fast);
  }

  .cancel-btn:hover:not(:disabled) {
    color: var(--p11-error);
    background-color: var(--p11-error-bg);
  }

  .cancel-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .cancel-btn svg {
    width: 16px;
    height: 16px;
  }
</style>
