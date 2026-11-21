<script lang="ts">
  import {
    getPlaybackState,
    getProgress,
    getCurrentTimeFormatted,
    getRemainingTimeFormatted,
  } from '#lib/stores/player.svelte.js';
  import { cachedArtworkUrl, handleArtworkError, resetArtwork } from '#lib/utils/artworkFallback.js';
  import { openFullScreenPlayer } from '#lib/stores/fullscreenPlayer.svelte.js';
  import { seek as tauriSeek, resume as tauriResume } from '#lib/services/tauri.js';
  import { reportTransportError } from '#lib/utils/transportError.js';
  import { queueTrackNext, queueTrackAtEnd } from '#lib/utils/queueActions.js';
  import TrackContextMenu from '#lib/components/library/TrackContextMenu.svelte';
  import Shuffle from '@lucide/svelte/icons/shuffle';
  import Repeat from '@lucide/svelte/icons/repeat';
  import Repeat1 from '@lucide/svelte/icons/repeat-1';
  import Maximize2 from '@lucide/svelte/icons/maximize-2';

  interface Props {
    onShuffle: () => void;
    onRepeat: () => void;
    onSeek: (event: Event) => void;
  }

  let { onShuffle, onRepeat, onSeek }: Props = $props();

  let menuOpen = $state(false);

  // Restarts the current track in place — never a new playContext call, which
  // would rebuild the queue around it (one PlayContext call per
  // list; the now-playing bar isn't a list).
  async function handleReplay(): Promise<void> {
    try {
      await tauriSeek(0);
      if (!getPlaybackState().isPlaying) await tauriResume();
    } catch (err) {
      reportTransportError('Play', err);
    }
  }

  // AddToJellyfinPlaylistDialog's state lives in +page.svelte (a route), out
  // of prop reach from here (PlayerBar mounts in the root +layout.svelte) —
  // the same window-event bridge PlaylistList already uses for
  // 'bragi-playlists-warmed'.
  function handleAddToPlaylist(): void {
    const track = getPlaybackState().currentTrack;
    if (track) window.dispatchEvent(new CustomEvent('bragi-add-to-playlist', { detail: [track] }));
  }
</script>

<div class="lcd-panel" class:idle={!getPlaybackState().currentTrack}>
  {#if getPlaybackState().currentTrack}
    {@const track = getPlaybackState().currentTrack!}
    <div class="lcd-artwork-wrap">
      <button
        class="lcd-artwork"
        onclick={openFullScreenPlayer}
        oncontextmenu={(e) => { e.preventDefault(); menuOpen = true; }}
        aria-label="Open full-screen player"
        title="Full-screen player"
      >
        {#if track.albumJellyfinId}
          <img
            src={cachedArtworkUrl(track.albumJellyfinId)}
            use:resetArtwork={cachedArtworkUrl(track.albumJellyfinId)}
            alt="Album artwork"
            class="lcd-artwork-img"
            onerror={(e) => handleArtworkError(e, track.artworkUrl)}
          />
          <img src="/bragi-rune.png" alt="Bragi" class="lcd-artwork-img lcd-artwork-icon" style="display:none" />
        {:else}
          <img src="/bragi-rune.png" alt="Bragi" class="lcd-artwork-img lcd-artwork-icon" />
        {/if}
        <span class="lcd-artwork-overlay"><Maximize2 size={18} /></span>
      </button>
      <span class="lcd-menu">
        <TrackContextMenu
          {track}
          showAddToPlaylist={true}
          bind:open={menuOpen}
          onPlay={handleReplay}
          onQueueNext={() => queueTrackNext([track])}
          onQueueEnd={() => queueTrackAtEnd([track])}
          onAddToPlaylist={handleAddToPlaylist}
        />
      </span>
    </div>
    <div class="lcd-content">
      <!-- Main area: 3-column grid with track info in centre -->
      <div class="lcd-main">
        <!-- Left: Shuffle + elapsed time -->
        <div class="lcd-time-group">
          <button
            class="mode-btn"
            class:active={getPlaybackState().shuffle}
            onclick={onShuffle}
            aria-label="Shuffle"
            title="Shuffle"
          >
            <Shuffle size={18} />
          </button>
          <span class="lcd-time">{getCurrentTimeFormatted()}</span>
        </div>
        <!-- Centre: Track info -->
        <div class="lcd-track-info">
          <span class="lcd-track-name">{track.name}</span>
          <span class="lcd-track-artist">
            {track.artistName}{#if track.albumName}&ensp;&mdash;&ensp;{track.albumName}{/if}
          </span>
        </div>
        <!-- Right: Repeat + remaining time -->
        <div class="lcd-time-group">
          <button
            class="mode-btn"
            class:active={getPlaybackState().repeat !== 'off'}
            onclick={onRepeat}
            aria-label={getPlaybackState().repeat === 'one'
              ? 'Repeat one'
              : getPlaybackState().repeat === 'all'
                ? 'Repeat all'
                : 'Repeat'}
            title={getPlaybackState().repeat === 'one'
              ? 'Repeat one'
              : getPlaybackState().repeat === 'all'
                ? 'Repeat all'
                : 'Repeat off'}
          >
            {#if getPlaybackState().repeat === 'one'}
              <Repeat1 size={18} />
            {:else}
              <Repeat size={18} />
            {/if}
          </button>
          <span class="lcd-time">-{getRemainingTimeFormatted()}</span>
        </div>
      </div>
      <!-- Bottom: Progress bar -->
      <input
        type="range"
        class="lcd-progress"
        min="0"
        max="100"
        value={getProgress()}
        style="--progress: {getProgress()}%"
        oninput={onSeek}
        aria-label="Seek"
      />
    </div>
  {:else}
    <div class="lcd-idle">
      <img src="/bragi-rune.png" alt="Bragi" class="lcd-idle-icon" />
    </div>
  {/if}
</div>

<style>
  /* ── LCD display panel ───────────────────────────────────────── */
  /* The top bar's middle column, which sets its width (PlayerBar). */
  .lcd-panel {
    container-type: inline-size;
    align-self: stretch;
    display: flex;
    align-items: stretch;
    background-color: var(--p11-bg-elevated);
    border-left: 1px solid var(--p11-border);
    border-right: 1px solid var(--p11-border);
  }

  .lcd-artwork-wrap {
    position: relative;
    flex-shrink: 0;
    height: 100%;
  }

  .lcd-artwork {
    position: relative;
    width: var(--header-height);
    height: 100%;
    flex-shrink: 0;
    padding: 0;
    border: none;
    background: transparent;
    cursor: pointer;
    display: block;
  }

  .lcd-menu {
    position: absolute;
    top: 2px;
    right: 2px;
    opacity: 0;
    transition: opacity var(--transition-fast);
  }

  .lcd-artwork-wrap:hover .lcd-menu,
  .lcd-artwork-wrap:focus-within .lcd-menu {
    opacity: 1;
  }

  .lcd-menu :global(.track-menu-trigger) {
    width: 22px;
    height: 22px;
    background-color: rgb(var(--p11-shade-rgb) / 0.55);
    color: rgb(var(--p11-overlay-rgb));
  }

  .lcd-menu :global(.track-menu-trigger:hover) {
    background-color: rgb(var(--p11-shade-rgb) / 0.8);
    color: rgb(var(--p11-overlay-rgb));
  }

  /* Maximise overlay revealed on hover — click opens the full-screen player. */
  .lcd-artwork-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgb(var(--p11-shade-rgb) / 0.45);
    color: rgb(var(--p11-overlay-rgb));
    opacity: 0;
    transition: opacity var(--transition-fast);
    pointer-events: none;
  }

  .lcd-artwork:hover .lcd-artwork-overlay,
  .lcd-artwork:focus-visible .lcd-artwork-overlay {
    opacity: 1;
  }

  .lcd-artwork-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .lcd-artwork-img.lcd-artwork-icon {
    object-fit: contain;
    padding: 12px;
    background-color: var(--p11-bg-surface);
  }

  /* LCD content: main area + progress bar at bottom */
  .lcd-content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  /* Main area: 3-column grid */
  .lcd-main {
    flex: 1;
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    padding: 0 var(--space-md);
    gap: var(--space-md);
  }

  /* Left and right: icon + time stacked */
  .lcd-time-group {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
  }

  .lcd-time {
    font-size: var(--font-size-xs);
    color: var(--p11-text-muted);
    font-variant-numeric: tabular-nums;
    line-height: 1;
  }

  .mode-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 20px;
    padding: 0;
    background: transparent;
    border: none;
    color: var(--p11-text-muted);
    cursor: pointer;
    transition: color var(--transition-fast);
  }

  .mode-btn:hover {
    color: var(--p11-text-secondary);
  }

  .mode-btn.active {
    color: var(--p11-accent);
  }

  .mode-btn.active:hover {
    color: var(--p11-accent-hover);
  }

  /* Centre: track info */
  .lcd-track-info {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-width: 0;
    text-align: center;
  }

  .lcd-track-name {
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-semibold);
    color: var(--p11-text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
    line-height: 1.3;
  }

  .lcd-track-artist {
    font-size: var(--font-size-xs);
    color: var(--p11-text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
    line-height: 1.3;
  }

  /* Progress bar at bottom */
  .lcd-progress {
    width: 100%;
    height: 4px;
    appearance: none;
    background: linear-gradient(
      to right,
      var(--p11-accent) 0%,
      var(--p11-accent) var(--progress, 0%),
      var(--p11-bg-hover) var(--progress, 0%),
      var(--p11-bg-hover) 100%
    );
    border-radius: 0;
    cursor: pointer;
    margin: 0;
    display: block;
    flex-shrink: 0;
  }

  .lcd-progress::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 0;
    background: transparent;
  }

  .lcd-progress::-webkit-slider-thumb {
    appearance: none;
    width: 0;
    height: 0;
    background: var(--p11-text-primary);
    border-radius: var(--radius-full);
    cursor: pointer;
    margin-top: -4px;
    transition:
      width var(--transition-fast),
      height var(--transition-fast),
      margin-top var(--transition-fast);
  }

  .lcd-progress:hover::-webkit-slider-thumb {
    width: 10px;
    height: 10px;
    margin-top: -3px;
  }

  /* ── Idle states ─────────────────────────────────────────────── */
  .lcd-idle {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 100%;
  }

  .lcd-idle-icon {
    height: 100%;
    width: auto;
    object-fit: contain;
    padding: 12px;
    opacity: 0.6;
  }

  /* A narrow panel drops the shuffle/repeat and time columns and centres the
     (still ellipsised) track name over the progress bar. Last in the
     block, so it outranks the base rules it overrides. */
  @container (width < 300px) {
    .lcd-main {
      grid-template-columns: 1fr;
      padding: 0 var(--space-sm);
    }

    .lcd-time-group {
      display: none;
    }
  }
</style>
