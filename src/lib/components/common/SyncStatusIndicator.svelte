<script lang="ts">
  import {
    getPendingScrobbles,
    getHasPendingScrobbles,
    getIsServiceConfigured,
    getIsServiceOnline,
    syncScrobbles,
  } from '#lib/stores/scrobble.svelte.js';

  let isSyncing = $state(false);

  async function handleSync(): Promise<void> {
    isSyncing = true;
    try {
      await syncScrobbles();
    } catch (e) {
      console.error('Sync failed:', e);
    } finally {
      isSyncing = false;
    }
  }
</script>

{#if getIsServiceConfigured()}
  <div
    class="sync-status"
    class:has-pending={getHasPendingScrobbles()}
    title={getHasPendingScrobbles()
      ? `${getPendingScrobbles()} scrobbles waiting to sync to your music service`
      : 'All scrobbles synced'}
  >
    <div
      class="status-dot"
      class:online={getIsServiceOnline()}
      class:offline={!getIsServiceOnline()}
      title={getIsServiceOnline() ? 'Music service connected' : 'Music service offline'}
    ></div>
    {#if getHasPendingScrobbles()}
      <button
        class="sync-button"
        onclick={handleSync}
        disabled={isSyncing || !getIsServiceOnline()}
        title={getIsServiceOnline() ? 'Click to sync now' : 'Cannot sync while offline'}
      >
        {#if isSyncing}
          Syncing...
        {:else}
          {getPendingScrobbles()} scrobbles pending
        {/if}
      </button>
    {:else}
      <span class="status-text">Synced</span>
    {/if}
  </div>
{/if}

<style>
  .sync-status {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-xs) var(--space-sm);
    border-radius: var(--p11-radius-md);
    background-color: transparent;
    transition: background-color var(--transition-fast);
  }

  .sync-status.has-pending {
    background-color: var(--p11-bg-elevated);
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: var(--radius-full);
  }

  .status-dot.online {
    background-color: var(--p11-success);
  }

  .status-dot.offline {
    background-color: var(--p11-accent);
  }

  .status-text {
    font-size: var(--font-size-xs);
    color: var(--p11-text-muted);
  }

  .sync-button {
    background: transparent;
    border: none;
    padding: 0;
    font-size: var(--font-size-xs);
    color: var(--p11-text-secondary);
    cursor: pointer;
    transition: color var(--transition-fast);
  }

  .sync-button:hover:not(:disabled) {
    color: var(--p11-text-primary);
  }

  .sync-button:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }
</style>
