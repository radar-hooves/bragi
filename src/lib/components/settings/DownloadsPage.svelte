<script lang="ts">
  import * as Select from '@poodle64/ui/select';
  import { Button } from '@poodle64/ui/button';
  import { Progress } from '@poodle64/ui/progress';
  import Panel from '@poodle64/ui/panel';
  import SettingsRow from '@poodle64/ui/settings-row';
  import { getCacheSettings, setCacheSettings, clearCache, getAppInfo } from '#lib/services/tauri.js';
  import { getPreferences, setDownloadFormat, setDownloadBitrate } from '#lib/stores/preferences.svelte.js';
  import { Spinner } from '#lib/components/common/index.js';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
  import { onMount } from 'svelte';
  import { toast } from 'svelte-sonner';
  import type { CacheSettings, AppInfo } from '#lib/types/tauri.js';

  type DownloadFormat = 'original' | 'flac' | 'mp3' | 'aac' | 'opus';

  let cacheSettings = $state<CacheSettings | null>(null);
  let appInfo = $state<AppInfo | null>(null);
  let isClearingCache = $state(false);
  let showClearConfirm = $state(false);
  let cacheSizeLimit = $state<number>(5 * 1024 * 1024 * 1024); // 5GB default
  const downloadFormat = $derived(getPreferences().downloadFormat);
  const downloadBitrate = $derived(getPreferences().downloadBitrate);

  const cacheSizeLimitOptions = [
    { value: 1 * 1024 * 1024 * 1024, label: '1 GB' },
    { value: 2 * 1024 * 1024 * 1024, label: '2 GB' },
    { value: 5 * 1024 * 1024 * 1024, label: '5 GB' },
    { value: 10 * 1024 * 1024 * 1024, label: '10 GB' },
    { value: 20 * 1024 * 1024 * 1024, label: '20 GB' },
    { value: 50 * 1024 * 1024 * 1024, label: '50 GB' },
    { value: 0, label: 'Unlimited' },
  ];

  const downloadFormatOptions: { value: DownloadFormat; label: string }[] = [
    { value: 'original', label: 'Original (no transcode)' },
    { value: 'flac', label: 'FLAC (lossless)' },
    { value: 'mp3', label: 'MP3' },
    { value: 'aac', label: 'AAC' },
    { value: 'opus', label: 'Opus' },
  ];

  const downloadBitrateOptions = [
    { value: 128, label: '128 kbps' },
    { value: 192, label: '192 kbps' },
    { value: 256, label: '256 kbps' },
    { value: 320, label: '320 kbps' },
  ];

  const lossyFormats: DownloadFormat[] = ['mp3', 'aac', 'opus'];
  const isLossyFormat = $derived(lossyFormats.includes(downloadFormat));

  const downloadFormatLabel = $derived(
    downloadFormatOptions.find((o) => o.value === downloadFormat)?.label ?? 'Original (no transcode)'
  );
  const downloadBitrateLabel = $derived(
    downloadBitrateOptions.find((o) => o.value === downloadBitrate)?.label ?? '256 kbps'
  );

  onMount(async () => {
    try {
      const [cache, info] = await Promise.all([getCacheSettings(), getAppInfo()]);
      cacheSettings = cache;
      cacheSizeLimit = cache.maxSizeBytes;
      appInfo = info;
    } catch (e) {
      console.error('Failed to load cache settings:', e);
    }
  });

  function handleDownloadFormatChange(value: string | undefined): void {
    if (!value) return;
    setDownloadFormat(value as DownloadFormat);
  }

  function handleDownloadBitrateChange(value: string | undefined): void {
    if (!value) return;
    setDownloadBitrate(parseInt(value, 10));
  }

  function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
  }

  function getCacheUsagePercent(): number {
    if (!cacheSettings || cacheSettings.maxSizeBytes === 0) return 0;
    return Math.min(100, (cacheSettings.currentSizeBytes / cacheSettings.maxSizeBytes) * 100);
  }

  function handleCacheSizeLimitChange(value: string | undefined): void {
    if (!value) return;
    const newLimit = parseInt(value, 10);
    cacheSizeLimit = newLimit;
    setCacheSettings(newLimit).catch((e) => {
      console.error('Failed to save cache size limit:', e);
      toast.error('Could not save the cache size limit', {
        description: e instanceof Error ? e.message : String(e),
      });
    });
  }

  const cacheSizeLimitLabel = $derived(
    cacheSizeLimitOptions.find((o) => o.value === cacheSizeLimit)?.label ?? '5 GB'
  );

  async function handleClearCache(): Promise<void> {
    isClearingCache = true;
    try {
      await clearCache();
      const cache = await getCacheSettings();
      cacheSettings = cache;
      showClearConfirm = false;
    } catch (e) {
      console.error('Failed to clear cache:', e);
      toast.error('Could not clear the download cache', {
        description: e instanceof Error ? e.message : String(e),
      });
    } finally {
      isClearingCache = false;
    }
  }
</script>

<Panel
  title="Downloaded music cache"
  description="Manage offline storage for downloaded tracks."
  pad={false}
>
  <SettingsRow
    label="Cache usage"
    description={cacheSettings ? `${cacheSettings.cachedTrackCount} tracks downloaded` : 'Loading...'}
  >
    <span class="value">
      {cacheSettings ? formatBytes(cacheSettings.currentSizeBytes) : '...'}
      {#if cacheSettings && cacheSettings.maxSizeBytes > 0}
        / {formatBytes(cacheSettings.maxSizeBytes)}
      {/if}
    </span>
  </SettingsRow>

  {#if cacheSettings && cacheSettings.maxSizeBytes > 0}
    <div class="meter">
      <Progress value={getCacheUsagePercent()} aria-label="Cache usage" />
    </div>
  {/if}

  <SettingsRow
    label="Maximum cache size"
    description="Older downloads are automatically removed when the limit is reached"
    controlId="cache-limit"
  >
    <Select.Root type="single" value={String(cacheSizeLimit)} onValueChange={handleCacheSizeLimitChange}>
      <Select.Trigger id="cache-limit">{cacheSizeLimitLabel}</Select.Trigger>
      <Select.Content>
        {#each cacheSizeLimitOptions as option}
          <Select.Item value={String(option.value)}>{option.label}</Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
  </SettingsRow>

  <SettingsRow
    label="Download format"
    description="Original keeps the exact source file; other formats transcode on the server to save space. Applies to new downloads only."
    controlId="download-format"
  >
    <Select.Root type="single" value={downloadFormat} onValueChange={handleDownloadFormatChange}>
      <Select.Trigger id="download-format">{downloadFormatLabel}</Select.Trigger>
      <Select.Content>
        {#each downloadFormatOptions as option}
          <Select.Item value={option.value}>{option.label}</Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
  </SettingsRow>

  {#if isLossyFormat}
    <SettingsRow
      label="Download bitrate"
      description="Target bitrate for lossy transcodes"
      controlId="download-bitrate"
    >
      <Select.Root
        type="single"
        value={String(downloadBitrate)}
        onValueChange={handleDownloadBitrateChange}
      >
        <Select.Trigger id="download-bitrate">{downloadBitrateLabel}</Select.Trigger>
        <Select.Content>
          {#each downloadBitrateOptions as option}
            <Select.Item value={String(option.value)}>{option.label}</Select.Item>
          {/each}
        </Select.Content>
      </Select.Root>
    </SettingsRow>
  {/if}

  <SettingsRow label="Cache location">
    <span class="path">{appInfo?.dataDir ?? '...'}</span>
  </SettingsRow>

  <SettingsRow label="Clear cache" description="Remove all downloaded tracks to free up disk space">
    {#if showClearConfirm}
      <Button variant="destructive" size="sm" onclick={handleClearCache} disabled={isClearingCache}>
        {#if isClearingCache}
          <Spinner />
        {:else}
          <AlertTriangle size={14} />
        {/if}
        Confirm
      </Button>
      <Button
        variant="outline"
        size="sm"
        onclick={() => (showClearConfirm = false)}
        disabled={isClearingCache}
      >
        Cancel
      </Button>
    {:else}
      <Button
        variant="outline"
        size="sm"
        onclick={() => (showClearConfirm = true)}
        disabled={!cacheSettings || cacheSettings.currentSizeBytes === 0}
      >
        <Trash2 size={14} />
        Clear
      </Button>
    {/if}
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

  .meter {
    padding: 0 var(--space-lg) var(--space-md);
  }
</style>
