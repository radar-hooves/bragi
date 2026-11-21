<script lang="ts">
  import { getUiState } from '#lib/stores/ui.svelte.js';

  export interface StatusBarData {
    /** Number of items (tracks, albums, artists, etc.) */
    itemCount: number;
    /** Label for items (e.g., "song", "album", "artist", "genre") */
    itemLabel: string;
    /** Total duration in milliseconds */
    durationMs: number;
    /** Total file size in bytes */
    sizeBytes: number;
  }

  interface Props {
    /** Structured data for the status bar */
    data?: StatusBarData | null;
  }

  let { data = null }: Props = $props();

  function formatTotalDuration(ms: number): string {
    const totalMinutes = Math.floor(ms / 60_000);
    if (totalMinutes < 60) return `${totalMinutes} minutes`;
    const hours = Math.floor(totalMinutes / 60);
    const minutes = totalMinutes % 60;
    if (minutes === 0) return `${hours} ${hours === 1 ? 'hour' : 'hours'}`;
    return `${hours}.${Math.round((minutes / 60) * 10)} hours`;
  }

  function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    const k = 1024;
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    const value = bytes / Math.pow(k, i);
    return `${value.toFixed(i > 0 ? 1 : 0)} ${units[i]}`;
  }

  function pluralise(count: number, singular: string): string {
    return count === 1 ? singular : `${singular}s`;
  }

  const statusText = $derived.by(() => {
    if (!data || data.itemCount === 0) return null;
    const parts = [
      `${data.itemCount} ${pluralise(data.itemCount, data.itemLabel)}`,
      formatTotalDuration(data.durationMs),
      formatBytes(data.sizeBytes),
    ];
    return parts.join(', ');
  });
</script>

{#if getUiState().statusBarVisible && statusText}
  <footer class="status-bar">
    {statusText}
  </footer>
{/if}

<style>
  .status-bar {
    flex-shrink: 0;
    padding: var(--space-sm) var(--space-md);
    border-top: 1px solid rgb(var(--p11-overlay-rgb) / 0.06);
    font-size: var(--font-size-xs);
    color: var(--p11-text-muted);
    text-align: center;
    background-color: rgb(var(--p11-shade-rgb) / 0.15);
  }
</style>
