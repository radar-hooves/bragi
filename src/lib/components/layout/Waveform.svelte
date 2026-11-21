<script lang="ts">
  /**
   * Seekable waveform.
   *
   * Draws precomputed peak buckets on a canvas — the played portion in the
   * theme accent, the rest muted — with a transparent full-size range input
   * overlaid for pointer + keyboard seeking and accessibility. The parent only
   * mounts this when peaks exist; otherwise it shows the plain seek bar.
   */
  interface Props {
    /** Normalised peak buckets (0–1). */
    peaks: number[];
    /** Playback progress, 0–100. */
    progress: number;
    /** Seek handler — receives the range input's `input` event (value 0–100). */
    onSeek: (event: Event) => void;
    /** Drawn height in CSS pixels. */
    height?: number;
  }

  import { getUiState } from '#lib/stores/ui.svelte.js';

  let { peaks, progress, onSeek, height = 56 }: Props = $props();

  let canvas = $state<HTMLCanvasElement | null>(null);
  let width = $state(0);

  function draw(): void {
    const c = canvas;
    const w = Math.floor(width);
    if (!c || peaks.length === 0 || w <= 0) return;

    const ctx = c.getContext('2d');
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    c.width = Math.floor(w * dpr);
    c.height = Math.floor(height * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, height);

    // Canvas cannot consume CSS custom properties, so both colours are resolved
    // from the live computed style — which is also what makes the waveform
    // track a theme or accent change. No literal fallback: tokens.css always
    // defines these, and a hard-coded one would be a second palette that
    // silently disagrees with the app's (was exactly that failure).
    const root = getComputedStyle(document.documentElement);
    const accent = root.getPropertyValue('--p11-accent').trim();
    const overlay = root.getPropertyValue('--p11-overlay-rgb').trim();
    if (!accent || !overlay) {
      // Canvas ignores an invalid fillStyle silently, so an unresolved token
      // would paint the bars in whatever colour was last set — a wrong-colour
      // waveform with nothing reported. Say so and draw nothing.
      console.error('Waveform: colour tokens did not resolve; skipping draw.');
      return;
    }
    const unplayed = `rgb(${overlay} / 0.24)`;

    // Group the stored peaks down to one bar per ~3px so bars stay crisp and
    // evenly spaced at any width (400 raw buckets in ~350px would otherwise
    // smear into a solid block).
    const BAR_SLOT = 3;
    const bars = Math.max(1, Math.min(peaks.length, Math.floor(w / BAR_SLOT)));
    const slot = w / bars;
    const barW = Math.max(1, slot - 1); // 1px gap between bars
    const mid = height / 2;
    const playedX = (Math.max(0, Math.min(100, progress)) / 100) * w;
    const usable = height - 2;

    for (let i = 0; i < bars; i++) {
      // Max-pool the raw buckets that fall into this bar.
      const start = Math.floor((i * peaks.length) / bars);
      const end = Math.max(start + 1, Math.floor(((i + 1) * peaks.length) / bars));
      let amp = 0;
      for (let j = start; j < end; j++) amp = Math.max(amp, peaks[j] ?? 0);
      // Floor the amplitude so silent sections still show a thin sliver.
      amp = Math.max(0.03, amp);

      const x = i * slot;
      const barH = amp * usable;
      ctx.fillStyle = x + barW / 2 <= playedX ? accent : unplayed;
      ctx.fillRect(x, mid - barH / 2, barW, barH);
    }
  }

  // Redraw whenever the peaks, width, progress, height — or the accent colour —
  // change. Depending on the accent explicitly means a theme change repaints
  // immediately, even while paused (when the progress tick is idle).
  $effect(() => {
    void peaks;
    void width;
    void progress;
    void height;
    void getUiState().accentColour;
    draw();
  });
</script>

<div class="wf" style="--wf-height: {height}px" bind:clientWidth={width}>
  <canvas bind:this={canvas} class="wf-canvas" aria-hidden="true"></canvas>
  <input
    type="range"
    class="wf-range"
    min="0"
    max="100"
    value={progress}
    oninput={onSeek}
    aria-label="Seek"
  />
</div>

<style>
  .wf {
    position: relative;
    flex: 1;
    height: var(--wf-height);
    min-width: 0;
  }

  .wf-canvas {
    display: block;
    width: 100%;
    height: 100%;
  }

  /* Transparent, full-size slider on top: gives pointer + keyboard seeking and
     a11y for free, reusing the same onSeek handler as the plain seek bar. */
  .wf-range {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    appearance: none;
    background: transparent;
    cursor: pointer;
  }

  .wf-range::-webkit-slider-runnable-track {
    height: 100%;
    background: transparent;
  }

  /* A thin, subtle playhead thumb. */
  .wf-range::-webkit-slider-thumb {
    appearance: none;
    width: 3px;
    height: var(--wf-height);
    border-radius: var(--radius-full);
    background: rgb(var(--p11-overlay-rgb));
    box-shadow: 0 0 4px rgb(var(--p11-shade-rgb) / 0.5);
  }

  .wf-range:focus-visible {
    outline: 2px solid var(--p11-accent);
    outline-offset: 2px;
    border-radius: var(--radius-sm);
  }
</style>
