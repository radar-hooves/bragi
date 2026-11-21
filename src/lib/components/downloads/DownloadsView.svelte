<script lang="ts">
  import { onMount } from 'svelte';
  import {
    getDownloadQueue,
    getDownloadState,
    getCacheSettings,
    getCacheUsagePercent,
    getPendingDownloadCount,
    pauseAllDownloads,
    resumeAllDownloads,
    clearCompletedDownloads,
    clearAllCache,
    updateCacheSizeLimit,
    formatBytes,
    refreshDownloadQueue,
    refreshDownloadState,
    refreshCacheSettings,
  } from '#lib/stores/downloads.svelte.js';
  import DownloadQueueItem from './DownloadQueueItem.svelte';
  import { Spinner } from '#lib/components/common/index.js';

  // Refresh all download state each time this view is shown.
  onMount(() => {
    refreshDownloadQueue();
    refreshDownloadState();
    refreshCacheSettings();
  });

  let isClearing = $state(false);
  let showClearConfirm = $state(false);
  let editingCacheSize = $state(false);
  let newCacheSize = $state(getCacheSettings().maxSizeBytes / (1024 * 1024 * 1024));

  // Predefined cache size options in GB
  const cacheSizeOptions = [5, 10, 20, 50, 100];

  async function handlePauseResume(): Promise<void> {
    if (getDownloadState() === 'paused') {
      await resumeAllDownloads();
    } else {
      await pauseAllDownloads();
    }
  }

  async function handleClearCompleted(): Promise<void> {
    await clearCompletedDownloads();
  }

  async function handleClearCache(): Promise<void> {
    if (!showClearConfirm) {
      showClearConfirm = true;
      return;
    }

    isClearing = true;
    try {
      await clearAllCache();
    } finally {
      isClearing = false;
      showClearConfirm = false;
    }
  }

  async function handleCacheSizeChange(sizeGb: number): Promise<void> {
    const sizeBytes = sizeGb * 1024 * 1024 * 1024;
    await updateCacheSizeLimit(sizeBytes);
    editingCacheSize = false;
  }

  function cancelClearConfirm(): void {
    showClearConfirm = false;
  }

  const hasQueueItems = $derived(getDownloadQueue().length > 0);
  const hasCompletedItems = $derived(
    getDownloadQueue().some(
      (d) => d.status === 'completed' || d.status === 'failed' || d.status === 'cancelled'
    )
  );
</script>

<div class="downloads-view">
  <header class="header">
    <h1>Downloads</h1>
    <div class="header-actions">
      {#if getPendingDownloadCount() > 0}
        <button class="btn-secondary" onclick={handlePauseResume}>
          {getDownloadState() === 'paused' ? 'Resume' : 'Pause'}
        </button>
      {/if}
      {#if hasCompletedItems}
        <button class="btn-secondary" onclick={handleClearCompleted}> Clear Completed </button>
      {/if}
    </div>
  </header>

  <!-- Cache Status -->
  <section class="cache-section">
    <div class="cache-header">
      <h2>Cache Storage</h2>
      <button class="btn-text" onclick={() => (editingCacheSize = !editingCacheSize)}>
        {editingCacheSize ? 'Cancel' : 'Change Limit'}
      </button>
    </div>

    <div class="cache-stats">
      <div class="cache-bar">
        <div
          class="cache-fill"
          class:warning={getCacheUsagePercent() > 80}
          class:danger={getCacheUsagePercent() > 95}
          style="width: {Math.min(getCacheUsagePercent(), 100)}%"
        ></div>
      </div>
      <div class="cache-info">
        <span class="cache-used">
          {formatBytes(getCacheSettings().currentSizeBytes)} used
        </span>
        <span class="cache-limit">
          of {formatBytes(getCacheSettings().maxSizeBytes)}
        </span>
        <span class="cache-tracks">
          ({getCacheSettings().cachedTrackCount} tracks)
        </span>
      </div>
    </div>

    {#if editingCacheSize}
      <div class="cache-size-options">
        <span class="option-label">Set cache limit:</span>
        {#each cacheSizeOptions as size}
          <button
            class="size-option"
            class:selected={Math.abs(getCacheSettings().maxSizeBytes / (1024 * 1024 * 1024) - size) <
              0.5}
            onclick={() => handleCacheSizeChange(size)}
          >
            {size} GB
          </button>
        {/each}
      </div>
    {/if}

    <div class="cache-actions">
      {#if showClearConfirm}
        <div class="clear-confirm">
          <span>Delete all cached files?</span>
          <button class="btn-danger" onclick={handleClearCache} disabled={isClearing}>
            {#if isClearing}
              <Spinner />
            {:else}
              Yes, Clear All
            {/if}
          </button>
          <button class="btn-secondary" onclick={cancelClearConfirm}>Cancel</button>
        </div>
      {:else}
        <button
          class="btn-danger-outline"
          onclick={handleClearCache}
          disabled={getCacheSettings().currentSizeBytes === 0}
        >
          Clear All Cache
        </button>
      {/if}
    </div>
  </section>

  <!-- Download Queue -->
  <section class="queue-section">
    <h2>
      Download Queue
      {#if getPendingDownloadCount() > 0}
        <span class="queue-badge">{getPendingDownloadCount()}</span>
      {/if}
    </h2>

    {#if getDownloadState() === 'paused' && getPendingDownloadCount() > 0}
      <div class="paused-notice">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <rect x="6" y="4" width="4" height="16" />
          <rect x="14" y="4" width="4" height="16" />
        </svg>
        Downloads paused
      </div>
    {/if}

    {#if hasQueueItems}
      <div class="queue-list">
        {#each getDownloadQueue() as download (download.trackId)}
          <DownloadQueueItem {download} />
        {/each}
      </div>
    {:else}
      <div class="empty-state">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
          <polyline points="7 10 12 15 17 10" />
          <line x1="12" y1="15" x2="12" y2="3" />
        </svg>
        <p>No downloads in queue</p>
        <p class="empty-hint">
          Click the download button on tracks or albums to download for offline playback.
        </p>
      </div>
    {/if}
  </section>
</div>

<style>
  .downloads-view {
    max-width: 800px;
    margin: 0 auto;
    padding: 0 var(--space-lg);
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-xl);
  }

  h1 {
    font-size: var(--font-size-2xl);
    font-weight: var(--font-weight-bold);
    margin: 0;
  }

  .header-actions {
    display: flex;
    gap: var(--space-sm);
  }

  h2 {
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-semibold);
    margin: 0 0 var(--space-md) 0;
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }

  .queue-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 24px;
    height: 24px;
    padding: 0 var(--space-xs);
    background-color: var(--p11-accent);
    color: rgb(var(--p11-overlay-rgb));
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-bold);
    border-radius: var(--radius-full);
  }

  .cache-section,
  .queue-section {
    background-color: var(--p11-bg-surface);
    border-radius: var(--p11-radius-lg);
    padding: var(--space-lg);
    margin-bottom: var(--space-lg);
  }

  .cache-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .cache-stats {
    margin-bottom: var(--space-md);
  }

  .cache-bar {
    height: 8px;
    background-color: var(--p11-bg-elevated);
    border-radius: var(--radius-full);
    overflow: hidden;
    margin-bottom: var(--space-sm);
  }

  .cache-fill {
    height: 100%;
    background-color: var(--p11-accent);
    border-radius: var(--radius-full);
    transition: width 0.3s ease;
  }

  .cache-fill.warning {
    background-color: var(--p11-warning);
  }

  .cache-fill.danger {
    background-color: var(--p11-error);
  }

  .cache-info {
    display: flex;
    gap: var(--space-sm);
    font-size: var(--font-size-sm);
  }

  .cache-used {
    font-weight: var(--font-weight-medium);
  }

  .cache-limit,
  .cache-tracks {
    color: var(--p11-text-secondary);
  }

  .cache-size-options {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    margin-bottom: var(--space-md);
    flex-wrap: wrap;
  }

  .option-label {
    font-size: var(--font-size-sm);
    color: var(--p11-text-secondary);
  }

  .size-option {
    padding: var(--space-xs) var(--space-sm);
    background-color: var(--p11-bg-elevated);
    border: 1px solid var(--p11-border);
    border-radius: var(--p11-radius-md);
    color: var(--p11-text-primary);
    font-size: var(--font-size-sm);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .size-option:hover {
    border-color: var(--p11-accent);
    color: var(--p11-accent);
  }

  .size-option.selected {
    background-color: var(--p11-accent);
    border-color: var(--p11-accent);
    color: rgb(var(--p11-overlay-rgb));
  }

  .cache-actions {
    display: flex;
    gap: var(--space-md);
    flex-wrap: wrap;
  }

  .clear-confirm {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-sm) var(--space-md);
    background-color: var(--p11-error-bg);
    border-radius: var(--p11-radius-md);
  }

  .clear-confirm span {
    font-size: var(--font-size-sm);
    color: var(--p11-error);
  }

  .paused-notice {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-sm) var(--space-md);
    background-color: var(--p11-warning-bg);
    border-radius: var(--p11-radius-md);
    color: var(--p11-warning);
    font-size: var(--font-size-sm);
    margin-bottom: var(--space-md);
  }

  .paused-notice svg {
    width: 16px;
    height: 16px;
  }

  .queue-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: var(--space-2xl) var(--space-lg);
    text-align: center;
    color: var(--p11-text-muted);
  }

  .empty-state svg {
    width: 48px;
    height: 48px;
    margin-bottom: var(--space-md);
    opacity: 0.5;
  }

  .empty-state p {
    margin: 0 0 var(--space-xs) 0;
  }

  .empty-hint {
    font-size: var(--font-size-sm);
  }

  .btn-secondary,
  .btn-danger,
  .btn-danger-outline,
  .btn-text {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-sm);
    padding: var(--space-xs) var(--space-md);
    border-radius: var(--p11-radius-md);
    font-size: var(--font-size-sm);
    font-family: var(--font-family);
    font-weight: var(--font-weight-medium);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .btn-secondary {
    background-color: var(--p11-bg-elevated);
    color: var(--p11-text-primary);
    border: 1px solid var(--p11-border);
  }

  .btn-secondary:hover:not(:disabled) {
    background-color: var(--p11-bg-surface);
  }

  .btn-danger {
    background-color: var(--p11-error);
    color: rgb(var(--p11-overlay-rgb));
    border: none;
  }

  .btn-danger:hover:not(:disabled) {
    background-color: var(--p11-error-hover);
  }

  .btn-danger-outline {
    background-color: transparent;
    color: var(--p11-error);
    border: 1px solid var(--p11-error);
  }

  .btn-danger-outline:hover:not(:disabled) {
    background-color: var(--p11-error-bg);
  }

  .btn-text {
    background: transparent;
    border: none;
    color: var(--p11-accent);
    padding: var(--space-xs);
  }

  .btn-text:hover {
    text-decoration: underline;
  }

  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
