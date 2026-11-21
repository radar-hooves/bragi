<script lang="ts">
  import { playTrackFromList } from '#lib/utils/playback.js';
  import {
    getRecentlyPlayedTracks,
    getRecentlyPlayedState,
    clearRecentlyPlayed,
  } from '#lib/stores/recentlyPlayed.svelte.js';
  import { getTrackListColumns, getTrackListColumnWidths, setTrackListColumns, setTrackListColumnWidths } from '#lib/stores/ui.svelte.js';
  import type { Track } from '#lib/types/models.js';
  import { Spinner, StatusBar, type StatusBarData } from '#lib/components/common/index.js';
  import TrackList from './TrackList.svelte';
  import Trash2 from '@lucide/svelte/icons/trash-2';

  let showClearConfirm = $state(false);

  // This view's own column layout: Last Played is on by default (see
  // the view's own default in ui.svelte.ts), and — unlike the old shared layout — the
  // user can now actually hide it here without it also disappearing from, or
  // leaking into, Songs.
  const recentlyPlayedColumns = $derived(getTrackListColumns('recentlyPlayed'));
  const recentlyPlayedColumnWidths = $derived(getTrackListColumnWidths('recentlyPlayed'));
  function handleColumnsChange(cols: string[]): void {
    setTrackListColumns('recentlyPlayed', cols);
  }
  function handleColumnWidthsChange(widths: Record<string, number>): void {
    setTrackListColumnWidths('recentlyPlayed', widths);
  }

  function handleTrackClick(_track: Track): void {
    // Single click selects - TrackList handles visual selection
  }

  async function handleTrackPlay(track: Track): Promise<void> {
    try {
      await playTrackFromList(track, getRecentlyPlayedTracks());
    } catch (e) {
      console.error('Failed to play track:', e);
    }
  }

  function handleClearClick(): void {
    showClearConfirm = true;
  }

  function handleConfirmClear(): void {
    clearRecentlyPlayed();
    showClearConfirm = false;
  }

  function handleCancelClear(): void {
    showClearConfirm = false;
  }

  const statusData = $derived.by((): StatusBarData | null => {
    const tracks = getRecentlyPlayedTracks();
    if (tracks.length === 0) return null;
    return {
      itemCount: tracks.length,
      itemLabel: 'song',
      durationMs: tracks.reduce((sum, t) => sum + t.durationMs, 0),
      sizeBytes: tracks.reduce((sum, t) => sum + (t.fileSizeBytes ?? 0), 0),
    };
  });
</script>

<div class="recently-played-view">
  {#if !getRecentlyPlayedState().isLoaded}
    <div class="loading-state">
      <Spinner size={32} />
      <p>Loading recently played...</p>
    </div>
  {:else if getRecentlyPlayedTracks().length === 0}
    <div class="empty-state">
      <p class="empty-title">No recently played tracks</p>
      <p class="empty-description">
        Tracks you play will appear here. Your listening history is stored locally.
      </p>
    </div>
  {:else}
    <div class="content">
      <div class="header">
        <span class="subtitle">
          {getRecentlyPlayedTracks().length}
          {getRecentlyPlayedTracks().length === 1 ? 'track' : 'tracks'}
        </span>
        <button class="clear-btn" onclick={handleClearClick} title="Clear recently played">
          <Trash2 size={14} />
          Clear
        </button>
      </div>
      <div class="list-container">
        <TrackList
          tracks={getRecentlyPlayedTracks()}
          visibleColumns={recentlyPlayedColumns}
          columnWidths={recentlyPlayedColumnWidths}
          viewKey="recently-played"
          onTrackClick={handleTrackClick}
          onTrackPlay={handleTrackPlay}
          onColumnsChange={handleColumnsChange}
          onColumnWidthsChange={handleColumnWidthsChange}
        />
      </div>
    </div>
  {/if}

  <StatusBar data={statusData} />
</div>

{#if showClearConfirm}
  <div class="confirm-overlay" onclick={handleCancelClear} role="presentation">
    <div
      class="confirm-dialog"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => {
        if (e.key === 'Escape') handleCancelClear();
      }}
      role="dialog"
      tabindex="-1"
    >
      <p class="confirm-title">Clear recently played?</p>
      <p class="confirm-description">This will remove all tracks from your recently played list.</p>
      <div class="confirm-actions">
        <button class="confirm-btn cancel" onclick={handleCancelClear}>Cancel</button>
        <button class="confirm-btn destructive" onclick={handleConfirmClear}>Clear</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .recently-played-view {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .content {
    /* TrackList owns its own scroll and virtualisation — this must never also
       scroll, or the two nested overflow containers fight over the wheel and
       TrackList's viewport never gets a bounded height to window against. */
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 var(--space-lg) var(--space-md);
  }

  .subtitle {
    font-size: var(--font-size-sm);
    color: var(--p11-text-muted);
  }

  .clear-btn {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
    padding: var(--space-xs) var(--space-sm);
    background: transparent;
    border: 1px solid rgb(var(--p11-overlay-rgb) / 0.1);
    border-radius: var(--p11-radius-sm);
    color: var(--p11-text-muted);
    font-size: var(--font-size-xs);
    font-family: var(--font-family);
    cursor: pointer;
    transition:
      background-color var(--transition-fast),
      color var(--transition-fast),
      border-color var(--transition-fast);
  }

  .clear-btn:hover {
    background-color: rgb(var(--p11-overlay-rgb) / 0.05);
    color: var(--p11-text-secondary);
    border-color: rgb(var(--p11-overlay-rgb) / 0.15);
  }

  .list-container {
    flex: 1;
    min-height: 0;
  }

  .loading-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-md);
    padding: var(--space-3xl);
    color: var(--p11-text-muted);
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: var(--space-3xl) var(--space-xl);
    text-align: center;
  }

  .empty-title {
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-medium);
    color: var(--p11-text-primary);
    margin: 0 0 var(--space-sm);
  }

  .empty-description {
    font-size: var(--font-size-md);
    color: var(--p11-text-secondary);
    margin: 0;
    max-width: 400px;
  }

  /* Confirmation dialog */
  .confirm-overlay {
    position: fixed;
    inset: 0;
    background-color: rgb(var(--p11-shade-rgb) / 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .confirm-dialog {
    background-color: var(--p11-surface);
    border: 1px solid rgb(var(--p11-overlay-rgb) / 0.1);
    border-radius: var(--p11-radius-lg);
    padding: var(--space-xl);
    max-width: 360px;
    box-shadow: 0 8px 32px rgb(var(--p11-shade-rgb) / 0.4);
  }

  .confirm-title {
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-semibold);
    color: var(--p11-text-primary);
    margin: 0 0 var(--space-sm);
  }

  .confirm-description {
    font-size: var(--font-size-md);
    color: var(--p11-text-secondary);
    margin: 0 0 var(--space-lg);
  }

  .confirm-actions {
    display: flex;
    gap: var(--space-sm);
    justify-content: flex-end;
  }

  .confirm-btn {
    padding: var(--space-sm) var(--space-lg);
    border-radius: var(--p11-radius-md);
    font-size: var(--font-size-sm);
    font-family: var(--font-family);
    font-weight: var(--font-weight-medium);
    cursor: pointer;
    transition: background-color var(--transition-fast);
  }

  .confirm-btn.cancel {
    background: rgb(var(--p11-overlay-rgb) / 0.08);
    border: 1px solid rgb(var(--p11-overlay-rgb) / 0.1);
    color: var(--p11-text-primary);
  }

  .confirm-btn.cancel:hover {
    background: rgb(var(--p11-overlay-rgb) / 0.12);
  }

  .confirm-btn.destructive {
    background-color: var(--p11-error);
    border: none;
    color: rgb(var(--p11-overlay-rgb));
  }

  .confirm-btn.destructive:hover {
    /* Darkened from the error token rather than a second red of its own. */
    background-color: color-mix(in oklab, var(--p11-error), rgb(var(--p11-shade-rgb)) 20%);
  }
</style>
