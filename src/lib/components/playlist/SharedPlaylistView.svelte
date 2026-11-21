<script lang="ts">
  import { getImportedPlaylistState, deleteImportedPlaylist, clearCurrentImportedPlaylist } from '#lib/stores/importedPlaylists.svelte.js';
  import type { ImportedPlaylistTrack } from '#lib/types/models.js';
  import { DownloadButton } from '#lib/components/downloads/index.js';
  import { Spinner, StatusBar, type StatusBarData, EmptyState } from '#lib/components/common/index.js';
  import Play from '@lucide/svelte/icons/play';
  import Shuffle from '@lucide/svelte/icons/shuffle';
  import ListMusic from '@lucide/svelte/icons/list-music';
  import { createVirtualList } from '#lib/utils/virtualList.svelte.js';
  import * as Dialog from '@poodle64/ui/dialog';

  interface Props {
    onTrackPlay?: (track: ImportedPlaylistTrack) => void;
    onShuffleAll?: () => void;
  }

  let { onTrackPlay, onShuffleAll }: Props = $props();

  let showDeleteConfirm = $state(false);
  let isDeleting = $state(false);

  const playlist = $derived(getImportedPlaylistState().currentPlaylist);
  const trackCount = $derived(playlist?.tracks.length ?? 0);
  const resolvedCount = $derived(playlist?.tracks.filter((t) => t.resolvedJellyfinId).length ?? 0);

  // Routes through the shared virtualiser — an imported playlist can
  // match the size of its source playlist, hundreds of rows or more.
  let viewportEl = $state<HTMLDivElement>();
  let innerEl = $state<HTMLDivElement>();

  const virtual = createVirtualList({
    count: () => trackCount,
    estimateRowHeight: () => 40,
    overscan: 8,
  });

  let visibleTracks = $derived(playlist ? playlist.tracks.slice(virtual.start, virtual.end) : []);

  $effect(() => virtual.attachViewport(viewportEl));
  $effect(() =>
    virtual.bindScrollMemory(playlist ? `shared-playlist:${playlist.id}` : 'shared-playlist', viewportEl)
  );

  $effect(() => {
    trackCount;
    virtual.refresh();
  });

  $effect(() => {
    void virtual.windowing;
    void virtual.end;
    virtual.remeasure(innerEl);
  });

  function formatDuration(ms: number): string {
    const seconds = Math.floor(ms / 1000);
    const minutes = Math.floor(seconds / 60);
    const remainingSeconds = seconds % 60;
    return `${minutes}:${remainingSeconds.toString().padStart(2, '0')}`;
  }

  function handlePlayAll(): void {
    const first = playlist?.tracks.find((t) => t.resolvedJellyfinId);
    if (first) onTrackPlay?.(first);
  }

  function handleTrackClick(track: ImportedPlaylistTrack): void {
    if (!track.resolvedJellyfinId) return;
    onTrackPlay?.(track);
  }

  function handleDeleteClick(): void {
    showDeleteConfirm = true;
  }

  async function confirmDelete(): Promise<void> {
    if (!playlist) return;
    isDeleting = true;
    const success = await deleteImportedPlaylist(playlist.id);
    isDeleting = false;
    showDeleteConfirm = false;
    if (success) clearCurrentImportedPlaylist();
  }

  const statusData = $derived.by((): StatusBarData | null => {
    if (!playlist || trackCount === 0) return null;
    let totalMs = 0;
    for (const track of playlist.tracks) totalMs += track.durationMs ?? 0;
    return {
      itemCount: trackCount,
      itemLabel: 'song',
      durationMs: totalMs,
      sizeBytes: 0,
    };
  });
</script>

<div class="shared-playlist-view">
  {#if !playlist}
    <EmptyState title="Select a shared playlist" description="Choose an imported playlist from the sidebar." />
  {:else}
    <header class="playlist-header">
      <div class="playlist-info">
        <div class="playlist-artwork">
          <ListMusic size={48} strokeWidth={1.5} />
        </div>
        <div class="playlist-meta">
          <h1 class="playlist-name">{playlist.name}</h1>
          <div class="playlist-stats">
            <span>{resolvedCount} of {trackCount} {trackCount === 1 ? 'track' : 'tracks'} in your library</span>
          </div>
          <p class="readonly-note">Imported playlist &mdash; read-only, never written to your Jellyfin.</p>
        </div>
      </div>

      <div class="playlist-actions">
        <button class="action-btn primary" onclick={handlePlayAll} disabled={resolvedCount === 0} title="Play all available tracks">
          <Play size={18} fill="currentColor" />
          <span>Play All</span>
        </button>
        <button class="action-btn" onclick={() => onShuffleAll?.()} disabled={resolvedCount === 0} title="Shuffle available tracks">
          <Shuffle size={18} />
          <span>Shuffle</span>
        </button>
        <button class="action-btn danger" onclick={handleDeleteClick}>Remove</button>
      </div>
    </header>

    <div class="track-header">
      <span class="col-number">#</span>
      <span class="col-title">Title</span>
      <span class="col-artist">Artist</span>
      <span class="col-album">Album</span>
      <span class="col-duration">Duration</span>
      <span class="col-status"></span>
    </div>
    <div class="track-list" role="list" bind:this={viewportEl} onscroll={virtual.handleScroll}>
      <div class="track-list-spacer" style:height={virtual.windowing ? `${virtual.totalSize}px` : undefined}>
        <div
          class="track-list-window"
          class:is-windowed={virtual.windowing}
          bind:this={innerEl}
          style:transform={virtual.windowing ? `translateY(${virtual.padStart}px)` : undefined}
        >
          {#each visibleTracks as track, i (virtual.start + i)}
            {@const index = virtual.start + i}
            <div class="track-row" class:unavailable={!track.resolvedJellyfinId} role="listitem">
              <span class="col-number">{index + 1}</span>
              {#if track.resolvedJellyfinId}
                <button class="col-title track-title-btn" onclick={() => handleTrackClick(track)}>
                  {track.title}
                </button>
              {:else}
                <span class="col-title track-title-static" title="Not in your library">{track.title}</span>
              {/if}
              <span class="col-artist">{track.artistName}</span>
              <span class="col-album">{track.albumName ?? ''}</span>
              <span class="col-duration">{track.durationMs ? formatDuration(track.durationMs) : ''}</span>
              <span class="col-status">
                {#if track.resolvedJellyfinId}
                  <DownloadButton
                    size="sm"
                    trackId={track.resolvedJellyfinId}
                    trackName={track.title}
                    artistName={track.artistName}
                    isDownloaded={!!track.cachedFilePath}
                  />
                {:else}
                  <span class="unavailable-badge">Not in your library</span>
                {/if}
              </span>
            </div>
          {/each}
        </div>
      </div>
    </div>

    <StatusBar data={statusData} />
  {/if}
</div>

<Dialog.Root bind:open={showDeleteConfirm}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Remove this import?</Dialog.Title>
      <Dialog.Description>
        {#if playlist}“{playlist.name}” will be removed from Shared playlists. Your Jellyfin library is unaffected.{/if}
      </Dialog.Description>
    </Dialog.Header>
    <div class="delete-confirm">
      <button class="cancel-btn" onclick={() => (showDeleteConfirm = false)} disabled={isDeleting}>Cancel</button>
      <button class="confirm-delete-btn" onclick={() => void confirmDelete()} disabled={isDeleting}>
        {#if isDeleting}
          <Spinner size={14} />
        {:else}
          Remove
        {/if}
      </button>
    </div>
  </Dialog.Content>
</Dialog.Root>

<style>
  .shared-playlist-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    padding: 0 var(--space-lg);
  }

  .playlist-header {
    display: flex;
    gap: var(--space-xl);
    padding-bottom: var(--space-xl);
    border-bottom: 1px solid var(--p11-border);
    margin-bottom: var(--space-lg);
    flex-wrap: wrap;
  }

  .playlist-info {
    display: flex;
    gap: var(--space-lg);
    flex: 1;
    min-width: 300px;
  }

  .playlist-artwork {
    width: 160px;
    height: 160px;
    background-color: var(--p11-bg-elevated);
    border-radius: var(--p11-radius-lg);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    color: var(--p11-text-muted);
  }

  .playlist-meta {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: var(--space-sm);
  }

  .playlist-name {
    font-size: var(--font-size-2xl);
    font-weight: var(--font-weight-bold);
    margin: 0;
    color: var(--p11-text-primary);
  }

  .playlist-stats {
    display: flex;
    gap: var(--space-sm);
    font-size: var(--font-size-sm);
    color: var(--p11-text-muted);
  }

  .readonly-note {
    margin: 0;
    font-size: var(--font-size-xs);
    color: var(--p11-text-muted);
  }

  .playlist-actions {
    display: flex;
    gap: var(--space-sm);
    align-items: flex-start;
    flex-wrap: wrap;
  }

  .action-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 10px var(--space-lg);
    background: var(--p11-bg-elevated);
    border: none;
    border-radius: var(--radius-full);
    color: var(--p11-text-primary);
    font-size: var(--font-size-sm);
    font-family: var(--font-family);
    font-weight: var(--font-weight-medium);
    cursor: pointer;
    transition:
      background-color var(--transition-fast),
      transform var(--transition-spring);
  }

  .action-btn:hover:not(:disabled) {
    background: var(--p11-bg-surface);
  }

  .action-btn:active:not(:disabled) {
    transform: scale(0.97);
  }

  .action-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .action-btn.primary {
    background: var(--p11-accent);
    color: rgb(var(--p11-overlay-rgb));
  }

  .action-btn.primary:hover:not(:disabled) {
    background: var(--p11-accent-hover);
  }

  .action-btn.danger {
    color: var(--p11-error);
  }

  .action-btn.danger:hover:not(:disabled) {
    background: var(--p11-error-bg);
  }

  .delete-confirm {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-xs) var(--space-sm);
    background-color: var(--p11-error-bg);
    border-radius: var(--p11-radius-md);
    font-size: var(--font-size-sm);
    color: var(--p11-error);
  }

  .confirm-delete-btn {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
    padding: var(--space-xs) var(--space-sm);
    background-color: var(--p11-error);
    border: none;
    border-radius: var(--p11-radius-sm);
    color: rgb(var(--p11-overlay-rgb));
    font-size: var(--font-size-sm);
    cursor: pointer;
  }

  .confirm-delete-btn:disabled {
    opacity: 0.5;
  }

  .cancel-btn {
    padding: var(--space-xs) var(--space-sm);
    background: transparent;
    border: 1px solid var(--p11-border);
    border-radius: var(--p11-radius-sm);
    color: var(--p11-text-primary);
    font-size: var(--font-size-sm);
    cursor: pointer;
  }

  .track-list {
    flex: 1;
    overflow-y: auto;
    overscroll-behavior: contain;
    position: relative;
  }

  .track-list-spacer {
    position: relative;
    width: 100%;
  }

  .track-list-window.is-windowed {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
  }

  .track-header,
  .track-row {
    display: grid;
    grid-template-columns: 32px 1fr 1fr 1fr 80px 130px;
    gap: var(--space-md);
    padding: var(--space-sm) var(--space-md);
    align-items: center;
  }

  .track-header {
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
    color: var(--p11-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    border-bottom: 1px solid var(--p11-border);
    background-color: var(--p11-bg);
    flex-shrink: 0;
  }

  .track-row {
    font-size: var(--font-size-sm);
    border-radius: var(--p11-radius-sm);
    transition: background-color var(--transition-fast);
  }

  .track-row:hover {
    background-color: var(--p11-bg-elevated);
  }

  .track-row.unavailable {
    opacity: 0.5;
  }

  .col-number {
    color: var(--p11-text-muted);
    text-align: center;
  }

  .col-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-title-btn {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: var(--p11-text-primary);
    text-align: left;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-title-btn:hover {
    color: var(--p11-accent);
    text-decoration: underline;
  }

  .track-title-static {
    color: var(--p11-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .col-artist,
  .col-album {
    color: var(--p11-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .col-duration {
    color: var(--p11-text-muted);
    text-align: right;
  }

  .col-status {
    display: flex;
    align-items: center;
    justify-content: flex-end;
  }

  .unavailable-badge {
    font-size: var(--font-size-xs);
    color: var(--p11-text-muted);
    white-space: nowrap;
  }
</style>
