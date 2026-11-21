<script lang="ts">
  import type { Track } from '#lib/types/models.js';
  import type { ColumnDef } from './trackColumns';
  import { DownloadButton } from '#lib/components/downloads/index.js';
  import TrackContextMenu from './TrackContextMenu.svelte';
  import { getPlaybackState } from '#lib/stores/player.svelte.js';
  import { getOfflineMode } from '#lib/stores/offlineMode.svelte.js';
  import Pause from '@lucide/svelte/icons/pause';

  interface Props {
    track: Track;
    activeColumns: ColumnDef[];
    showAddToPlaylist: boolean;
    /** Odd position in the whole sorted list, not among the rows drawn. */
    striped: boolean;
    /** Multi-selected (the shared selection model in TrackList). */
    selected?: boolean;
    /** The keyboard cursor — the row `aria-activedescendant` points at. */
    active?: boolean;
    onClick?: (track: Track, event: MouseEvent) => void;
    onDoubleClick?: (track: Track) => void;
    onDragStart?: (event: DragEvent, track: Track) => void;
    onPlay?: (track: Track) => void;
    onQueueNext?: (tracks: Track[]) => void;
    onQueueEnd?: (tracks: Track[]) => void;
    onAddToPlaylist?: (tracks: Track[]) => void;
  }

  let {
    track,
    activeColumns,
    showAddToPlaylist,
    striped,
    selected = false,
    active = false,
    onClick,
    onDoubleClick,
    onDragStart,
    onPlay,
    onQueueNext,
    onQueueEnd,
    onAddToPlaylist,
  }: Props = $props();

  // Native right-click opens the exact same menu the "..." button does
  // (Spotify/Apple Music reference: both openers show one item set) — never a
  // second menu. A row outside the current selection is selected first, as a
  // plain click would; one already part of a multi-selection keeps it.
  let menuOpen = $state(false);

  function handleContextMenu(event: MouseEvent): void {
    if (unavailable) return;
    event.preventDefault();
    if (!selected) onClick?.(track, event);
    menuOpen = true;
  }

  function isTrackPlaying(t: Track): boolean {
    return getPlaybackState().currentTrack?.jellyfinId === t.jellyfinId && getPlaybackState().isPlaying;
  }

  function isTrackCurrent(t: Track): boolean {
    return getPlaybackState().currentTrack?.jellyfinId === t.jellyfinId;
  }

  /**
   * Offline Mode (the user's own toggle) with no local copy: Spotify's reference behaviour is "anything
   * you haven't downloaded is now grayed out and unavailable" — never
   * silently skipped, never clickable into a stream attempt that can only
   * fail once mpv tries to open a URL with no network to reach.
   */
  const unavailable = $derived(getOfflineMode() && !track.cachedFilePath);
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<tr
  id="track-row-{track.jellyfinId}"
  data-track-id={track.jellyfinId}
  class="track-row"
  class:striped
  class:selected
  class:active
  class:now-playing={isTrackCurrent(track)}
  class:unavailable
  aria-selected={selected}
  aria-disabled={unavailable}
  title={unavailable ? 'Not available offline' : undefined}
  tabindex="-1"
  draggable={!unavailable}
  onclick={(e) => !unavailable && onClick?.(track, e)}
  ondblclick={() => !unavailable && onDoubleClick?.(track)}
  ondragstart={(e) => !unavailable && onDragStart?.(e, track)}
  oncontextmenu={handleContextMenu}
>
  {#each activeColumns as col (col.id)}
    <td
      class="col-cell"
      class:align-right={col.align === 'right'}
      class:align-center={col.align === 'center'}
      class:col-title={col.id === 'title'}
      class:col-secondary={col.id !== 'title' && col.id !== 'time'}
      class:col-time={col.id === 'time'}
      class:col-numeric={col.id === 'trackNumber' || col.id === 'discNumber' || col.id === 'year'}
    >
      {#if col.id === 'title'}
        <span class="title-content">
          {#if isTrackPlaying(track)}
            <span class="playing-indicator" title="Now playing">
              <span class="eq-bar"></span>
              <span class="eq-bar"></span>
              <span class="eq-bar"></span>
            </span>
          {:else if isTrackCurrent(track)}
            <span class="paused-indicator">
              <Pause size={11} fill="currentColor" />
            </span>
          {/if}
          <span class="title-text">{col.render(track)}</span>
        </span>
      {:else if col.id === 'trackNumber' && track.trackNumber > 0}
        {track.trackNumber}
      {:else}
        {col.render(track)}
      {/if}
    </td>
  {/each}
  <td class="col-actions" class:is-downloaded={!!track.cachedFilePath}>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- Stop the click from bubbling to the row's own onclick, which would
         collapse a multi-selection to just this row before the download
         button or the "..." menu ever reads it (Add to Playlist must act on
         the whole selection, not whatever opening the menu left selected). -->
    <div class="actions-container" onclick={(e) => e.stopPropagation()}>
      {#if !unavailable}
        <DownloadButton
          size="sm"
          trackId={track.jellyfinId}
          trackName={track.name}
          artistName={track.artistName}
          isDownloaded={!!track.cachedFilePath}
        />
      {/if}
      <TrackContextMenu
        {track}
        {showAddToPlaylist}
        playbackDisabled={unavailable}
        bind:open={menuOpen}
        onPlay={() => onPlay?.(track)}
        onQueueNext={() => onQueueNext?.([track])}
        onQueueEnd={() => onQueueEnd?.([track])}
        onAddToPlaylist={() => onAddToPlaylist?.([track])}
      />
    </div>
  </td>
</tr>

<style>
  .track-row {
    cursor: pointer;
    background-color: var(--p11-bg-primary);
    transition: background-color var(--transition-fast);
  }

  /* Striped by position in the whole sorted list: the list is virtualised, so
     :nth-child would count only the rows drawn and flip them all as it scrolls. */
  .track-row.striped {
    background-color: var(--p11-bg-surface);
  }

  .track-row:hover {
    background-color: var(--p11-bg-hover);
  }

  /* Multi-selected (⌘/Shift-click, or an extended arrow-key range). */
  .track-row.selected {
    background-color: var(--p11-accent-subtle);
  }

  .track-row.selected:hover {
    background-color: var(--p11-accent-subtle);
  }

  /* The keyboard cursor — the table viewport (not this row) holds real DOM
     focus and points `aria-activedescendant` here, so this is what a focus
     ring would be on a per-row-focusable table. */
  .track-row.active {
    outline: 2px solid var(--p11-border-focus);
    outline-offset: -2px;
  }

  /* Offline with no local copy (Spotify reference: "grayed out and
     unavailable"). Never hidden — hiding would silently skip it. */
  .track-row.unavailable {
    cursor: not-allowed;
  }

  .track-row.unavailable .col-cell {
    opacity: 0.45;
  }

  .col-cell {
    padding: 6px var(--space-md);
    font-size: var(--font-size-md);
    color: var(--p11-text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .col-title {
    font-weight: var(--font-weight-medium);
  }

  .title-content {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    min-width: 0;
  }

  .title-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .col-secondary {
    color: var(--p11-text-secondary);
  }

  .col-time {
    color: var(--p11-text-muted);
    font-variant-numeric: tabular-nums;
    font-size: var(--font-size-sm);
  }

  .col-numeric {
    font-variant-numeric: tabular-nums;
  }

  .align-right {
    text-align: right;
  }

  .align-center {
    text-align: center;
  }

  /* Now-playing row highlight */
  .track-row.now-playing {
    background-color: var(--p11-now-playing);
  }

  .track-row.now-playing:hover {
    background-color: var(--p11-accent-subtle);
  }

  .track-row.now-playing .col-title {
    color: var(--p11-accent);
  }

  /* Equaliser bars animation */
  .playing-indicator {
    display: flex;
    align-items: flex-end;
    justify-content: center;
    gap: 2px;
    width: 16px;
    height: 14px;
    flex-shrink: 0;
  }

  .eq-bar {
    width: 2px;
    background-color: var(--p11-accent);
    border-radius: 1px;
    animation: eq-bounce 0.8s ease-in-out infinite;
  }

  .eq-bar:nth-child(1) {
    height: 6px;
    animation-delay: 0s;
  }

  .eq-bar:nth-child(2) {
    height: 10px;
    animation-delay: 0.2s;
  }

  .eq-bar:nth-child(3) {
    height: 5px;
    animation-delay: 0.4s;
  }

  @keyframes eq-bounce {
    0%,
    100% {
      transform: scaleY(0.4);
    }
    50% {
      transform: scaleY(1);
    }
  }

  /* Paused indicator (current track but not playing) */
  .paused-indicator {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 14px;
    flex-shrink: 0;
    color: var(--p11-accent);
  }

  .col-actions {
    width: 64px;
    text-align: center;
    padding: 0;
    opacity: 0;
    transition: opacity var(--transition-fast);
  }

  .col-actions.is-downloaded,
  .track-row:hover .col-actions,
  .track-row:focus-within .col-actions {
    opacity: 1;
  }

  .actions-container {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 2px;
  }
</style>
