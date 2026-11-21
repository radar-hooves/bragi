<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { toast } from 'svelte-sonner';
  import {
    getPlaybackState,
    getProgress,
    setPlaying,
    setCurrentTime,
  } from '#lib/stores/player.svelte.js';
  import { getHasNext, getHasPrevious } from '#lib/stores/queue.svelte.js';
  import {
    seek as tauriSeek,
    pause as tauriPause,
    resume as tauriResume,
    skipNext as tauriSkipNext,
    skipPrevious as tauriSkipPrevious,
    setVolume as tauriSetVolume,
    closeMiniPlayer,
    setMiniAlwaysOnTop,
    getSettings,
  } from '#lib/services/tauri.js';
  import { reportTransportError } from '#lib/utils/transportError.js';
  import { cachedArtworkUrl, handleArtworkError, resetArtwork } from '#lib/utils/artworkFallback.js';
  import Play from '@lucide/svelte/icons/play';
  import Pause from '@lucide/svelte/icons/pause';
  import SkipBack from '@lucide/svelte/icons/skip-back';
  import SkipForward from '@lucide/svelte/icons/skip-forward';
  import Minimize2 from '@lucide/svelte/icons/minimize-2';
  import Pin from '@lucide/svelte/icons/pin';
  import PinOff from '@lucide/svelte/icons/pin-off';
  import Music from '@lucide/svelte/icons/music';

  const track = $derived(getPlaybackState().currentTrack);
  const volumePct = $derived(Math.round(getPlaybackState().volume * 100));

  // Reflects the persisted preference; the window itself is created with the
  // correct always-on-top state by the backend.
  let alwaysOnTop = $state(true);

  // A transient volume readout that appears while scroll-adjusting.
  let volumeFlash = $state(false);
  let volumeFlashTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(async () => {
    try {
      alwaysOnTop = (await getSettings()).miniPlayerAlwaysOnTop;
    } catch {
      /* not in Tauri — keep default */
    }
    window.addEventListener('keydown', onKeydown);
  });

  onDestroy(() => {
    window.removeEventListener('keydown', onKeydown);
    clearTimeout(volumeFlashTimer);
  });

  // Escape or the toggle shortcut returns to the full player.
  function onKeydown(event: KeyboardEvent): void {
    const toggle = (event.ctrlKey || event.metaKey) && event.shiftKey && event.key.toLowerCase() === 'm';
    if (event.key === 'Escape' || toggle) {
      event.preventDefault();
      handleExpand();
    }
  }

  // ── Transport (mirrors PlayerBar) ──────────────────────────────────────────
  async function handlePlayPause(): Promise<void> {
    const newPlaying = !getPlaybackState().isPlaying;
    setPlaying(newPlaying);
    try {
      await (newPlaying ? tauriResume() : tauriPause());
    } catch (err) {
      setPlaying(!newPlaying);
      reportTransportError(newPlaying ? 'Resume' : 'Pause', err);
    }
  }

  async function handleSeek(event: Event): Promise<void> {
    const percent = parseFloat((event.target as HTMLInputElement).value);
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

  // Scroll-to-adjust volume (no slider, to keep the window compact).
  async function handleWheel(event: WheelEvent): Promise<void> {
    event.preventDefault();
    const current = getPlaybackState().volume;
    const step = event.deltaY < 0 ? 0.05 : -0.05;
    const next = Math.min(1, Math.max(0, Math.round((current + step) * 100) / 100));
    volumeFlash = true;
    clearTimeout(volumeFlashTimer);
    volumeFlashTimer = setTimeout(() => (volumeFlash = false), 900);
    try {
      await tauriSetVolume(next);
    } catch {
      /* browser-only mode */
    }
  }

  async function handleTogglePin(): Promise<void> {
    alwaysOnTop = !alwaysOnTop;
    try {
      await setMiniAlwaysOnTop(alwaysOnTop);
    } catch {
      /* browser-only mode */
    }
  }

  async function handleExpand(): Promise<void> {
    try {
      await closeMiniPlayer();
    } catch (e) {
      console.error('Mini player close failed:', e);
      toast.error('Could not close the mini player', {
        description: e instanceof Error ? e.message : String(e),
      });
    }
  }

  // Whole-window drag (except over interactive elements). The startDragging()
  // fallback covers WebKitGTK, where -webkit-app-region is unreliable.
  function handleDragMouseDown(event: MouseEvent): void {
    if (event.buttons !== 1) return;
    const target = event.target as HTMLElement;
    if (!target.closest('button, input, a, [role="button"]')) {
      getCurrentWindow()
        .startDragging()
        .catch((e) => console.warn('Window drag failed:', e));
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions a11y_no_noninteractive_element_interactions -->
<div
  class="mini"
  onmousedown={handleDragMouseDown}
  onwheel={handleWheel}
  role="group"
  aria-label="MiniPlayer"
>
  <div class="mini-art">
    {#if track?.albumJellyfinId}
      <img
        src={cachedArtworkUrl(track.albumJellyfinId)}
        use:resetArtwork={cachedArtworkUrl(track.albumJellyfinId)}
        alt=""
        draggable="false"
        onerror={(e) => handleArtworkError(e, track?.artworkUrl)}
      />
    {:else}
      <div class="mini-art-placeholder"><Music size={22} /></div>
    {/if}
  </div>

  <div class="mini-body">
    <div class="mini-meta">
      <div class="mini-titles">
        <span class="mini-title">{track?.name ?? 'Not playing'}</span>
        <span class="mini-artist">{track?.artistName ?? ''}</span>
      </div>
      <div class="mini-corner">
        <button
          class="mini-btn"
          onclick={handleTogglePin}
          aria-label={alwaysOnTop ? 'Stop floating on top' : 'Float on top'}
          title={alwaysOnTop ? 'Floating on top' : 'Not on top'}
        >
          {#if alwaysOnTop}<Pin size={14} fill="currentColor" />{:else}<PinOff size={14} />{/if}
        </button>
        <button
          class="mini-btn"
          onclick={handleExpand}
          aria-label="Back to full player"
          title="Back to full player"
        >
          <Minimize2 size={14} />
        </button>
      </div>
    </div>

    <div class="mini-controls">
      <button
        class="mini-transport"
        onclick={handleSkipPrevious}
        disabled={!getHasPrevious()}
        aria-label="Previous"
      >
        <SkipBack size={15} fill="currentColor" />
      </button>
      <button
        class="mini-transport play"
        onclick={handlePlayPause}
        aria-label={getPlaybackState().isPlaying ? 'Pause' : 'Play'}
      >
        {#if getPlaybackState().isPlaying}
          <Pause size={17} fill="currentColor" />
        {:else}
          <Play size={17} fill="currentColor" />
        {/if}
      </button>
      <button
        class="mini-transport"
        onclick={handleSkipNext}
        disabled={!getHasNext()}
        aria-label="Next"
      >
        <SkipForward size={15} fill="currentColor" />
      </button>
      <input
        class="mini-seek"
        type="range"
        min="0"
        max="100"
        step="0.1"
        value={getProgress()}
        oninput={handleSeek}
        disabled={!track}
        aria-label="Seek"
      />
    </div>
  </div>

  {#if volumeFlash}
    <div class="mini-volume-flash">Vol {volumePct}%</div>
  {/if}
</div>

<style>
  .mini {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    width: 100vw;
    height: 100vh;
    padding: var(--space-md);
    background-color: var(--p11-bg-surface);
    overflow: hidden;
    -webkit-app-region: drag;
    app-region: drag;
    user-select: none;
  }

  /* macOS overlays the native traffic lights top-left, where AppKit puts them
     (21px down against the macOS 14 SDK Bragi builds with, 23px against 26+);
     the strip clears them and the 64px artwork still fits the default 96px
     window. */
  :global([data-platform='macos']) .mini {
    padding-top: 24px;
    padding-bottom: var(--space-sm);
  }

  .mini :global(button),
  .mini :global(input) {
    -webkit-app-region: no-drag;
    app-region: no-drag;
  }

  .mini-art {
    flex-shrink: 0;
    width: 64px;
    height: 64px;
    border-radius: var(--radius-md);
    overflow: hidden;
    background-color: var(--p11-bg-elevated);
  }

  .mini-art img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .mini-art-placeholder {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 100%;
    color: var(--p11-text-tertiary);
  }

  .mini-body {
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    flex: 1;
    min-width: 0;
    height: 64px;
  }

  .mini-meta {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-sm);
  }

  .mini-titles {
    display: flex;
    flex-direction: column;
    min-width: 0;
    line-height: 1.25;
  }

  .mini-title {
    font-size: var(--font-size-sm);
    font-weight: 600;
    color: var(--p11-text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .mini-artist {
    font-size: var(--font-size-xs);
    color: var(--p11-text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .mini-corner {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;
  }

  .mini-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: var(--radius-full);
    color: var(--p11-text-tertiary);
    cursor: pointer;
    transition: color var(--transition-fast);
  }

  .mini-btn:hover {
    color: var(--p11-text-primary);
  }

  .mini-controls {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
  }

  .mini-transport {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: var(--radius-full);
    color: var(--p11-text-secondary);
    cursor: pointer;
    transition: color var(--transition-fast);
  }

  .mini-transport:hover:not(:disabled) {
    color: var(--p11-text-primary);
  }

  .mini-transport.play {
    width: 30px;
    height: 30px;
    color: var(--p11-text-primary);
  }

  .mini-transport.play:hover {
    color: var(--p11-accent);
  }

  .mini-transport:disabled {
    opacity: 0.25;
    cursor: default;
  }

  .mini-seek {
    flex: 1;
    min-width: 0;
    height: 4px;
    margin-left: var(--space-xs);
    accent-color: var(--p11-accent);
    cursor: pointer;
  }

  .mini-seek:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .mini-volume-flash {
    position: absolute;
    right: var(--space-md);
    bottom: var(--space-xs);
    padding: 2px 8px;
    border-radius: var(--radius-full);
    background-color: var(--p11-bg-elevated);
    color: var(--p11-text-secondary);
    font-size: var(--font-size-xs);
    font-variant-numeric: tabular-nums;
    pointer-events: none;
  }
</style>
