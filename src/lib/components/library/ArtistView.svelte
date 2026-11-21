<!--
  ArtistView.svelte

  Artist detail: backdrop image, name with stats, action buttons and the album
  sections. Fills the page when an artist is opened from the grid view, and the
  right pane beside the artist list in list view, where `artist` is null until
  one is selected.
-->
<script lang="ts">
  import type { Artist, Album, Track } from '#lib/types/models.js';
  import { cachedArtworkUrl, handleArtworkError, resetArtwork } from '#lib/utils/artworkFallback.js';
  import {
    getArtistAlbums,
    getAlbumTracks,
    getCachedArtistAlbums,
    getCachedAlbumTracks,
  } from '#lib/services/tauri.js';
  import { getLibraryState } from '#lib/stores/library.svelte.js';
  import { compareArtistAlbumOrder, compareFolded, compareTrackOrder } from '#lib/utils/ordering.js';
  import { Spinner } from '#lib/components/common/index.js';
  import { bindScrollMemory } from '#lib/utils/scrollMemory.svelte.js';
  import ArtistAlbumSection from './ArtistAlbumSection.svelte';
  import ArtistHeader from './ArtistHeader.svelte';

  export type AlbumSortOption = 'year-desc' | 'year-asc' | 'name-asc' | 'name-desc';

  interface Props {
    artist: Artist | null;
    albumSort?: AlbumSortOption;
    onTrackPlay?: (track: Track, albumTracks: Track[]) => void;
    onPlayAll?: (tracks: Track[]) => void;
    onShuffleAll?: (tracks: Track[]) => void;
    onAddToPlaylist?: (tracks: Track[]) => void;
  }

  let {
    artist,
    albumSort = 'year-desc',
    onTrackPlay,
    onPlayAll,
    onShuffleAll,
    onAddToPlaylist,
  }: Props = $props();

  // Keyed by artist id, so scroll position resumes if you come back to this
  // artist and starts fresh for a different one.
  let albumsContentEl = $state<HTMLDivElement>();
  $effect(() => bindScrollMemory(
    artist ? `artist:${artist.jellyfinId}` : 'artist', albumsContentEl,
    !isLoadingAlbums && loadingTrackAlbumIds.size === 0
  ));

  let albums = $state<Album[]>([]);
  let albumTracks = $state<Map<string, Track[]>>(new Map());
  let isLoadingAlbums = $state(true);
  let loadingTrackAlbumIds = $state<Set<string>>(new Set());
  let failedTrackAlbumIds = $state<Set<string>>(new Set());

  const heroBackgroundSrc = $derived(artist?.backdropUrl ?? artist?.artworkUrl ?? null);
  const hasBackdrop = $derived(!!artist?.backdropUrl);

  // Sync download counts from the library store into our local albums.
  // The store gets updated by markTrackDownloaded/markTrackRemoved when
  // downloads complete, but our local albums are a separate copy from
  // the Jellyfin fetch. This effect merges the live counts in.
  $effect(() => {
    const storeAlbums = getLibraryState().albums;
    if (albums.length === 0 || storeAlbums.length === 0) return;

    const countMap = new Map<string, number>();
    for (const sa of storeAlbums) {
      if (sa.downloadedTrackCount > 0) {
        countMap.set(sa.jellyfinId, sa.downloadedTrackCount);
      }
    }

    let changed = false;
    for (const album of albums) {
      const storeCount = countMap.get(album.jellyfinId) ?? 0;
      if (album.downloadedTrackCount !== storeCount) {
        album.downloadedTrackCount = storeCount;
        changed = true;
      }
    }
    if (changed) {
      albums = [...albums];
    }
  });

  const totalTracks = $derived(albums.reduce((sum, a) => sum + a.trackCount, 0));
  const downloadedTracks = $derived(albums.reduce((sum, a) => sum + a.downloadedTrackCount, 0));

  /** All tracks across all albums, sorted by album year then disc/track number */
  const allTracks = $derived.by(() => {
    const tracks: Track[] = [];
    for (const album of albums) {
      const albumTrackList = albumTracks.get(album.jellyfinId) ?? [];
      tracks.push(...albumTrackList);
    }
    return tracks;
  });

  function handlePlayAll(): void {
    if (allTracks.length > 0) {
      onPlayAll?.(allTracks);
    }
  }

  function handleShuffleAll(): void {
    if (allTracks.length > 0) {
      onShuffleAll?.(allTracks);
    }
  }

  const sortedAlbums = $derived.by(() => {
    return [...albums].sort((a, b) => {
      switch (albumSort) {
        case 'year-desc':
          return compareArtistAlbumOrder(a, b);
        case 'year-asc':
          // Ascending year, but the missing-year-sorts-last and name tie-break
          // stay the same direction as year-desc — only the year comparison flips.
          return (a.year ?? Infinity) - (b.year ?? Infinity) || compareFolded(a.name, b.name);
        case 'name-asc':
          return compareFolded(a.name, b.name);
        case 'name-desc':
          return compareFolded(b.name, a.name);
        default:
          return 0;
      }
    });
  });

  function sortTracks(tracks: Track[]): Track[] {
    return [...tracks].sort(compareTrackOrder);
  }

  function loadTracksForAlbums(albumList: Album[], version: number): void {
    const trackLoadIds = new Set(albumList.map((a) => a.jellyfinId));
    loadingTrackAlbumIds = trackLoadIds;

    for (const album of albumList) {
      loadAlbumTracks(album, trackLoadIds, version);
    }
  }

  async function loadAlbumTracks(
    album: Album,
    trackLoadIds: Set<string>,
    version: number
  ): Promise<void> {
    const albumId = album.jellyfinId;
    let hasCachedData = false;

    // Phase 1: load from SQLite cache for instant display
    try {
      const cached = await getCachedAlbumTracks(albumId);
      if (loadVersion !== version) return;
      if (cached.length > 0) {
        albumTracks = new Map(albumTracks).set(albumId, sortTracks(cached));
        hasCachedData = true;
        // Show cached data immediately by removing from loading set
        trackLoadIds.delete(albumId);
        loadingTrackAlbumIds = new Set(trackLoadIds);
      }
    } catch {
      // Cache miss is fine — live fetch will handle it
    }

    if (loadVersion !== version) return;

    // Phase 2: background refresh from Jellyfin
    try {
      const tracks = await getAlbumTracks(albumId);
      if (loadVersion !== version) return;
      albumTracks = new Map(albumTracks).set(albumId, sortTracks(tracks));
    } catch (err) {
      if (loadVersion !== version) return;
      if (!hasCachedData) {
        // Both cache and Jellyfin failed — mark as unavailable
        console.error(`Failed to load tracks for ${album.name}:`, err);
        failedTrackAlbumIds = new Set(failedTrackAlbumIds).add(albumId);
      }
    } finally {
      if (loadVersion === version) {
        trackLoadIds.delete(albumId);
        loadingTrackAlbumIds = new Set(trackLoadIds);
      }
    }
  }

  // Non-reactive version counter to guard against stale async responses.
  // Must NOT be $state to avoid re-triggering the $effect.
  let loadVersion = 0;

  $effect(() => {
    const currentArtist = artist;
    if (!currentArtist) {
      albums = [];
      albumTracks = new Map();
      isLoadingAlbums = false;
      loadingTrackAlbumIds = new Set();
      failedTrackAlbumIds = new Set();
      return;
    }

    loadVersion += 1;
    const thisVersion = loadVersion;
    isLoadingAlbums = true;
    albums = [];
    albumTracks = new Map();
    loadingTrackAlbumIds = new Set();
    failedTrackAlbumIds = new Set();

    const artistId = currentArtist.jellyfinId;

    // Cache-first: load from SQLite immediately, then refresh from Jellyfin
    getCachedArtistAlbums(artistId)
      .then((cachedAlbums) => {
        if (loadVersion !== thisVersion) return;
        if (cachedAlbums.length > 0) {
          albums = cachedAlbums;
          isLoadingAlbums = false;
          loadTracksForAlbums(cachedAlbums, thisVersion);
        }
      })
      .catch(() => {
        // Cache miss is fine — live fetch will handle it
      })
      .finally(() => {
        // Background refresh from Jellyfin
        getArtistAlbums(artistId)
          .then((fetchedAlbums) => {
            if (loadVersion !== thisVersion) return;
            albums = fetchedAlbums;
            isLoadingAlbums = false;
            loadTracksForAlbums(fetchedAlbums, thisVersion);
          })
          .catch((err) => {
            if (loadVersion !== thisVersion) return;
            console.error('Failed to load artist albums:', err);
            // Only show error state if cache didn't already provide data
            if (albums.length === 0) {
              isLoadingAlbums = false;
            }
          });
      });
  });
</script>

{#if !artist}
  <div class="empty-state">
    <p class="empty-text">Select an artist to view their albums</p>
  </div>
{:else}
  <div class="artist-page">
    <!-- Backdrop extends behind both hero and albums -->
    <div class="page-backdrop">
      {#if heroBackgroundSrc}
        <img
          src={heroBackgroundSrc}
          use:resetArtwork={heroBackgroundSrc}
          alt=""
          class="backdrop-image"
          class:has-backdrop={hasBackdrop}
          onerror={(e) => handleArtworkError(e, cachedArtworkUrl(artist.jellyfinId))}
        />
      {/if}
      <div class="backdrop-overlay"></div>
    </div>

    {#key artist.jellyfinId}
      <ArtistHeader
        name={artist.name}
        albumIds={albums.map((a) => a.jellyfinId)}
        {totalTracks}
        {downloadedTracks}
        hasTracks={allTracks.length > 0}
        onPlayAll={handlePlayAll}
        onShuffleAll={handleShuffleAll}
      />
    {/key}

    <div class="albums-content" bind:this={albumsContentEl}>
      {#if isLoadingAlbums}
        <div class="loading-state">
          <Spinner size={32} />
        </div>
      {:else if albums.length === 0}
        <div class="empty-state">
          <p class="empty-text">No albums found</p>
        </div>
      {:else}
        <div class="albums-list">
          {#each sortedAlbums as album (album.jellyfinId)}
            <ArtistAlbumSection
              {album}
              tracks={albumTracks.get(album.jellyfinId) ?? []}
              isLoadingTracks={loadingTrackAlbumIds.has(album.jellyfinId)}
              loadFailed={failedTrackAlbumIds.has(album.jellyfinId)}
              {onTrackPlay}
              {onAddToPlaylist}
            />
          {/each}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .artist-page {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  /* Backdrop covers both hero and bleeds into albums */
  .page-backdrop {
    position: absolute;
    inset: 0;
    overflow: hidden;
    pointer-events: none;
    z-index: 0;
  }

  .backdrop-image {
    position: absolute;
    top: 0;
    left: 50%;
    transform: translateX(-50%);
    max-width: 100%;
    max-height: 100%;
    filter: blur(4px);
    opacity: 0.25;
    /* The box is the picture itself (not the panel), so the mask feathers
       wherever the image really ends, in proportion at any window size. */
    mask-image:
      linear-gradient(to right, transparent, var(--p11-bg-primary) 20%, var(--p11-bg-primary) 80%, transparent),
      linear-gradient(to bottom, var(--p11-bg-primary) 50%, transparent);
    mask-composite: intersect;
  }

  .backdrop-image.has-backdrop {
    filter: none;
    opacity: 0.35;
  }

  .backdrop-overlay {
    position: absolute;
    inset: 0;
    background: linear-gradient(
      to bottom,
      rgba(var(--p11-bg-primary-rgb, 18, 18, 18), 0.2) 0%,
      rgba(var(--p11-bg-primary-rgb, 18, 18, 18), 0.6) 70%,
      var(--p11-bg-primary) 100%
    );
  }

  .albums-content {
    position: relative;
    z-index: 1;
    flex: 1;
    overflow-y: auto;
    padding: var(--space-xl);
  }

  .albums-list {
    display: flex;
    flex-direction: column;
  }

  .loading-state,
  .empty-state {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 200px;
  }

  .empty-text {
    font-size: var(--font-size-md);
    color: var(--p11-text-muted);
  }
</style>
