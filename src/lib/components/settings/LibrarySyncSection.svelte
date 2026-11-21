<script lang="ts">
  import * as Select from '@poodle64/ui/select';
  import { Button } from '@poodle64/ui/button';
  import { StatusBadge } from '@poodle64/ui/status-badge';
  import Panel from '@poodle64/ui/panel';
  import SettingsRow from '@poodle64/ui/settings-row';
  import { getIsJellyfinConnected } from '#lib/stores/jellyfin.svelte.js';
  import { syncLibrary, getSettings } from '#lib/services/tauri.js';
  import {
    getIsSyncing,
    getLastSyncResult,
    getLastSyncMetrics,
    getLastSyncedAt,
  } from '#lib/stores/syncStatus.svelte.js';
  import {
    getPreferences,
    setPlayCountSource,
    setLibrarySyncInterval,
    setRecentlyAddedDays,
  } from '#lib/stores/preferences.svelte.js';
  import { Spinner } from '#lib/components/common/index.js';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import { onMount } from 'svelte';

  // Read from the shared syncStatus store rather than a local
  // flag: this reflects EVERY sync trigger (this button, the toolbar, and a
  // background scheduled sync), not just one triggered from this page.
  const isSyncingLibrary = $derived(getIsSyncing());
  const librarySyncResult = $derived(getLastSyncResult());
  const lastSyncMetrics = $derived(getLastSyncMetrics());
  // The shared store only knows a timestamp once a sync has completed THIS
  // session; the initial value (from a previous session, or one the
  // scheduler ran before this page ever mounted) is read once on mount.
  let initialLastSync = $state<number | null>(null);
  const lastLibrarySync = $derived(getLastSyncedAt() ?? initialLastSync);
  const librarySyncInterval = $derived(getPreferences().librarySyncInterval);
  const recentlyAddedDays = $derived(getPreferences().recentlyAddedDays);

  const syncIntervalOptions = [
    { value: 0, label: 'Manual only' },
    { value: 3600, label: 'Every hour' },
    { value: 21600, label: 'Every 6 hours' },
    { value: 43200, label: 'Every 12 hours' },
    { value: 86400, label: 'Every day' },
    { value: 604800, label: 'Every week' },
  ];

  const recentlyAddedDaysOptions = [
    { value: 7, label: '7 days' },
    { value: 14, label: '14 days' },
    { value: 30, label: '30 days' },
    { value: 60, label: '60 days' },
    { value: 90, label: '90 days' },
  ];

  onMount(async () => {
    try {
      initialLastSync = (await getSettings()).lastLibrarySync;
    } catch (e) {
      console.error('Failed to load settings:', e);
    }
  });

  function formatLastSync(timestamp: number | null): string {
    if (!timestamp) return 'Never';
    const date = new Date(timestamp * 1000);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    const diffHours = Math.floor(diffMs / 3600000);
    const diffDays = Math.floor(diffMs / 86400000);

    if (diffMins < 1) return 'Just now';
    if (diffMins < 60) return `${diffMins} minute${diffMins === 1 ? '' : 's'} ago`;
    if (diffHours < 24) return `${diffHours} hour${diffHours === 1 ? '' : 's'} ago`;
    if (diffDays < 7) return `${diffDays} day${diffDays === 1 ? '' : 's'} ago`;
    return date.toLocaleDateString('en-AU', { day: 'numeric', month: 'short', year: 'numeric' });
  }

  function formatDuration(ms: number): string {
    if (ms < 1000) return `${ms}ms`;
    const seconds = (ms / 1000).toFixed(1);
    return `${seconds}s`;
  }

  function handleSyncIntervalChange(value: string | undefined): void {
    if (!value) return;
    setLibrarySyncInterval(parseInt(value, 10));
  }

  function handleRecentlyAddedDaysChange(value: string | undefined): void {
    if (!value) return;
    setRecentlyAddedDays(parseInt(value, 10));
  }

  const syncIntervalLabel = $derived(
    syncIntervalOptions.find((o) => o.value === librarySyncInterval)?.label ?? 'Manual only'
  );

  const recentlyAddedDaysLabel = $derived(
    recentlyAddedDaysOptions.find((o) => o.value === recentlyAddedDays)?.label ?? '30 days'
  );

  // Where play counts come from (used by AI-playlist "most played" sorting).
  // Lives here, not General: it is a library-data-source question, the same
  // family as sync interval and recently-added, not an appearance/startup toggle.
  const PLAY_COUNT_SOURCES: { value: 'auto' | 'jellyfin' | 'earworm'; label: string }[] = [
    { value: 'auto', label: 'Auto (media server, then scrobble server)' },
    { value: 'jellyfin', label: 'Media server only' },
    { value: 'earworm', label: 'Scrobble server only' },
  ];
  const playCountSourceLabel = $derived(
    PLAY_COUNT_SOURCES.find((s) => s.value === getPreferences().playCountSource)?.label ?? 'Auto'
  );
  function handlePlayCountSourceChange(value: string | undefined): void {
    if (value === 'auto' || value === 'jellyfin' || value === 'earworm') {
      setPlayCountSource(value);
    }
  }

  // Fires the sync; the button's spinner, the result banner and the metrics
  // below all update off the shared syncStatus store's library-sync-*
  // events, not this function's return value — so they update the same way
  // whether this click, the toolbar button, or the scheduler started it.
  async function handleSyncLibrary(): Promise<void> {
    if (isSyncingLibrary) return;
    try {
      await syncLibrary();
    } catch (e) {
      console.error('Library sync failed:', e);
    }
  }
</script>

<Panel
  title="Sync"
  description="Sync your music library from Jellyfin to keep your local cache up to date."
  pad={false}
>
  <SettingsRow
    label="Sync now"
    description={!getIsJellyfinConnected()
      ? 'Connect to a Jellyfin server first to sync your library.'
      : librarySyncResult === 'success' && lastSyncMetrics
        ? `Synced ${lastSyncMetrics.artistCount} artists, ${lastSyncMetrics.albumCount} albums, ${lastSyncMetrics.trackCount} tracks in ${formatDuration(lastSyncMetrics.durationMs)}`
        : librarySyncResult === 'success'
          ? 'Library synced'
          : librarySyncResult === 'skipped'
            ? 'A sync is already in progress'
            : librarySyncResult === 'error'
              ? 'Sync failed'
              : undefined}
  >
    {#if librarySyncResult === 'success' || librarySyncResult === 'error'}
      {#if getIsJellyfinConnected()}
        <StatusBadge
          status={librarySyncResult === 'success' ? 'success' : 'error'}
          label={librarySyncResult === 'success' ? 'Synced' : 'Failed'}
        />
      {/if}
    {/if}
    <Button
      variant="outline"
      size="sm"
      onclick={handleSyncLibrary}
      disabled={isSyncingLibrary || !getIsJellyfinConnected()}
    >
      {#if isSyncingLibrary}
        <Spinner />
        Syncing...
      {:else}
        <RefreshCw size={16} />
        Sync Now
      {/if}
    </Button>
  </SettingsRow>

  {#if getIsJellyfinConnected() && lastSyncMetrics && librarySyncResult === 'success'}
    <div class="sync-metrics">
      <span class="metric">
        <span class="metric-label">Fetch:</span>
        <span class="metric-value">{formatDuration(lastSyncMetrics.fetchTimeMs)}</span>
      </span>
      <span class="metric">
        <span class="metric-label">DB:</span>
        <span class="metric-value">{formatDuration(lastSyncMetrics.dbTimeMs)}</span>
      </span>
      <span class="metric">
        <span class="metric-label">API calls:</span>
        <span class="metric-value">{lastSyncMetrics.apiRequests}</span>
      </span>
      {#if lastSyncMetrics.staleArtists > 0 || lastSyncMetrics.staleAlbums > 0 || lastSyncMetrics.staleTracks > 0}
        <span class="metric stale">
          <span class="metric-label">Removed:</span>
          <span class="metric-value">{lastSyncMetrics.staleTracks} tracks</span>
        </span>
      {/if}
      {#if lastSyncMetrics.failedFetches > 0}
        <span class="metric warning">
          <span class="metric-label">Failed:</span>
          <span class="metric-value">{lastSyncMetrics.failedFetches} albums</span>
        </span>
      {/if}
    </div>
  {/if}

  <SettingsRow
    label="Auto-sync"
    description="Automatically sync your library on a schedule."
    controlId="auto-sync"
  >
    <Select.Root type="single" value={String(librarySyncInterval)} onValueChange={handleSyncIntervalChange}>
      <Select.Trigger id="auto-sync">{syncIntervalLabel}</Select.Trigger>
      <Select.Content>
        {#each syncIntervalOptions as option}
          <Select.Item value={String(option.value)}>{option.label}</Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
  </SettingsRow>

  <SettingsRow label="Last synced">
    <span class="value">{formatLastSync(lastLibrarySync)}</span>
  </SettingsRow>
</Panel>

<Panel title="Library options" pad={false}>
  <SettingsRow
    label="Recently added period"
    description="Show albums added within this time period in Recently Added."
    controlId="recently-added-days"
  >
    <Select.Root type="single" value={String(recentlyAddedDays)} onValueChange={handleRecentlyAddedDaysChange}>
      <Select.Trigger id="recently-added-days">{recentlyAddedDaysLabel}</Select.Trigger>
      <Select.Content>
        {#each recentlyAddedDaysOptions as option}
          <Select.Item value={String(option.value)}>{option.label}</Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
  </SettingsRow>

  <SettingsRow
    label="Play count source"
    description={'Where "most played" comes from (e.g. for AI playlists). Auto uses the media server\'s counts and falls back to the scrobble server; choose one if you only track plays in one place.'}
    controlId="play-count-source"
  >
    <Select.Root
      type="single"
      value={getPreferences().playCountSource}
      onValueChange={handlePlayCountSourceChange}
    >
      <Select.Trigger id="play-count-source">{playCountSourceLabel}</Select.Trigger>
      <Select.Content>
        {#each PLAY_COUNT_SOURCES as option}
          <Select.Item value={option.value}>{option.label}</Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
  </SettingsRow>
</Panel>

<style>
  .value {
    color: var(--p11-text-secondary);
    font-size: var(--font-size-sm);
    font-variant-numeric: tabular-nums;
  }

  .sync-metrics {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm) var(--space-md);
    padding: var(--space-sm) var(--space-lg);
    background-color: var(--p11-bg-elevated);
    border-top: 1px solid var(--p11-border-subtle);
    font-size: var(--font-size-xs);
  }

  .metric {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .metric-label {
    color: var(--p11-text-muted);
  }

  .metric-value {
    color: var(--p11-text-secondary);
    font-variant-numeric: tabular-nums;
  }

  .metric.stale .metric-value {
    color: var(--p11-text-muted);
  }

  .metric.warning .metric-value {
    color: var(--p11-warning);
  }
</style>
