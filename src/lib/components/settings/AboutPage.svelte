<script lang="ts">
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import Copy from '@lucide/svelte/icons/copy';
  import Check from '@lucide/svelte/icons/check';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import Download from '@lucide/svelte/icons/download';
  import { onMount } from 'svelte';
  import { Button } from '@poodle64/ui/button';
  import Panel from '@poodle64/ui/panel';
  import SettingsRow from '@poodle64/ui/settings-row';
  import { getUpdaterState, checkForUpdate, downloadAndInstall } from '@poodle64/ui/updater';
  import { getAppInfo, getCacheStats, getCacheSettings } from '#lib/services/tauri.js';
  import type { AppInfo, CacheStats, CacheSettings } from '#lib/types/tauri.js';

  const updater = getUpdaterState();
  const updateBusy = $derived(updater.state === 'checking' || updater.state === 'downloading');
  const updateDescription = $derived.by(() => {
    switch (updater.state) {
      case 'checking':
        return 'Checking for a new version…';
      case 'available':
        return `Version ${updater.updateVersion} is ready to install`;
      case 'downloading':
        return `Downloading version ${updater.updateVersion}…`;
      case 'up-to-date':
        return 'You have the latest version';
      case 'error':
        return updater.error ?? 'The last check failed';
      default:
        return 'Bragi checks for a new version when it starts and every six hours';
    }
  });

  let appInfo = $state<AppInfo | null>(null);
  let cacheStats = $state<CacheStats | null>(null);
  let cacheSettings = $state<CacheSettings | null>(null);
  let copied = $state(false);

  onMount(async () => {
    try {
      const [info, stats, cache] = await Promise.all([
        getAppInfo(),
        getCacheStats(),
        getCacheSettings(),
      ]);
      appInfo = info;
      cacheStats = stats;
      cacheSettings = cache;
    } catch (e) {
      console.error('Failed to load app info:', e);
    }
  });

  function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
  }

  async function copyVersionInfo(): Promise<void> {
    if (!appInfo) return;

    const info = [
      `Bragi v${appInfo.version}`,
      `Tauri: ${appInfo.tauriVersion}`,
      `Platform: ${appInfo.platform} ${appInfo.arch}`,
      `Data directory: ${appInfo.dataDir}`,
    ];

    if (cacheStats) {
      info.push(
        `Library: ${cacheStats.trackCount} tracks, ${cacheStats.albumCount} albums, ${cacheStats.artistCount} artists`
      );
    }

    try {
      await navigator.clipboard.writeText(info.join('\n'));
      copied = true;
      setTimeout(() => {
        copied = false;
      }, 2000);
    } catch (e) {
      console.error('Failed to copy to clipboard:', e);
    }
  }
</script>

<Panel title="About" pad={false}>
  <SettingsRow label="Bragi" description="Desktop music player">
    <span class="value">v{appInfo?.version ?? '...'}</span>
  </SettingsRow>

  <SettingsRow label="Source code" description="View the project on GitHub">
    <Button
      variant="outline"
      size="sm"
      href="https://github.com/radar-hooves/bragi"
      target="_blank"
      rel="noopener noreferrer"
    >
      <ExternalLink size={14} />
      GitHub
    </Button>
  </SettingsRow>

  <SettingsRow
    label="MIT Licence"
    description="Bragi is open source software. See the project repository for full licence text and third-party acknowledgements."
  >
    <Button
      variant="outline"
      size="sm"
      href="https://github.com/radar-hooves/bragi/blob/main/LICENSE"
      target="_blank"
      rel="noopener noreferrer"
    >
      <ExternalLink size={14} />
      View
    </Button>
  </SettingsRow>

  <SettingsRow label="Updates" description={updateDescription}>
    {#if updater.state === 'available'}
      <Button size="sm" onclick={() => void downloadAndInstall()}>
        <Download size={14} />
        Install and restart
      </Button>
    {:else}
      <Button variant="outline" size="sm" onclick={() => void checkForUpdate()} disabled={updateBusy}>
        <RefreshCw size={14} class={updateBusy ? 'animate-spin' : ''} />
        {updater.state === 'downloading' ? 'Downloading' : updater.state === 'checking' ? 'Checking' : 'Check now'}
      </Button>
    {/if}
  </SettingsRow>

  <SettingsRow label="Releases" description="Release notes for every version">
    <Button
      variant="outline"
      size="sm"
      href="https://github.com/radar-hooves/bragi/releases"
      target="_blank"
      rel="noopener noreferrer"
    >
      <ExternalLink size={14} />
      Releases
    </Button>
  </SettingsRow>
</Panel>

<Panel title="The Tónlist Rune">
  <div class="rune-block">
    <img src="/icon.png" alt="The Tónlist rune" class="rune-icon" />
    <p class="rune-text">
      Bragi's emblem is the <strong>Tónlist</strong> rune — <em>tónlist</em> is Icelandic for
      &ldquo;music.&rdquo; A fitting mark for a player named after Bragi, the Norse god of
      poetry and song.
    </p>
  </div>
</Panel>

<Panel title="Build information" pad={false}>
  <SettingsRow label="Tauri version">
    <span class="value">{appInfo?.tauriVersion ?? '...'}</span>
  </SettingsRow>

  <SettingsRow label="Platform">
    <span class="value">{appInfo ? `${appInfo.platform} ${appInfo.arch}` : '...'}</span>
  </SettingsRow>

  <SettingsRow label="Copy version info" description="Copy version and system info for bug reports">
    <Button variant="outline" size="sm" onclick={copyVersionInfo} disabled={!appInfo}>
      {#if copied}
        <Check size={14} />
        Copied
      {:else}
        <Copy size={14} />
        Copy
      {/if}
    </Button>
  </SettingsRow>
</Panel>

<Panel title="Library statistics" pad={false}>
  <SettingsRow label="Tracks">
    <span class="value">{cacheStats?.trackCount.toLocaleString() ?? '...'}</span>
  </SettingsRow>

  <SettingsRow label="Albums">
    <span class="value">{cacheStats?.albumCount.toLocaleString() ?? '...'}</span>
  </SettingsRow>

  <SettingsRow label="Artists">
    <span class="value">{cacheStats?.artistCount.toLocaleString() ?? '...'}</span>
  </SettingsRow>
</Panel>

<Panel title="Storage" pad={false}>
  <SettingsRow label="Data directory">
    <span class="path">{appInfo?.dataDir ?? '...'}</span>
  </SettingsRow>

  <SettingsRow label="Cache size" description="Downloaded tracks for offline playback">
    <span class="value">{cacheSettings ? formatBytes(cacheSettings.currentSizeBytes) : '...'}</span>
  </SettingsRow>
</Panel>

<style>
  .value {
    color: var(--p11-text-secondary);
    font-size: var(--font-size-sm);
    font-variant-numeric: tabular-nums;
  }

  .path {
    max-width: 28rem;
    color: var(--p11-text-muted);
    font-family: monospace;
    font-size: var(--font-size-xs);
    word-break: break-all;
  }

  .rune-block {
    display: flex;
    align-items: center;
    gap: var(--space-lg);
  }

  .rune-icon {
    width: 84px;
    height: 84px;
    flex-shrink: 0;
    object-fit: contain;
  }

  .rune-text {
    margin: 0;
    color: var(--p11-text-secondary);
    line-height: var(--line-height-normal);
  }
</style>
