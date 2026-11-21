<script lang="ts">
  import FolderOpen from '@lucide/svelte/icons/folder-open';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import { onMount } from 'svelte';
  import { toast } from 'svelte-sonner';
  import { Button } from '@poodle64/ui/button';
  import { StatusBadge } from '@poodle64/ui/status-badge';
  import Panel from '@poodle64/ui/panel';
  import SettingsRow from '@poodle64/ui/settings-row';
  import TelemetryConnection from './TelemetryConnection.svelte';
  import { formatTimestampSecs } from '#lib/utils/formatTimestamp.js';
  import {
    getDeveloperInfo,
    openDataDirectory,
    openCacheDirectory,
    getPendingSyncOperations,
    clearFailedSyncOperations,
  } from '#lib/services/tauri.js';
  import type { DeveloperInfo, QueuedOperation } from '#lib/types/tauri.js';

  // Platform detection for the modifier key shown in the shortcut list.
  const isLinux = typeof navigator !== 'undefined' &&
    navigator.userAgent.toLowerCase().includes('linux');
  const mod = isLinux ? 'Ctrl' : '⌘';

  const shortcuts: { keys: string[]; action: string }[] = [
    { keys: ['Media Play/Pause'], action: 'Play or pause playback' },
    { keys: ['Media Next Track'], action: 'Skip to next track' },
    { keys: ['Media Previous Track'], action: 'Skip to previous track' },
    { keys: ['Space'], action: 'Play or pause (when app is focused)' },
    { keys: [mod, 'F'], action: 'Search library' },
    { keys: [mod, 'U'], action: 'Show or hide Up Next' },
    { keys: [mod, '+'], action: 'Zoom in' },
    { keys: [mod, '−'], action: 'Zoom out' },
    { keys: [mod, '0'], action: 'Reset zoom to 100%' },
    ...(isLinux ? [{ keys: ['Ctrl', 'Q'], action: 'Quit application' }] : []),
  ];

  let developerInfo = $state<DeveloperInfo | null>(null);
  let pendingOperations = $state<QueuedOperation[]>([]);
  let loading = $state(true);
  let clearing = $state(false);
  let refreshing = $state(false);

  onMount(async () => {
    await loadData();
  });

  async function loadData(): Promise<void> {
    try {
      const [info, operations] = await Promise.all([
        getDeveloperInfo(),
        getPendingSyncOperations(),
      ]);
      developerInfo = info;
      pendingOperations = operations;
    } catch (e) {
      console.error('Failed to load developer info:', e);
    } finally {
      loading = false;
    }
  }

  async function handleRefresh(): Promise<void> {
    refreshing = true;
    await loadData();
    refreshing = false;
  }

  async function handleOpenData(): Promise<void> {
    try {
      await openDataDirectory();
    } catch (e) {
      console.error('Failed to open data directory:', e);
      toast.error('Could not open the data folder', {
        description: e instanceof Error ? e.message : String(e),
      });
    }
  }

  async function handleOpenCache(): Promise<void> {
    try {
      await openCacheDirectory();
    } catch (e) {
      console.error('Failed to open cache directory:', e);
      toast.error('Could not open the cache folder', {
        description: e instanceof Error ? e.message : String(e),
      });
    }
  }

  async function handleClearFailed(): Promise<void> {
    clearing = true;
    try {
      const cleared = await clearFailedSyncOperations();
      if (cleared > 0) {
        await loadData();
      }
    } catch (e) {
      console.error('Failed to clear failed operations:', e);
      toast.error('Could not clear the failed operations', {
        description: e instanceof Error ? e.message : String(e),
      });
    } finally {
      clearing = false;
    }
  }

  function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
  }


  function formatOperationType(op: string): string {
    const labels: Record<string, string> = {
      scrobble: 'Scrobble',
      skip: 'Skip',
    };
    return labels[op] ?? op;
  }

  // Count failed operations (5+ attempts)
  const failedCount = $derived(pendingOperations.filter((op) => op.attempts >= 5).length);
</script>

<TelemetryConnection />

<Panel title="Developer" pad={false}>
  {#snippet action()}
    <Button variant="ghost" size="icon-sm" onclick={handleRefresh} disabled={refreshing} title="Refresh">
      <RefreshCw size={16} class={refreshing ? 'spinning' : ''} />
    </Button>
  {/snippet}

  <SettingsRow label="Debug mode" description="Application is running in debug mode">
    <StatusBadge
      status={developerInfo?.debugMode ? 'success' : 'neutral'}
      label={developerInfo?.debugMode ? 'Enabled' : 'Disabled'}
    />
  </SettingsRow>

  <SettingsRow label="Device ID" description="Unique identifier for this device">
    <span class="mono">{developerInfo?.deviceId ?? 'Not set'}</span>
  </SettingsRow>
</Panel>

<Panel title="Storage paths" pad={false}>
  <SettingsRow label="Data directory">
    <span class="mono">{developerInfo?.databasePath ?? '...'}</span>
    <Button variant="outline" size="sm" onclick={handleOpenData}>
      <FolderOpen size={14} />
      Open
    </Button>
  </SettingsRow>

  <SettingsRow label="Audio cache">
    <span class="mono">{developerInfo?.cacheDir ?? '...'}</span>
    <Button variant="outline" size="sm" onclick={handleOpenCache}>
      <FolderOpen size={14} />
      Open
    </Button>
  </SettingsRow>

  <SettingsRow label="Database size" description="SQLite database file size">
    <span class="value">
      {developerInfo ? formatBytes(developerInfo.databaseSizeBytes) : '...'}
    </span>
  </SettingsRow>
</Panel>

<Panel title="Sync queue" pad={false}>
  <SettingsRow label="Pending operations" description="Operations waiting to sync with the server">
    <span class="value">{pendingOperations.length}</span>
  </SettingsRow>

  {#if failedCount > 0}
    <SettingsRow
      label="Failed operations"
      description="Operations that exceeded maximum retry attempts"
    >
      <Button variant="destructive" size="sm" onclick={handleClearFailed} disabled={clearing}>
        <Trash2 size={14} />
        Clear {failedCount}
      </Button>
    </SettingsRow>
  {/if}

  {#each pendingOperations.slice(0, 10) as op}
    <SettingsRow label={formatOperationType(op.operation)} description={formatTimestampSecs(op.createdAt)}>
      {#if op.attempts >= 5}
        <StatusBadge status="error" label="Failed" />
      {:else if op.attempts > 0}
        <StatusBadge status="warning" label="Retry {op.attempts}" />
      {:else}
        <StatusBadge status="neutral" label="Pending" />
      {/if}
    </SettingsRow>
  {/each}
  {#if pendingOperations.length > 10}
    <p class="overflow">+{pendingOperations.length - 10} more operations</p>
  {/if}
</Panel>

<Panel
  title="Keyboard shortcuts"
  description={isLinux
    ? undefined
    : 'Media keys work globally when Bragi has Accessibility permission. On macOS, grant permission in System Settings → Privacy & Security → Accessibility.'}
  pad={false}
>
  {#each shortcuts as shortcut}
    <SettingsRow label={shortcut.action}>
      <span class="keys">
        {#each shortcut.keys as key, i}
          {#if i > 0}+{/if}
          <kbd>{key}</kbd>
        {/each}
      </span>
    </SettingsRow>
  {/each}
</Panel>

<style>
  .value {
    color: var(--p11-text-secondary);
    font-size: var(--font-size-sm);
    font-variant-numeric: tabular-nums;
  }

  .mono {
    max-width: 28rem;
    color: var(--p11-text-muted);
    font-family: monospace;
    font-size: var(--font-size-xs);
    word-break: break-all;
  }

  .overflow {
    margin: 0;
    padding: var(--space-sm) var(--space-lg);
    border-top: 1px solid var(--p11-border-subtle);
    color: var(--p11-text-muted);
    font-size: var(--font-size-sm);
    text-align: center;
  }

  .keys {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
    color: var(--p11-text-secondary);
    font-size: var(--font-size-sm);
  }

  kbd {
    padding: 2px var(--space-xs);
    background-color: var(--p11-bg-elevated);
    border: 1px solid var(--p11-border);
    border-radius: var(--p11-radius-sm);
    font-family: var(--font-family);
    font-size: var(--font-size-xs);
    color: var(--p11-text-primary);
  }

  :global(.spinning) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
  }
</style>
