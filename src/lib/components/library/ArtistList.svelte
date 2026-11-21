<script lang="ts">
  import type { Artist } from '#lib/types/models.js';
  import ArtistCard from './ArtistCard.svelte';
  import { createVirtualList } from '#lib/utils/virtualList.svelte.js';

  interface Props {
    artists: Artist[];
    selectedArtistId?: string | null;
    /** Keys this list's remembered scroll position. */
    viewKey?: string;
    onArtistClick?: (artist: Artist) => void;
  }

  let { artists, selectedArtistId = null, viewKey = 'artists-list', onArtistClick }: Props = $props();

  let viewportEl = $state<HTMLDivElement>();
  let innerEl = $state<HTMLDivElement>();

  const virtual = createVirtualList({
    count: () => artists.length,
    estimateRowHeight: () => 40,
    overscan: 8,
  });

  let visibleArtists = $derived(artists.slice(virtual.start, virtual.end));

  $effect(() => virtual.attachViewport(viewportEl));
  $effect(() => virtual.bindScrollMemory(viewKey, viewportEl));

  $effect(() => {
    artists.length;
    virtual.refresh();
  });

  $effect(() => {
    void virtual.windowing;
    void virtual.end;
    virtual.remeasure(innerEl);
  });
</script>

{#if artists.length === 0}
  <div class="empty-state">
    <p class="empty-title">No artists found</p>
    <p class="empty-description">Your artist library is empty.</p>
  </div>
{:else}
  <div class="artist-list" bind:this={viewportEl} onscroll={virtual.handleScroll}>
    <div class="artist-list-spacer" style:height={virtual.windowing ? `${virtual.totalSize}px` : undefined}>
      <div
        class="artist-list-window"
        class:is-windowed={virtual.windowing}
        bind:this={innerEl}
        style:transform={virtual.windowing ? `translateY(${virtual.padStart}px)` : undefined}
      >
        {#each visibleArtists as artist (artist.jellyfinId)}
          <ArtistCard
            {artist}
            selected={artist.jellyfinId === selectedArtistId}
            onClick={onArtistClick}
          />
        {/each}
      </div>
    </div>
  </div>
{/if}

<style>
  .artist-list {
    display: flex;
    flex-direction: column;
    height: 100%;
    background-color: var(--p11-bg-surface);
    border-radius: var(--p11-radius-md);
    overflow-y: auto;
    overscroll-behavior: contain;
    position: relative;
    padding: var(--space-xs) 0;
  }

  .artist-list-spacer {
    position: relative;
    width: 100%;
  }

  .artist-list-window.is-windowed {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: var(--space-3xl);
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
  }
</style>
