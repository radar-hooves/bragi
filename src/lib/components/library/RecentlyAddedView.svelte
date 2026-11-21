<script lang="ts">
  import { onMount } from 'svelte';
  import { getAlbumsTracks } from '#lib/utils/collectionTracks.js';
  import { startPlayback } from '#lib/utils/playback.js';
  import { setCurrentSection, toggleColumnBrowser, getUiState } from '#lib/stores/ui.svelte.js';
  import { getRecentlyAddedState, loadRecentlyAdded } from '#lib/stores/recentlyAdded.svelte.js';
  import { sortRecentlyAdded } from '#lib/utils/sorting.js';
  import { syncColumnBrowserToSelection } from '#lib/stores/columnBrowser.svelte.js';
  import { navigateTo } from '#lib/stores/navigation.svelte.js';
  import type { Album } from '#lib/types/models.js';
  import { Spinner, StatusBar, type StatusBarData } from '#lib/components/common/index.js';
  import AlbumGrid from './AlbumGrid.svelte';

  const recent = $derived(getRecentlyAddedState());
  const ui = $derived(getUiState());
  // The backend already returns date-added order; re-sort only when the user
  // has picked something else, so the default path costs nothing.
  const albums = $derived(sortRecentlyAdded(recent.albums, ui.recentlyAddedSort));
  const recentlyAddedDays = $derived(recent.days);
  // Only show the spinner on a cold load — a warm cache renders immediately
  // while any revalidation happens behind the already-painted grid.
  const isLoading = $derived(recent.isLoading && !recent.isLoaded);
  const error = $derived(recent.error);

  onMount(() => {
    loadRecentlyAdded();
  });

  function retry(): void {
    loadRecentlyAdded(true);
  }

  function handleAlbumClick(album: Album): void {
    // Switch to Songs view filtered to this album
    syncColumnBrowserToSelection({ artist: album.artistName, albumId: album.jellyfinId });
    setCurrentSection('tracks');
    // Show column browser so the user can see the filter context
    if (!getUiState().columnBrowserVisible) {
      toggleColumnBrowser();
    }
    navigateTo({
      section: 'tracks',
      selectedArtistId: null,
      selectedAlbumId: album.jellyfinId,
      columnBrowserVisible: true,
    });
  }

  // Acts on the whole grid selection when the played tile is selected — the
  // same rule AlbumGrid's other consumer (the Albums view, +page.svelte)
  // applies.
  async function handleAlbumsPlay(albums: Album[]): Promise<void> {
    try {
      const tracks = await getAlbumsTracks(albums.map((a) => a.jellyfinId));
      await startPlayback(tracks, 0);
    } catch (e) {
      console.error('Failed to play album:', e);
    }
  }

  const statusData = $derived.by((): StatusBarData | null => {
    if (albums.length === 0) return null;
    return {
      itemCount: albums.length,
      itemLabel: 'album',
      durationMs: albums.reduce((sum, a) => sum + a.durationMs, 0),
      sizeBytes: 0, // Albums don't track file size directly
    };
  });
</script>

<div class="recently-added-view">
  {#if isLoading}
    <div class="loading-state">
      <Spinner size={32} />
      <p>Loading recently added albums...</p>
    </div>
  {:else if error}
    <div class="error-state">
      <p class="error-title">Failed to load albums</p>
      <p class="error-description">{error}</p>
      <button class="retry-btn" onclick={retry}>Try Again</button>
    </div>
  {:else if albums.length === 0}
    <div class="empty-state">
      <p class="empty-title">No recently added albums</p>
      <p class="empty-description">
        Albums added to your library in the last {recentlyAddedDays} days will appear here. You can adjust
        this in Settings &gt; Library.
      </p>
    </div>
  {:else}
    <div class="content">
      <div class="header">
        <span class="subtitle">
          Added in the last {recentlyAddedDays}
          {recentlyAddedDays === 1 ? 'day' : 'days'}
        </span>
      </div>
      <div class="grid-container">
        <AlbumGrid {albums} viewKey="recently-added" sizeView="recentlyAdded" onAlbumClick={handleAlbumClick} onAlbumsPlay={handleAlbumsPlay} />
      </div>
    </div>
  {/if}

  <StatusBar data={statusData} />
</div>

<style>
  .recently-added-view {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .content {
    /* AlbumGrid owns its own scroll (and, above a couple hundred albums, its
       own virtualisation) — this must never also scroll, or the two nested
       overflow containers fight over the wheel/trackpad and AlbumGrid's
       viewport never gets a bounded height to actually window against. */
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .header {
    padding: 0 var(--space-lg) var(--space-md);
  }

  .subtitle {
    font-size: var(--font-size-sm);
    color: var(--p11-text-muted);
  }

  .grid-container {
    flex: 1;
    min-height: 0;
    padding: 0 var(--space-lg);
  }

  .loading-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-md);
    padding: var(--space-3xl);
    color: var(--p11-text-muted);
  }

  .empty-state,
  .error-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: var(--space-3xl) var(--space-xl);
    text-align: center;
  }

  .empty-title,
  .error-title {
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-medium);
    color: var(--p11-text-primary);
    margin: 0 0 var(--space-sm);
  }

  .empty-description,
  .error-description {
    font-size: var(--font-size-md);
    color: var(--p11-text-secondary);
    margin: 0;
    max-width: 400px;
  }

  .error-state {
    gap: var(--space-md);
  }

  .retry-btn {
    margin-top: var(--space-md);
    padding: var(--space-sm) var(--space-lg);
    background-color: var(--p11-accent);
    border: none;
    border-radius: var(--p11-radius-md);
    color: rgb(var(--p11-overlay-rgb));
    font-size: var(--font-size-sm);
    font-family: var(--font-family);
    font-weight: var(--font-weight-medium);
    cursor: pointer;
    transition: background-color var(--transition-fast);
  }

  .retry-btn:hover {
    background-color: var(--p11-accent-hover);
  }
</style>
