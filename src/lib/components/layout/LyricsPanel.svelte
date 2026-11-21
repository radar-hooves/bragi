<script lang="ts">
  import { getPlaybackState } from '#lib/stores/player.svelte.js';
  import { loadLyrics, setLyricsVisible, getLyricsOffsetMs } from '#lib/stores/lyrics.svelte.js';
  import { getEffectiveOffline } from '#lib/stores/offlineMode.svelte.js';
  import { resolveActiveLyricIndex } from '#lib/utils/lyrics.js';
  import { EmptyState, LoadingState } from '#lib/components/common/index.js';
  import SidePanelFrame from './SidePanelFrame.svelte';
  import type { TrackLyrics } from '#lib/types/models.js';
  import MicVocal from '@lucide/svelte/icons/mic-vocal';

  let lyrics = $state<TrackLyrics | null>(null);
  let loading = $state(false);
  let error = $state(false);
  let lineEls = $state<(HTMLElement | undefined)[]>([]);

  // Live playback position drives the highlight; it updates via ProgressEvent.
  const currentTrack = $derived(getPlaybackState().currentTrack);
  const trackId = $derived(currentTrack?.jellyfinId ?? null);

  // Smooth scrolling can stutter on WebKitGTK; fall back to instant on Linux.
  const scrollBehavior: ScrollBehavior =
    typeof document !== 'undefined' &&
    document.documentElement.getAttribute('data-platform') === 'linux'
      ? 'auto'
      : 'smooth';

  // Load lyrics whenever the track changes.
  $effect(() => {
    const id = trackId;
    lineEls = [];
    if (!id) {
      lyrics = null;
      error = false;
      loading = false;
      return;
    }
    // Offline: the lyrics endpoint needs the live server; skip the fetch.
    if (getEffectiveOffline()) {
      lyrics = null;
      error = false;
      loading = false;
      return;
    }

    let cancelled = false;
    loading = true;
    error = false;
    loadLyrics(id)
      .then((result) => {
        if (cancelled) return;
        lyrics = result;
      })
      .catch(() => {
        if (cancelled) return;
        error = true;
        lyrics = null;
      })
      .finally(() => {
        if (!cancelled) loading = false;
      });

    return () => {
      cancelled = true;
    };
  });

  // Index of the line to highlight in the accent colour. Synced lyrics use the
  // real per-line timestamps; unsynced lyrics (no timing from Jellyfin) fall
  // back to an even progress-based estimate so there is still a "currently sung"
  // cue — approximate, but better than a static wall of text.
  const activeIndex = $derived.by(() => {
    const state = getPlaybackState();
    return resolveActiveLyricIndex(
      lyrics,
      state.currentTimeMs,
      state.durationMs,
      getLyricsOffsetMs()
    );
  });

  // Keep the active line centred as playback advances (and after a seek).
  $effect(() => {
    const el = lineEls[activeIndex];
    el?.scrollIntoView({ block: 'center', behavior: scrollBehavior });
  });

  function handleClose(): void {
    setLyricsVisible(false);
  }
</script>

<SidePanelFrame title="Lyrics" icon={MicVocal} closeLabel="Close lyrics" onClose={handleClose} ariaLabel="Lyrics">
  <div class="lyrics-body">
    {#if !currentTrack}
      <EmptyState title="Nothing playing" description="Start a track to see its lyrics." />
    {:else if getEffectiveOffline()}
      <EmptyState title="Lyrics unavailable offline" description="Go online to fetch lyrics." />
    {:else if loading}
      <LoadingState message="Loading lyrics…" />
    {:else if error}
      <EmptyState title="Couldn’t load lyrics" description="Something went wrong fetching lyrics." />
    {:else if !lyrics || lyrics.lines.length === 0}
      <EmptyState title="No lyrics" description="No lyrics were found for this track." />
    {:else}
      <div class="lyrics-lines" class:synced={lyrics.synced} class:highlight={activeIndex >= 0}>
        {#each lyrics.lines as line, i}
          <p
            bind:this={lineEls[i]}
            class="lyric-line"
            class:active={i === activeIndex}
            class:passed={activeIndex >= 0 && i < activeIndex}
          >
            {line.text || ' '}
          </p>
        {/each}
      </div>
    {/if}
  </div>
</SidePanelFrame>

<style>
  .lyrics-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .lyrics-lines {
    display: flex;
    flex-direction: column;
    gap: var(--space-xs);
    padding: var(--space-lg) var(--space-md);
  }

  /* A little vertical room so the centred active line reads well, without the
     big empty gap the old 40% (of width) padding left at the top. */
  .lyrics-lines.highlight {
    padding: var(--space-xl) var(--space-md);
  }

  .lyric-line {
    margin: 0;
    padding: var(--space-xs) var(--space-sm);
    color: var(--p11-text-secondary);
    font-size: var(--font-size-md);
    line-height: 1.5;
    border-radius: var(--p11-radius-sm);
    transition:
      color var(--transition-fast),
      background-color var(--transition-fast),
      opacity var(--transition-fast);
  }

  /* Dim non-active lines so the currently-sung line stands out. */
  .lyrics-lines.highlight .lyric-line {
    opacity: 0.55;
  }

  .lyrics-lines.highlight .lyric-line.passed {
    opacity: 0.4;
  }

  /* The currently-sung line: the theme's accent colour, in a subtle accent pill. */
  .lyric-line.active {
    color: var(--p11-accent);
    background-color: var(--p11-accent-subtle);
    opacity: 1;
    font-weight: 700;
  }
</style>
