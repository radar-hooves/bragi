/**
 * Downloads store for managing offline download state.
 */

import type { CacheSettings, DownloadState, QueuedDownload, DownloadProgressEvent, DownloadCompletedEvent, DownloadFailedEvent } from '#lib/types/tauri.js';
import { markTrackDownloaded, markTrackRemoved } from '#lib/stores/library.svelte.js';
import * as tauri from '#lib/services/tauri.js';

/** Current download queue */
let downloadQueue = $state<QueuedDownload[]>([]);

/** Current download manager state */
let downloadState = $state<DownloadState>('idle');

/** Cache settings */
let cacheSettings = $state<CacheSettings>({
  maxSizeBytes: 10 * 1024 * 1024 * 1024, // 10GB default
  currentSizeBytes: 0,
  cachedTrackCount: 0,
});

/** Get the download queue */
export function getDownloadQueue(): QueuedDownload[] {
  return downloadQueue;
}

/** Get the download state */
export function getDownloadState(): DownloadState {
  return downloadState;
}

/** Get cache settings */
export function getCacheSettings(): CacheSettings {
  return cacheSettings;
}

/** Number of pending downloads */
export function getPendingDownloadCount(): number {
  return downloadQueue.filter((d) => d.status === 'pending' || d.status === 'downloading').length;
}

/** Number of completed downloads */
export function getCompletedDownloadCount(): number {
  return downloadQueue.filter((d) => d.status === 'completed').length;
}

/** Number of failed downloads */
export function getFailedDownloadCount(): number {
  return downloadQueue.filter((d) => d.status === 'failed').length;
}

/** Active download (currently downloading) */
export function getActiveDownload(): QueuedDownload | undefined {
  return downloadQueue.find((d) => d.status === 'downloading');
}

/** Cache usage percentage */
export function getCacheUsagePercent(): number {
  if (cacheSettings.maxSizeBytes === 0) return 0;
  return Math.round((cacheSettings.currentSizeBytes / cacheSettings.maxSizeBytes) * 100);
}

/** Refresh download queue from backend */
export async function refreshDownloadQueue(): Promise<void> {
  try {
    const queue = await tauri.getDownloadQueue();
    downloadQueue = queue;
  } catch (e) {
    console.error('Failed to refresh download queue:', e);
  }
}

/** Refresh download state from backend */
export async function refreshDownloadState(): Promise<void> {
  try {
    const state = await tauri.getDownloadState();
    downloadState = state;
  } catch (e) {
    console.error('Failed to refresh download state:', e);
  }
}

/** Refresh cache settings from backend */
export async function refreshCacheSettings(): Promise<void> {
  try {
    const settings = await tauri.getCacheSettings();
    cacheSettings = settings;
  } catch (e) {
    console.error('Failed to refresh cache settings:', e);
  }
}

/** Queue a single track for download */
export async function queueTrackDownload(trackId: string, trackName: string, artistName: string): Promise<void> {
  await tauri.downloadTrack(trackId, trackName, artistName);
  await refreshDownloadQueue();
}

/** Queue all tracks in an album for download */
export async function queueAlbumDownload(albumId: string): Promise<number> {
  const count = await tauri.downloadAlbum(albumId);
  await refreshDownloadQueue();
  return count;
}

/** Cancel a download */
export async function cancelDownload(trackId: string): Promise<void> {
  await tauri.cancelDownload(trackId);
  await refreshDownloadQueue();
}

/** Pause all downloads */
export async function pauseAllDownloads(): Promise<void> {
  await tauri.pauseDownloads();
  await refreshDownloadState();
}

/** Resume downloads */
export async function resumeAllDownloads(): Promise<void> {
  await tauri.resumeDownloads();
  await refreshDownloadState();
}

/** Clear completed and failed downloads from queue */
export async function clearCompletedDownloads(): Promise<void> {
  await tauri.clearCompletedDownloads();
  await refreshDownloadQueue();
}

/** Update cache size limit */
export async function updateCacheSizeLimit(maxSizeBytes: number): Promise<void> {
  await tauri.setCacheSettings(maxSizeBytes);
  await refreshCacheSettings();
}

/**
 * Clear all cached files.
 *
 * Badge clearing is left to the `download-cache-evicted` event, which names
 * exactly the cleared tracks: a currently-playing (protected) track survives
 * a "Clear cache" with its file intact, so its badge must survive too.
 */
export async function clearAllCache(): Promise<void> {
  await tauri.clearCache();
  await refreshCacheSettings();
}

/** Remove a single downloaded track */
export async function removeDownloadedTrack(trackId: string): Promise<void> {
  await tauri.removeDownloadedTrack(trackId);
  markTrackRemoved(trackId);
  await refreshCacheSettings();
}

/** Handle download progress event */
export function handleDownloadProgress(event: DownloadProgressEvent): void {
  const item = downloadQueue.find((q) => q.trackId === event.trackId);
  if (!item) return;
  item.status = 'downloading';
  item.progressPercent = event.progressPercent;
  item.bytesDownloaded = event.bytesDownloaded;
  item.totalBytes = event.totalBytes;
}

/** Handle download completed event */
export function handleDownloadCompleted(event: DownloadCompletedEvent): void {
  const item = downloadQueue.find((q) => q.trackId === event.trackId);
  if (item) {
    item.status = 'completed';
    item.progressPercent = 100;
    item.bytesDownloaded = event.fileSizeBytes;
    item.totalBytes = event.fileSizeBytes;
  }

  // Update library state so album/track indicators refresh immediately
  markTrackDownloaded(event.trackId, event.filePath);

  // Refresh cache settings to update size
  refreshCacheSettings();
}

/** Handle download failed event */
export function handleDownloadFailed(event: DownloadFailedEvent): void {
  const item = downloadQueue.find((q) => q.trackId === event.trackId);
  if (!item) return;
  item.status = 'failed';
  item.error = event.error;
}

/** Handle download cancelled event */
export function handleDownloadCancelled(trackId: string): void {
  const item = downloadQueue.find((q) => q.trackId === trackId);
  if (!item) return;
  item.status = 'cancelled';
}

/** Handle queue changed event */
export function handleQueueChanged(): void {
  refreshDownloadQueue();
}

/** Handle state changed event */
export function handleStateChanged(state: DownloadState): void {
  downloadState = state;
}

/**
 * Handle cache evicted event (fired by "Clear all cache").
 *
 * Clears only the tracks the backend actually cleared: a protected
 * (currently-playing) track survives the clear, so its badge must too.
 * `evictedTrackIds` names exactly what changed, mirroring the same per-track
 * path `markTrackDownloaded` uses on a download's completion.
 */
export function handleCacheEvicted(evictedTrackIds: string[]): void {
  for (const trackId of evictedTrackIds) {
    markTrackRemoved(trackId);
  }
  refreshCacheSettings();
}

/** Format bytes as human-readable string */
export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}
