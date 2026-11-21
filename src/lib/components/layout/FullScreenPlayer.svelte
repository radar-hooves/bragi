<script lang="ts">
  import {
    getPlaybackState,
    getProgress,
    getCurrentTimeFormatted,
    getRemainingTimeFormatted,
    setPlaying,
    setCurrentTime,
    toggleShuffle,
    cycleRepeat,
  } from '#lib/stores/player.svelte.js';
  import { getHasNext, getHasPrevious, getUpcomingTracks } from '#lib/stores/queue.svelte.js';
  import {
    seek as tauriSeek,
    pause as tauriPause,
    resume as tauriResume,
    skipNext as tauriSkipNext,
    skipPrevious as tauriSkipPrevious,
    setVolume as tauriSetVolume,
  } from '#lib/services/tauri.js';
  import { reportTransportError } from '#lib/utils/transportError.js';
  import { cachedArtworkUrl, handleArtworkError, resetArtwork } from '#lib/utils/artworkFallback.js';
  import { closeFullScreenPlayer, isFullScreenPlayerOpen } from '#lib/stores/fullscreenPlayer.svelte.js';
  import {
    loadLyrics,
    getLyricsOffsetMs,
    adjustLyricsOffsetMs,
    resetLyricsOffset,
    LYRICS_OFFSET_STEP_MS,
  } from '#lib/stores/lyrics.svelte.js';
  import { loadWaveform, prefetchWaveforms } from '#lib/stores/waveform.svelte.js';
  import { resolveActiveLyricIndex } from '#lib/utils/lyrics.js';
  import type { TrackLyrics } from '#lib/types/models.js';
  import Waveform from './Waveform.svelte';
  import Shuffle from '@lucide/svelte/icons/shuffle';
  import Repeat from '@lucide/svelte/icons/repeat';
  import Repeat1 from '@lucide/svelte/icons/repeat-1';
  import Play from '@lucide/svelte/icons/play';
  import Pause from '@lucide/svelte/icons/pause';
  import SkipBack from '@lucide/svelte/icons/skip-back';
  import SkipForward from '@lucide/svelte/icons/skip-forward';
  import Volume2 from '@lucide/svelte/icons/volume-2';
  import Minus from '@lucide/svelte/icons/minus';
  import Plus from '@lucide/svelte/icons/plus';
  import X from '@lucide/svelte/icons/x';

  // WebKitGTK's smooth-scroll can be janky; use instant on Linux.
  const scrollBehavior: ScrollBehavior =
    typeof navigator !== 'undefined' && navigator.userAgent.toLowerCase().includes('linux')
      ? 'auto'
      : 'smooth';

  const track = $derived(getPlaybackState().currentTrack);
  // Always mounted (so the blurred backdrop layer is pre-warmed on the GPU and
  // opening is instant); the body + lyrics only render/fetch while open.
  const isOpen = $derived(isFullScreenPlayerOpen());

  // ── Waveform: precomputed peaks, prefetched one track ahead ──────────
  let waveform = $state<number[] | null>(null);
  // The track whose waveform is currently loaded/loading. Guards against the
  // playback-state object being replaced on every pause/resume/seek/volume
  // event (a new `currentTrack` reference for the SAME track) — without this,
  // the effect would re-run and null out `waveform`, unmounting the very
  // waveform being dragged to seek.
  let loadedWaveformId: string | null = null;

  $effect(() => {
    const id = track?.jellyfinId ?? null;
    if (!isOpen || !id) {
      waveform = null;
      loadedWaveformId = null;
      return;
    }
    if (id === loadedWaveformId) return; // same track — keep the current waveform
    loadedWaveformId = id;

    let cancelled = false;
    waveform = null;
    loadWaveform(id)
      .then((peaks) => {
        if (!cancelled) waveform = peaks;
      })
      .catch(() => {
        if (!cancelled) waveform = null;
      });
    // Warm the next queued track's waveform so advancing paints instantly.
    prefetchWaveforms(getUpcomingTracks().map((t) => t.jellyfinId));
    return () => {
      cancelled = true;
    };
  });

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

  async function handleVolume(event: Event): Promise<void> {
    const value = parseFloat((event.target as HTMLInputElement).value) / 100;
    try {
      await tauriSetVolume(value);
    } catch {
      /* browser-only mode */
    }
  }

  // ── Lyrics (independent, best-effort; never blocks anything) ────────────────
  let lyrics = $state<TrackLyrics | null>(null);
  let lyricsLoading = $state(false);
  let lyricsError = $state(false);
  let lineEls = $state<HTMLElement[]>([]);
  let lyricsBox = $state<HTMLElement | null>(null);

  $effect(() => {
    // Only fetch while open (the overlay stays mounted for pre-warming).
    const id = track?.jellyfinId;
    if (!isOpen || !id) {
      lyrics = null;
      return;
    }
    let cancelled = false;
    lyricsLoading = true;
    lyricsError = false;
    loadLyrics(id)
      .then((result) => {
        if (!cancelled) lyrics = result;
      })
      .catch(() => {
        if (!cancelled) {
          lyrics = null;
          lyricsError = true;
        }
      })
      .finally(() => {
        if (!cancelled) lyricsLoading = false;
      });
    return () => {
      cancelled = true;
    };
  });

  // Synced lyrics use real timestamps; unsynced fall back to an even
  // progress-based estimate so there is still a "now singing" cue.
  const activeIndex = $derived.by(() => {
    const state = getPlaybackState();
    return resolveActiveLyricIndex(
      lyrics,
      state.currentTimeMs,
      state.durationMs,
      getLyricsOffsetMs()
    );
  });

  /** Signed, human-readable offset readout, e.g. `+250 ms`, `0 ms`, `−500 ms`. */
  function formatLyricsOffset(ms: number): string {
    if (ms === 0) return '0 ms';
    return `${ms > 0 ? '+' : '−'}${Math.abs(ms)} ms`;
  }

  // Position the active line ~35% from the top of the lyrics box (Apple-Music
  // style: current line sits in the upper third, upcoming lines flow below),
  // rather than dead-centre — so the lyrics use the space above the album art.
  // Falls back to line 0 before playback reaches the first synced timestamp
  // (activeIndex is -1) so the box is correctly positioned from the very first
  // frame, using the same clientHeight-relative math as everything after —
  // `.fsp-lyrics`'s padding-top is only a viewport-relative fallback bound and
  // will disagree with this on short/unmaximized windows.
  $effect(() => {
    const el = lineEls[activeIndex >= 0 ? activeIndex : 0];
    const box = lyricsBox;
    if (!el || !box) return;
    const elCentre = el.getBoundingClientRect().top - box.getBoundingClientRect().top + box.scrollTop + el.clientHeight / 2;
    box.scrollTo({ top: Math.max(0, elCentre - box.clientHeight * 0.35), behavior: scrollBehavior });
  });

  function onKeydown(event: KeyboardEvent): void {
    if (isOpen && event.key === 'Escape') closeFullScreenPlayer();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="fsp" class:open={isOpen} role="dialog" aria-modal="true" aria-label="Now playing">
  <!-- Blurred album-art backdrop (kept mounted so its GPU layer is pre-warmed). -->
  {#if track?.albumJellyfinId}
    <img
      class="fsp-backdrop"
      src={cachedArtworkUrl(track.albumJellyfinId)}
      use:resetArtwork={cachedArtworkUrl(track.albumJellyfinId)}
      alt=""
      aria-hidden="true"
      onerror={(e) => handleArtworkError(e, track?.artworkUrl)}
    />
  {/if}
  <div class="fsp-scrim"></div>

  {#if isOpen}
    <button class="fsp-close" onclick={closeFullScreenPlayer} aria-label="Close full-screen player" title="Close (Esc)">
      <X size={22} />
    </button>

    <div class="fsp-body">
    <!-- Left: artwork + info + transport -->
    <div class="fsp-left">
      <div class="fsp-art">
        {#if track?.albumJellyfinId}
          <img
            src={cachedArtworkUrl(track.albumJellyfinId)}
            use:resetArtwork={cachedArtworkUrl(track.albumJellyfinId)}
            alt="Album artwork"
            onerror={(e) => handleArtworkError(e, track?.artworkUrl)}
          />
        {:else}
          <img src="/bragi-rune.png" alt="Bragi" class="fsp-art-placeholder" />
        {/if}
      </div>

      {#if track}
        <div class="fsp-info">
          <h1 class="fsp-title">{track.name}</h1>
          <p class="fsp-artist">
            {track.artistName}{#if track.albumName}&ensp;&mdash;&ensp;{track.albumName}{/if}
          </p>
        </div>

        <div class="fsp-progress-row">
          <span class="fsp-time">{getCurrentTimeFormatted()}</span>
          {#if waveform && waveform.length > 0}
            <Waveform peaks={waveform} progress={getProgress()} onSeek={handleSeek} height={56} />
          {:else}
            <input
              type="range"
              class="fsp-progress"
              min="0"
              max="100"
              value={getProgress()}
              style="--progress: {getProgress()}%"
              oninput={handleSeek}
              aria-label="Seek"
            />
          {/if}
          <span class="fsp-time">-{getRemainingTimeFormatted()}</span>
        </div>

        <div class="fsp-controls">
          <button
            class="fsp-mode"
            class:active={getPlaybackState().shuffle}
            onclick={toggleShuffle}
            aria-label="Shuffle"
            title="Shuffle"
          >
            <Shuffle size={20} />
          </button>
          <button
            class="fsp-transport"
            onclick={handleSkipPrevious}
            disabled={!getHasPrevious()}
            aria-label="Previous"
          >
            <SkipBack size={26} />
          </button>
          <button class="fsp-play" onclick={handlePlayPause} aria-label={getPlaybackState().isPlaying ? 'Pause' : 'Play'}>
            {#if getPlaybackState().isPlaying}
              <Pause size={30} />
            {:else}
              <Play size={30} />
            {/if}
          </button>
          <button
            class="fsp-transport"
            onclick={handleSkipNext}
            disabled={!getHasNext()}
            aria-label="Next"
          >
            <SkipForward size={26} />
          </button>
          <button
            class="fsp-mode"
            class:active={getPlaybackState().repeat !== 'off'}
            onclick={cycleRepeat}
            aria-label={`Repeat ${getPlaybackState().repeat}`}
            title={`Repeat ${getPlaybackState().repeat}`}
          >
            {#if getPlaybackState().repeat === 'one'}
              <Repeat1 size={20} />
            {:else}
              <Repeat size={20} />
            {/if}
          </button>
        </div>

        <div class="fsp-volume-row">
          <Volume2 size={16} />
          <input
            type="range"
            class="fsp-volume"
            min="0"
            max="100"
            value={getPlaybackState().volume * 100}
            style="--progress: {getPlaybackState().volume * 100}%"
            oninput={handleVolume}
            aria-label="Volume"
          />
        </div>
      {/if}
    </div>

    <!-- Right: flowing lyrics -->
    <div class="fsp-right" bind:this={lyricsBox}>
      {#if !track}
        <p class="fsp-lyrics-note">Nothing playing.</p>
      {:else if lyricsLoading}
        <p class="fsp-lyrics-note">Loading lyrics…</p>
      {:else if lyricsError}
        <p class="fsp-lyrics-note">Couldn’t load lyrics — retrying…</p>
      {:else if !lyrics || lyrics.lines.length === 0}
        <p class="fsp-lyrics-note">No lyrics for this track.</p>
      {:else}
        <div class="fsp-lyrics">
          {#each lyrics.lines as line, i}
            <p
              bind:this={lineEls[i]}
              class="fsp-line"
              class:active={i === activeIndex}
              class:passed={activeIndex >= 0 && i < activeIndex}
            >
              {line.text || ' '}
            </p>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Manual lyrics sync offset. Floats over the lyrics column so it
         sits outside the masked/scrolling area. Synced lyrics only — it has no
         effect on the unsynced progress estimate. Local-only, never sent to
         Jellyfin. -->
    {#if track && lyrics && lyrics.synced && lyrics.lines.length > 0}
      <div class="fsp-offset" role="group" aria-label="Lyrics timing offset (local only)">
        <button
          class="fsp-offset-btn"
          onclick={() => adjustLyricsOffsetMs(-LYRICS_OFFSET_STEP_MS)}
          title="Nudge lyrics later"
          aria-label="Nudge lyrics later"
        >
          <Minus size={16} />
        </button>
        <button
          class="fsp-offset-value"
          onclick={resetLyricsOffset}
          title="Lyrics sync offset — click to reset to 0"
          aria-label={`Lyrics offset ${formatLyricsOffset(getLyricsOffsetMs())}, click to reset`}
        >
          {formatLyricsOffset(getLyricsOffsetMs())}
        </button>
        <button
          class="fsp-offset-btn"
          onclick={() => adjustLyricsOffsetMs(LYRICS_OFFSET_STEP_MS)}
          title="Nudge lyrics earlier"
          aria-label="Nudge lyrics earlier"
        >
          <Plus size={16} />
        </button>
      </div>
    {/if}
    </div>
  {/if}
</div>

<style>
  .fsp {
    container: fsp / size;
    position: fixed;
    inset: 0;
    z-index: 200;
    overflow: hidden;
    color: var(--p11-text-primary);
    /* Kept mounted (so the blurred backdrop layer stays warm on the GPU); only
       opacity flips on open, which composites instantly instead of building and
       blurring the layer at the moment the user opens it. */
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--transition-medium, 200ms) ease;
  }

  .fsp.open {
    opacity: 1;
    pointer-events: auto;
  }

  /* Blurred album art fills the backdrop; the scrim darkens it for contrast. */
  .fsp-backdrop {
    position: absolute;
    top: 50%;
    left: 50%;
    /* Blur a heavily DOWN-SCALED copy of the art, then scale it up. Blur cost is
       ~ pixels x radius, so blurring at ~32% size costs ~1/10th of a full-size
       blur — this is what makes the overlay open smoothly on WebKitGTK. The 3D
       transform keeps it on its own GPU layer. */
    width: 32%;
    height: 32%;
    object-fit: cover;
    filter: blur(13px) saturate(1.6);
    transform: translate(-50%, -50%) scale(3.6) translateZ(0);
    will-change: transform;
    backface-visibility: hidden;
  }

  .fsp-scrim {
    position: absolute;
    inset: 0;
    /* Plain gradient overlay only — NO backdrop-filter (very expensive on
       WebKitGTK: it re-samples everything behind it every frame). */
    background: radial-gradient(
      ellipse at 30% 40%,
      rgb(var(--p11-shade-rgb) / 0.35),
      rgb(var(--p11-shade-rgb) / 0.72)
    );
  }

  .fsp-close {
    position: absolute;
    top: var(--space-lg);
    right: var(--space-lg);
    z-index: 2;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    border: none;
    border-radius: var(--radius-full);
    background: rgb(var(--p11-overlay-rgb) / 0.1);
    color: rgb(var(--p11-overlay-rgb));
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .fsp-close:hover {
    background: rgb(var(--p11-overlay-rgb) / 0.22);
  }

  .fsp-body {
    position: relative;
    z-index: 1;
    display: grid;
    grid-template-columns: minmax(320px, 40%) 1fr;
    grid-template-rows: minmax(0, 1fr);
    gap: clamp(var(--space-xl), 5vw, 6rem);
    align-items: center;
    height: 100%;
    padding: clamp(var(--space-xl), 6vh, 6rem) clamp(var(--space-xl), 6vw, 6rem);
  }

  /* ── Left column ─────────────────────────────────────────────── */
  /* The column fills the window's height; the cover gives up height first, so the
     transport never runs off a short window. */
  .fsp-left {
    display: flex;
    flex-direction: column;
    align-self: stretch;
    justify-content: center;
    gap: var(--space-lg);
    min-width: 0;
    min-height: 0;
    max-width: 460px;
  }

  /* The cover is the column's width, or on a short window the height left over
     once the title, seek bar, transport and volume (about 27rem with the window
     padding) have their room; it stays square either way. */
  .fsp-art {
    flex: none;
    align-self: center;
    max-width: 100%;
    aspect-ratio: 1;
    width: min(100%, max(8rem, calc(100cqh - 27rem)));
    border-radius: var(--p11-radius-lg, 12px);
    overflow: hidden;
    box-shadow: 0 30px 60px rgb(var(--p11-shade-rgb) / 0.5);
  }

  .fsp-art img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .fsp-art img.fsp-art-placeholder {
    object-fit: contain;
    padding: 22%;
    background: rgb(var(--p11-overlay-rgb) / 0.06);
  }

  .fsp-info {
    min-width: 0;
  }

  .fsp-title {
    margin: 0;
    font-size: clamp(1.25rem, 2vw, 1.75rem);
    font-weight: 700;
    line-height: 1.2;
    color: rgb(var(--p11-overlay-rgb));
  }

  .fsp-artist {
    margin: 4px 0 0;
    font-size: var(--font-size-md);
    color: rgb(var(--p11-overlay-rgb) / 0.7);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .fsp-progress-row,
  .fsp-volume-row {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    color: rgb(var(--p11-overlay-rgb) / 0.7);
  }

  .fsp-time {
    font-size: var(--font-size-sm);
    font-variant-numeric: tabular-nums;
    min-width: 40px;
    text-align: center;
  }

  .fsp-progress,
  .fsp-volume {
    flex: 1;
    height: 5px;
    appearance: none;
    border-radius: var(--radius-full);
    background: linear-gradient(
      to right,
      var(--p11-accent) 0%,
      var(--p11-accent) var(--progress, 0%),
      rgb(var(--p11-overlay-rgb) / 0.22) var(--progress, 0%),
      rgb(var(--p11-overlay-rgb) / 0.22) 100%
    );
    cursor: pointer;
  }

  .fsp-progress::-webkit-slider-thumb,
  .fsp-volume::-webkit-slider-thumb {
    appearance: none;
    width: 12px;
    height: 12px;
    border-radius: var(--radius-full);
    background: rgb(var(--p11-overlay-rgb));
    margin-top: -3.5px;
  }

  .fsp-controls {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-lg);
  }

  .fsp-transport,
  .fsp-mode,
  .fsp-play {
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    background: transparent;
    color: rgb(var(--p11-overlay-rgb));
    cursor: pointer;
    transition:
      color var(--transition-fast),
      transform var(--transition-fast),
      opacity var(--transition-fast);
  }

  .fsp-mode {
    color: rgb(var(--p11-overlay-rgb) / 0.6);
  }

  .fsp-mode.active {
    color: var(--p11-accent);
  }

  .fsp-transport:hover:not(:disabled),
  .fsp-mode:hover {
    transform: scale(1.1);
    color: rgb(var(--p11-overlay-rgb));
  }

  .fsp-transport:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .fsp-play {
    width: 60px;
    height: 60px;
    border-radius: var(--radius-full);
    background: rgb(var(--p11-overlay-rgb));
    color: rgb(var(--p11-shade-rgb));
  }

  .fsp-play:hover {
    transform: scale(1.05);
  }

  /* The volume slider spans the full column width (matches the seek bar). */

  /* ── Right column: flowing lyrics ────────────────────────────── */
  .fsp-right {
    height: 100%;
    overflow-y: auto;
    min-width: 0;
    scrollbar-width: none;
    mask-image: linear-gradient(
      to bottom,
      transparent 0%,
      rgb(var(--p11-shade-rgb)) 12%,
      rgb(var(--p11-shade-rgb)) 88%,
      transparent 100%
    );
  }

  .fsp-right::-webkit-scrollbar {
    display: none;
  }

  /* Reserve room above (so line 1 can sit at the ~35% mark) and below (so the
     last line can still scroll up to it). Stable from the first frame — the
     lyrics never jump when the first line activates. */
  .fsp-lyrics {
    padding-top: 33vh;
    padding-bottom: 60vh;
  }

  .fsp-line {
    margin: 0;
    /* Leaves headroom for the active line's 1.14x scale below so it never
       overflows the column and gets clipped instead of wrapping. */
    max-width: calc(100% / 1.14);
    padding: var(--space-sm) 0;
    font-size: clamp(1.35rem, 2.3vw, 2.1rem);
    font-weight: 700;
    line-height: 1.28;
    color: rgb(var(--p11-overlay-rgb) / 0.38);
    transform-origin: left center;
    transition:
      color var(--transition-medium, 200ms) ease,
      transform var(--transition-medium, 200ms) ease;
  }

  .fsp-line.passed {
    color: rgb(var(--p11-overlay-rgb) / 0.28);
  }

  /* The currently-sung line: bright accent, clearly larger (a GPU transform, so
     no layout shift), with a soft glow so it reads at a glance. */
  .fsp-line.active {
    color: var(--p11-accent);
    transform: scale(1.14);
    text-shadow: 0 2px 26px rgb(var(--p11-shade-rgb) / 0.5);
  }

  .fsp-lyrics-note {
    color: rgb(var(--p11-overlay-rgb) / 0.6);
    font-size: var(--font-size-md);
  }

  /* Floating lyrics-offset stepper: positioned over the lyrics column,
     outside .fsp-right so the mask/scroll never clips it. */
  .fsp-offset {
    position: absolute;
    right: clamp(var(--space-xl), 6vw, 6rem);
    bottom: clamp(var(--space-lg), 5vh, 4rem);
    z-index: 2;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px;
    background: rgb(var(--p11-shade-rgb) / 0.4);
    backdrop-filter: blur(10px);
    border: 1px solid rgb(var(--p11-overlay-rgb) / 0.14);
    border-radius: var(--p11-radius-md);
    opacity: 0.55;
    transition: opacity var(--transition-fast);
  }

  .fsp-offset:hover {
    opacity: 1;
  }

  .fsp-offset-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: var(--p11-radius-sm);
    color: rgb(var(--p11-overlay-rgb) / 0.85);
    cursor: pointer;
    transition: background-color var(--transition-fast);
  }

  .fsp-offset-btn:hover {
    background: rgb(var(--p11-overlay-rgb) / 0.14);
    color: rgb(var(--p11-overlay-rgb) / 1);
  }

  .fsp-offset-value {
    min-width: 68px;
    padding: 0 var(--space-xs);
    background: transparent;
    border: none;
    color: rgb(var(--p11-overlay-rgb) / 0.85);
    font: inherit;
    font-size: var(--font-size-sm);
    font-variant-numeric: tabular-nums;
    text-align: center;
    cursor: pointer;
  }

  .fsp-offset-value:hover {
    color: rgb(var(--p11-overlay-rgb) / 1);
    text-decoration: underline;
  }

  /* 820px at the 13px root; rem so UI zoom counts (WebKit compares px against
     the unzoomed window). */
  @container fsp (width <= 63.1rem) {
    .fsp-body {
      grid-template-columns: 1fr;
      grid-template-rows: minmax(0, auto) minmax(0, 1fr);
      gap: var(--space-lg);
      padding: var(--space-2xl) var(--space-lg) var(--space-lg);
    }
    .fsp-left {
      max-width: 320px;
      margin: 0 auto;
    }
    .fsp-art {
      max-width: 240px;
      margin: 0 auto;
      width: min(100%, max(6rem, calc(100cqh - 32rem)));
    }
  }
</style>
