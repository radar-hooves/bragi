<script lang="ts">
  import type { JellyfinPlaylistTrack } from '#lib/types/models.js';
  import { cachedArtworkUrl, handleArtworkError, resetArtwork } from '#lib/utils/artworkFallback.js';

  interface Props {
    /** Playlist tracks, walked in order to source the collage covers. */
    tracks: JellyfinPlaylistTrack[];
    /**
     * Font-size (px) of the ♫ fallback shown for a genuinely-empty playlist.
     * 64 suits the large artwork box; pass something small (~16) for a sidebar
     * mini-collage so the glyph never overflows a tiny slot.
     */
    emptyGlyphSize?: number;
  }

  let { tracks, emptyGlyphSize = 64 }: Props = $props();

  // Walk the tracks in order and collect the first four DISTINCT album covers.
  // Reading the deeply-reactive `tracks` proxy subscribes this derived to
  // length/index/field changes, so add/remove (the store replaces
  // currentPlaylist) and in-place reorder (splice) both re-run it. The early
  // exit at four keeps the walk cheap even for very large playlists.
  const covers = $derived.by(() => {
    const seen = new Set<string>();
    const out: { id: string; fallback?: string }[] = [];
    for (const track of tracks) {
      const id = track.albumJellyfinId;
      // Skip tracks with no album id (would build an always-failing artwork URL)
      // and albums already collected — Jellyfin collages DISTINCT albums.
      if (!id || seen.has(id)) continue;
      seen.add(id);
      out.push({ id, fallback: track.artworkUrl });
      if (out.length === 4) break;
    }
    return out;
  });
</script>

{#if covers.length === 0}
  <!-- Genuinely-empty (or entirely album-less) playlist: keep the ♫ placeholder. -->
  <span class="collage-empty" style="font-size:{emptyGlyphSize}px">&#9835;</span>
{:else}
  <!-- 1 cover: full-bleed · 2: side-by-side · 3: L-shape · 4: 2x2 (see CSS). -->
  <div class="collage" data-count={covers.length}>
    {#each covers as cover (cover.id)}
      <!-- One positioned cell per tile so a single failed image reveals its own
           muted fill (handleArtworkError shows img.nextElementSibling) without
           reflowing the neighbouring tiles. -->
      <div class="cell">
        <img
          src={cachedArtworkUrl(cover.id)}
          use:resetArtwork={cachedArtworkUrl(cover.id)}
          alt=""
          loading="lazy"
          decoding="async"
          onerror={(e) => handleArtworkError(e, cover.fallback)}
        />
        <span class="cell-fallback" style="display:none"></span>
      </div>
    {/each}
  </div>
{/if}

<style>
  .collage {
    display: grid;
    gap: 1px;
    width: 100%;
    height: 100%;
    /* Every row shares the height evenly so single-row layouts (1 and 2 covers)
       fill the box instead of collapsing to the images' intrinsic height. */
    grid-auto-rows: 1fr;
    /* The 1px gap over a border-coloured backdrop gives Jellyfin-like seams. */
    background: var(--p11-border);
    overflow: hidden;
    border-radius: inherit;
  }

  /* 1 distinct album: a single full-bleed cover (not a stretched 2x2). */
  .collage[data-count='1'] {
    grid-template-columns: 1fr;
  }

  /* 2 distinct albums: two covers side by side. */
  .collage[data-count='2'] {
    grid-template-columns: 1fr 1fr;
  }

  /* 3 distinct albums: an L-shape — the first cover spans the full left column,
     the other two stack on the right (grid auto-placement fills the rest). */
  .collage[data-count='3'] {
    grid-template-columns: 1fr 1fr;
    grid-template-rows: 1fr 1fr;
  }

  .collage[data-count='3'] .cell:first-child {
    grid-row: 1 / span 2;
  }

  /* 4 distinct albums: the classic 2x2 tile. */
  .collage[data-count='4'] {
    grid-template-columns: 1fr 1fr;
    grid-template-rows: 1fr 1fr;
  }

  .cell {
    position: relative;
    overflow: hidden;
  }

  .cell img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  /* Muted fill for a single tile whose art fails to load — avoids a broken-image
     icon while leaving the sibling tiles untouched. */
  .cell-fallback {
    position: absolute;
    inset: 0;
    background: var(--p11-bg-surface);
  }

  .collage-empty {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 100%;
    color: var(--p11-text-muted);
  }
</style>
