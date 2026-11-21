<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import {
    getPlaybackState,
    setCurrentTime,
    toggleShuffle,
    cycleRepeat,
  } from '#lib/stores/player.svelte.js';
  import { getHasNext, getHasPrevious } from '#lib/stores/queue.svelte.js';
  import {
    seek as tauriSeek,
    skipNext as tauriSkipNext,
    skipPrevious as tauriSkipPrevious,
  } from '#lib/services/tauri.js';
  import { reportTransportError } from '#lib/utils/transportError.js';
  import { togglePlayPause } from '#lib/utils/playback.js';
  import SkipBack from '@lucide/svelte/icons/skip-back';
  import Play from '@lucide/svelte/icons/play';
  import Pause from '@lucide/svelte/icons/pause';
  import SkipForward from '@lucide/svelte/icons/skip-forward';
  import LcdPanel from './LcdPanel.svelte';
  import ToolbarRight from './ToolbarRight.svelte';

  interface Props {
    onSearch?: (query: string, activate?: boolean) => void;
  }

  let { onSearch }: Props = $props();

  async function handleSeek(event: Event): Promise<void> {
    const target = event.target as HTMLInputElement;
    const percent = parseFloat(target.value);
    const positionMs = Math.round((getPlaybackState().durationMs * percent) / 100);
    setCurrentTime(positionMs);
    try {
      await tauriSeek(positionMs);
    } catch (err) {
      reportTransportError('Seek', err);
    }
  }

  async function handleSkipNext(): Promise<void> {
    try {
      await tauriSkipNext();
    } catch (err) {
      reportTransportError('Skip to next track', err);
    }
  }

  async function handleSkipPrevious(): Promise<void> {
    try {
      await tauriSkipPrevious();
    } catch (err) {
      reportTransportError('Skip to previous track', err);
    }
  }

  function handleShuffle(): void {
    toggleShuffle();
  }

  function handleRepeat(): void {
    cycleRepeat();
  }

  // Handle window dragging from header bar
  function handleDragRegionMouseDown(event: MouseEvent): void {
    if (event.buttons !== 1) return;
    const target = event.target as HTMLElement;
    const interactive = target.closest('button, input, a, [role="button"]');
    if (!interactive) {
      getCurrentWindow()
        .startDragging()
        .catch((e) => console.warn('Window drag failed:', e));
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<header class="top-bar" onmousedown={handleDragRegionMouseDown}>
  <!-- Left: transport controls -->
  <div class="left-group">
    <div class="transport" class:idle={!getPlaybackState().currentTrack}>
      <button
        class="transport-btn"
        onclick={handleSkipPrevious}
        disabled={!getHasPrevious()}
        aria-label="Previous"
      >
        <SkipBack size={18} fill="currentColor" />
      </button>
      <button
        class="transport-btn play-btn"
        onclick={togglePlayPause}
        aria-label={getPlaybackState().isPlaying ? 'Pause' : 'Play'}
      >
        {#if getPlaybackState().isPlaying}
          <Pause size={24} fill="currentColor" />
        {:else}
          <Play size={24} fill="currentColor" />
        {/if}
      </button>
      <button class="transport-btn" onclick={handleSkipNext} disabled={!getHasNext()} aria-label="Next">
        <SkipForward size={18} fill="currentColor" />
      </button>
    </div>
  </div>

  <!-- Centre: LCD display panel -->
  <LcdPanel onShuffle={handleShuffle} onRepeat={handleRepeat} onSeek={handleSeek} />

  <!-- Right: Search + Offline + Device + Settings -->
  <ToolbarRight {onSearch} />
</header>

<style>
  /* Three columns, the now-playing panel in the middle, centred on the window
     because the side tracks are equal and can shrink to nothing (minmax(0, 1fr)),
     so a side group never moves the panel: when the bar is too narrow for a
     group it clips. --side-reserve is the wider side group (the full toolbar,
     312px); the panel gets the window less that on both sides, from 176px to
     600px. Below 688px the transport keeps only play/pause so it fits the
     156px track at the 520px window minimum. The one row is the bar's height:
     an auto row lets the logo and artwork size the panel to their own pixels. */
  .top-bar {
    --side-reserve: 312px;
    position: relative;
    display: grid;
    grid-template-columns:
      minmax(0, 1fr)
      minmax(0, clamp(176px, 100% - 2 * (var(--side-reserve) + var(--space-md)), 600px))
      minmax(0, 1fr);
    grid-template-rows: 100%;
    align-items: center;
    column-gap: var(--space-md);
    height: var(--header-height);
    background-color: var(--p11-bg-surface);
    border-bottom: 1px solid var(--p11-border);
    flex-shrink: 0;
    /* Enable window dragging */
    -webkit-app-region: drag;
    app-region: drag;
  }

  /* Disable drag on interactive elements */
  .top-bar :global(button),
  .top-bar :global(input) {
    -webkit-app-region: no-drag;
    app-region: no-drag;
  }

  /* ── Left group: transport controls ── */
  .left-group {
    display: flex;
    align-items: center;
    /* Clears the window controls at every width (the macOS traffic lights
       end by 79px, Linux's in-app controls at 96px) and ends the transport
       at the sidebar's edge. */
    padding-left: 118px;
    min-width: 0;
    overflow: hidden;
  }

  .transport {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }

  .transport-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: var(--radius-full);
    color: var(--p11-text-secondary);
    cursor: pointer;
    transition: color var(--transition-fast);
  }

  .transport-btn:hover:not(:disabled) {
    color: var(--p11-text-primary);
  }

  .transport-btn:disabled {
    opacity: 0.2;
    cursor: default;
  }

  .play-btn {
    width: 38px;
    height: 38px;
    color: var(--p11-text-primary);
  }

  .play-btn:hover:not(:disabled) {
    color: var(--p11-accent);
  }

  /* ── Idle state ─────────────────────────────────────────────── */
  .transport.idle {
    opacity: 0.3;
    pointer-events: none;
  }

  /* Compact mode (shared with ToolbarRight): the toolbar has folded into its
     overflow menu, so the transport group (240px) is the wider side. */
  @container app (width <= 67.7rem) {
    .top-bar {
      --side-reserve: 240px;
    }
  }

  @container app (width <= 52.9rem) {
    .transport-btn:not(.play-btn) {
      display: none;
    }
  }
</style>
