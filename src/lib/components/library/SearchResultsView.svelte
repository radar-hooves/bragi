<script lang="ts">
  import { TrackList, AlbumList, ArtistList } from '#lib/components/library/index.js';
  import { getTrackListColumns, getTrackListColumnWidths, setTrackListColumns, setTrackListColumnWidths } from '#lib/stores/ui.svelte.js';
  import type { Album, Artist, Track } from '#lib/types/models.js';

  interface Props {
    searchQuery: string;
    tracks: Track[];
    albums: Album[];
    artists: Artist[];
    onTrackClick: (track: Track) => void;
    onTrackPlay: (track: Track) => void;
    onAddToPlaylist: (tracks: Track[]) => void;
    onArtistTrackPlay: (track: Track, albumTracks: Track[]) => void;
    onAlbumPlay: (album: Album) => void;
    onArtistClick: (artist: Artist) => void;
  }

  let {
    searchQuery,
    tracks,
    albums,
    artists,
    onTrackClick,
    onTrackPlay,
    onAddToPlaylist,
    onArtistTrackPlay,
    onAlbumPlay,
    onArtistClick,
  }: Props = $props();

  // Search's own column layout — separate from Songs, so hiding or
  // resizing a column here never touches the Songs view's layout.
  const columns = $derived(getTrackListColumns('search'));
  const columnWidths = $derived(getTrackListColumnWidths('search'));
  function handleColumnsChange(cols: string[]): void {
    setTrackListColumns('search', cols);
  }
  function handleColumnWidthsChange(widths: Record<string, number>): void {
    setTrackListColumnWidths('search', widths);
  }
</script>

<p class="search-status">
  Showing results for "{searchQuery}"
</p>

<div class="search-results">
  {#if tracks.length > 0}
    <div class="search-section">
      <h3 class="search-section-header">Songs ({tracks.length})</h3>
      <TrackList
        {tracks}
        visibleColumns={columns}
        {columnWidths}
        viewKey="search:tracks"
        {onTrackClick}
        {onTrackPlay}
        onColumnsChange={handleColumnsChange}
        onColumnWidthsChange={handleColumnWidthsChange}
        {onAddToPlaylist}
      />
    </div>
  {/if}
  {#if albums.length > 0}
    <div class="search-section">
      <h3 class="search-section-header">Albums ({albums.length})</h3>
      <AlbumList
        {albums}
        viewKey="search:albums"
        onTrackPlay={onArtistTrackPlay}
        onAlbumPlay={onAlbumPlay}
      />
    </div>
  {/if}
  {#if artists.length > 0}
    <div class="search-section">
      <h3 class="search-section-header">Artists ({artists.length})</h3>
      <ArtistList
        {artists}
        selectedArtistId={null}
        viewKey="search:artists"
        {onArtistClick}
      />
    </div>
  {/if}
  {#if tracks.length === 0 && albums.length === 0 && artists.length === 0}
    <div class="state-container">
      <p class="empty-title">No results found</p>
      <p class="empty-description">Try a different search term.</p>
    </div>
  {/if}
</div>

<style>
  .search-status {
    font-size: var(--font-size-sm);
    color: var(--p11-text-muted);
    margin-bottom: var(--space-md);
    padding: 0 var(--space-lg);
  }

  .search-results {
    display: flex;
    flex-direction: column;
    gap: var(--space-xl);
    padding: 0 var(--space-lg);
  }

  .search-section-header {
    font-size: var(--font-size-md);
    font-weight: var(--font-weight-semibold);
    color: var(--p11-text-secondary);
    margin: 0 0 var(--space-sm) 0;
    padding-bottom: var(--space-xs);
    border-bottom: 1px solid rgb(var(--p11-overlay-rgb) / 0.06);
  }

  .state-container {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: var(--space-3xl) var(--space-xl);
    text-align: center;
    min-height: 300px;
  }

  .empty-title {
    font-size: var(--font-size-xl);
    font-weight: var(--font-weight-semibold);
    color: var(--p11-text-primary);
    margin-bottom: var(--space-sm);
  }

  .empty-description {
    max-width: 400px;
    color: var(--p11-text-secondary);
    line-height: var(--line-height-normal);
  }
</style>
