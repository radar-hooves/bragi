<script lang="ts">
  import type { Artist } from '#lib/types/models.js';

  interface Props {
    artist: Artist;
    selected?: boolean;
    onClick?: (artist: Artist) => void;
  }

  let { artist, selected = false, onClick }: Props = $props();

  function handleClick(): void {
    onClick?.(artist);
  }
</script>

<button type="button" class="artist-row" class:selected onclick={handleClick}>
  <span class="artist-name">{artist.name}</span>
  {#if artist.albumCount > 0}
    <span class="album-count">{artist.albumCount}</span>
  {/if}
</button>

<style>
  .artist-row {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    width: 100%;
    padding: 2px var(--space-md);
    height: 22px;
    background-color: transparent;
    border: none;
    cursor: pointer;
    font-family: var(--font-family);
    text-align: left;
    transition: background-color var(--transition-fast);
  }

  .artist-row:hover {
    background-color: var(--p11-bg-elevated);
  }

  .artist-row.selected {
    background-color: var(--p11-selection);
  }

  .artist-row:focus-visible {
    outline: 2px solid var(--p11-accent);
    outline-offset: -2px;
  }

  .artist-name {
    font-size: var(--font-size-sm);
    color: var(--p11-text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }

  .album-count {
    font-size: var(--font-size-xs);
    color: var(--p11-text-muted);
    flex-shrink: 0;
  }
</style>
