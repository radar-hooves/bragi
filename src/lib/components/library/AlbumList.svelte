<script lang="ts">
  import type { Album, Track } from '#lib/types/models.js';
  import { getAlbumTracks, getCachedAlbumTracks } from '#lib/services/tauri.js';
  import { compareTrackOrder } from '#lib/utils/ordering.js';
  import AlbumListItem from './AlbumListItem.svelte';
  import { bindScrollMemory } from '#lib/utils/scrollMemory.svelte.js';
  import { getGridTileSize, gridTileSizeToListArtworkSize } from '#lib/stores/ui.svelte.js';

  interface Props {
    albums: Album[];
    /** Keys this list's remembered scroll position. */
    viewKey?: string;
    onTrackPlay?: (track: Track, albumTracks: Track[]) => void;
    onAlbumPlay?: (album: Album) => void;
  }

  let { albums, viewKey = 'albums-list', onTrackPlay, onAlbumPlay }: Props = $props();

  // Each row expands in place to show its tracks, so row height is inherently
  // dynamic — not a fit for the shared virtualiser's fixed/measured-row model.
  // This owns its own scroll and remembers position; virtualising it
  // is a separate, larger problem than this item's scope.
  let listEl = $state<HTMLDivElement>();
  $effect(() => bindScrollMemory(viewKey, listEl));

  let expandedAlbumId = $state<string | null>(null);
  let albumTracks = $state<Map<string, Track[]>>(new Map());
  let loadingAlbumIds = $state<Set<string>>(new Set());

  // Deliberately does NOT drive the column browser. Expanding a row here
  // shows the album's tracks in place — it is not navigation, and the column
  // browser belongs to the Songs view. Selecting from here left a filter behind
  // that the user then met in Songs with nothing on screen explaining it.
  function toggleAlbum(album: Album): void {
    if (expandedAlbumId === album.jellyfinId) {
      expandedAlbumId = null;
    } else {
      expandedAlbumId = album.jellyfinId;
      if (!albumTracks.has(album.jellyfinId)) {
        loadTracksForAlbum(album.jellyfinId);
      }
    }
  }

  function sortTracks(tracks: Track[]): Track[] {
    return [...tracks].sort(compareTrackOrder);
  }

  async function loadTracksForAlbum(albumId: string): Promise<void> {
    loadingAlbumIds = new Set(loadingAlbumIds).add(albumId);

    try {
      // Try cache first
      const cached = await getCachedAlbumTracks(albumId);
      if (cached.length > 0) {
        albumTracks = new Map(albumTracks).set(albumId, sortTracks(cached));
        loadingAlbumIds = new Set([...loadingAlbumIds].filter((id) => id !== albumId));
      }

      // Background refresh from Jellyfin
      const tracks = await getAlbumTracks(albumId);
      albumTracks = new Map(albumTracks).set(albumId, sortTracks(tracks));
    } catch (err) {
      console.error(`Failed to load tracks for album ${albumId}:`, err);
    } finally {
      loadingAlbumIds = new Set([...loadingAlbumIds].filter((id) => id !== albumId));
    }
  }

  function getDiscNumbers(tracks: Track[]): number[] {
    const discs = new Set(tracks.map((t) => t.discNumber));
    return [...discs].sort((a, b) => a - b);
  }

  function getTracksByDisc(tracks: Track[]): Map<number, Track[]> {
    const map = new Map<number, Track[]>();
    for (const track of tracks) {
      const disc = track.discNumber;
      if (!map.has(disc)) map.set(disc, []);
      map.get(disc)!.push(track);
    }
    return map;
  }
</script>

{#if albums.length === 0}
  <div class="empty-state">
    <p class="empty-title">No albums found</p>
    <p class="empty-description">Your album library is empty.</p>
  </div>
{:else}
  <div
    class="album-list"
    style:--list-artwork-size="{gridTileSizeToListArtworkSize(getGridTileSize('albums'))}px"
    bind:this={listEl}
  >
    {#each albums as album (album.jellyfinId)}
      {@const tracks = albumTracks.get(album.jellyfinId) ?? []}
      {@const discNumbers = getDiscNumbers(tracks)}
      <AlbumListItem
        {album}
        isExpanded={expandedAlbumId === album.jellyfinId}
        {tracks}
        isLoading={loadingAlbumIds.has(album.jellyfinId)}
        {discNumbers}
        tracksByDisc={getTracksByDisc(tracks)}
        hasMultipleDiscs={discNumbers.length > 1}
        onToggle={toggleAlbum}
        {onAlbumPlay}
        {onTrackPlay}
      />
    {/each}
  </div>
{/if}

<style>
  .album-list {
    display: flex;
    flex-direction: column;
    /* height:100% + overflow-y:auto only engages as a real scroll container
       when an ancestor gives it a bounded height (LibraryContent's albums
       section); embedded in a naturally-flowing page (search results) this
       stays auto-height and the outer page scrolls instead, unchanged. */
    height: 100%;
    overflow-y: auto;
    overscroll-behavior: contain;
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
