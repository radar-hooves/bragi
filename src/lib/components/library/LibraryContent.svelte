<script lang="ts">
  import { Spinner, WelcomeScreen } from '#lib/components/common/index.js';
  import {
    AlbumGrid, AlbumList, ArtistView, ArtistGrid, ArtistList,
    ColumnBrowser, GenresView, RecentlyAddedView,
    RecentlyPlayedView, SearchResultsView, TrackList,
  } from '#lib/components/library/index.js';
  import { PlaylistView, SharedPlaylistView } from '#lib/components/playlist/index.js';
  import { SettingsView } from '#lib/components/settings/index.js';
  import { DownloadsView } from '#lib/components/downloads/index.js';
  import type { Album, Artist, Track, JellyfinPlaylistTrack, ImportedPlaylistTrack } from '#lib/types/models.js';
  import {
    getTrackListColumns, getTrackListColumnWidths, setTrackListColumns, setTrackListColumnWidths,
    type ArtistAlbumsSort, type GenresSort, type ViewMode,
  } from '#lib/stores/ui.svelte.js';

  interface Props {
    currentSection: string;
    isLoading: boolean;
    isSyncing: boolean;
    isNotConnected: boolean;
    error: string | null;
    isSearching: boolean;
    libraryEmpty: boolean;
    isShowingSearchResults: boolean;
    searchQuery: string;
    // View modes
    viewMode: ViewMode;
    artistViewMode: ViewMode;
    genreViewMode: ViewMode;
    genresSort: GenresSort;
    columnBrowserVisible: boolean;
    artistAlbumsSort: ArtistAlbumsSort;
    // Data
    displayedAlbums: Album[];
    displayedArtists: Artist[];
    displayedTracks: Track[];
    selectedArtist: Artist | null;
    // Handlers
    onGoToSettings: () => void;
    onLoadLibrary: () => void;
    onAlbumClick: (album: Album) => void;
    /** AlbumList / SearchResultsView — no selection concept, always one album. */
    onAlbumPlay: (album: Album) => void;
    /** AlbumGrid — the whole grid selection when the acted-on tile is selected. */
    onAlbumsPlay: (albums: Album[]) => void;
    onAlbumQueue: (albums: Album[], position: 'next' | 'end') => void;
    onAlbumAddToPlaylist: (albums: Album[]) => void;
    onArtistClick: (artist: Artist) => void;
    onArtistTilePlay: (artists: Artist[]) => void;
    onArtistTileQueue: (artists: Artist[], position: 'next' | 'end') => void;
    onArtistTileAddToPlaylist: (artists: Artist[]) => void;
    onArtistTrackPlay: (track: Track, albumTracks: Track[]) => void;
    onArtistPlayAll: (tracks: Track[]) => void;
    onArtistShuffleAll: (tracks: Track[]) => void;
    onTrackClick: (track: Track) => void;
    onTrackPlay: (track: Track) => void;
    onPlaylistTrackPlay: (track: JellyfinPlaylistTrack) => void;
    onPlaylistShuffleAll: () => void;
    onSharedPlaylistTrackPlay: (track: ImportedPlaylistTrack) => void;
    onSharedPlaylistShuffleAll: () => void;
    onAddToPlaylist: (tracks: Track[]) => void;
    onGenrePlay: (genre: string) => void;
    onGenreShuffle: (genre: string) => void;
    onColumnArtistPlay: (artist: string) => void;
    onColumnAlbumPlay: (albumId: string) => void;
  }

  let {
    currentSection, isLoading, isSyncing, isNotConnected, error, isSearching,
    libraryEmpty, isShowingSearchResults, searchQuery,
    viewMode, artistViewMode, genreViewMode, genresSort, columnBrowserVisible, artistAlbumsSort,
    displayedAlbums, displayedArtists, displayedTracks, selectedArtist,
    onGoToSettings, onLoadLibrary, onAlbumClick, onAlbumPlay, onAlbumsPlay, onAlbumQueue, onAlbumAddToPlaylist,
    onArtistClick, onArtistTilePlay, onArtistTileQueue, onArtistTileAddToPlaylist,
    onArtistTrackPlay, onArtistPlayAll, onArtistShuffleAll,
    onTrackClick, onTrackPlay, onPlaylistTrackPlay, onPlaylistShuffleAll,
    onSharedPlaylistTrackPlay, onSharedPlaylistShuffleAll, onAddToPlaylist,
    onGenrePlay, onGenreShuffle, onColumnArtistPlay, onColumnAlbumPlay,
  }: Props = $props();

  // Songs is a TrackList consumer inline here (not a dedicated sub-view like
  // RecentlyPlayedView), but its columns are still its own view's layout
  // state — same per-view store, self-fetched the same way.
  const songsColumns = $derived(getTrackListColumns('songs'));
  const songsColumnWidths = $derived(getTrackListColumnWidths('songs'));
  function handleSongsColumnsChange(cols: string[]): void {
    setTrackListColumns('songs', cols);
  }
  function handleSongsColumnWidthsChange(widths: Record<string, number>): void {
    setTrackListColumnWidths('songs', widths);
  }
</script>

{#if currentSection === 'settings'}
  <SettingsView />
{:else if currentSection === 'playlists'}
  <PlaylistView onTrackPlay={onPlaylistTrackPlay} onShuffleAll={onPlaylistShuffleAll} {onAddToPlaylist} />
{:else if currentSection === 'sharedPlaylists'}
  <SharedPlaylistView onTrackPlay={onSharedPlaylistTrackPlay} onShuffleAll={onSharedPlaylistShuffleAll} />
{:else if isLoading}
  <div class="state-container"><Spinner size={48} /><p class="state-text">Loading library...</p></div>
{:else if isNotConnected}
  <WelcomeScreen {onGoToSettings} />
{:else if error}
  <div class="state-container">
    <p class="error-message">{error}</p>
    <button class="retry-btn" onclick={onLoadLibrary}>Try again</button>
  </div>
{:else if isSearching}
  <div class="state-container"><Spinner size={32} /><p class="state-text">Searching...</p></div>
{:else if libraryEmpty && isSyncing}
  <div class="state-container">
    <Spinner size={48} />
    <p class="state-text">Setting up your library…</p>
    <p class="state-subtext">Syncing from your media server. This can take a minute for large libraries.</p>
  </div>
{:else if libraryEmpty}
  <div class="state-container">
    <p class="empty-title">Your library is empty</p>
    <p class="empty-description">No music found on your media server. Add some music and refresh.</p>
    <button class="retry-btn" onclick={onLoadLibrary}>Refresh</button>
  </div>
{:else if isShowingSearchResults && searchQuery}
  <SearchResultsView
    {searchQuery}
    tracks={displayedTracks}
    albums={displayedAlbums}
    artists={displayedArtists}
    onTrackClick={onTrackClick}
    onTrackPlay={onTrackPlay}
    onAddToPlaylist={onAddToPlaylist}
    onArtistTrackPlay={onArtistTrackPlay}
    onAlbumPlay={onAlbumPlay}
    onArtistClick={onArtistClick}
  />
{:else if currentSection === 'albums'}
  <div class="section-padded">
    {#if viewMode === 'grid'}
      <AlbumGrid
        albums={displayedAlbums}
        onAlbumClick={onAlbumClick}
        onAlbumsPlay={onAlbumsPlay}
        onAlbumQueue={onAlbumQueue}
        onAlbumAddToPlaylist={onAlbumAddToPlaylist}
      />
    {:else}
      <AlbumList albums={displayedAlbums} onTrackPlay={onArtistTrackPlay} onAlbumPlay={onAlbumPlay} />
    {/if}
  </div>
{:else if currentSection === 'artists'}
  {#if artistViewMode === 'grid' && selectedArtist}
    <ArtistView artist={selectedArtist} albumSort={artistAlbumsSort}
      onTrackPlay={onArtistTrackPlay} onPlayAll={onArtistPlayAll}
      onShuffleAll={onArtistShuffleAll} onAddToPlaylist={onAddToPlaylist} />
  {:else if artistViewMode === 'grid'}
    <div class="section-padded">
      <ArtistGrid
        artists={displayedArtists}
        onArtistClick={onArtistClick}
        onArtistPlay={onArtistTilePlay}
        onArtistQueue={onArtistTileQueue}
        onArtistAddToPlaylist={onArtistTileAddToPlaylist}
      />
    </div>
  {:else}
    <div class="artist-split-pane">
      <div class="artist-list-panel">
        <ArtistList artists={displayedArtists} selectedArtistId={selectedArtist?.jellyfinId ?? null} onArtistClick={onArtistClick} />
      </div>
      <div class="artist-detail-panel">
        <ArtistView artist={selectedArtist}
          onTrackPlay={onArtistTrackPlay} onPlayAll={onArtistPlayAll}
          onShuffleAll={onArtistShuffleAll} onAddToPlaylist={onAddToPlaylist} />
      </div>
    </div>
  {/if}
{:else if currentSection === 'tracks'}
  <div class="tracks-layout">
    {#if columnBrowserVisible}
      <div class="tracks-browser">
        <ColumnBrowser visible onGenrePlay={onGenrePlay} onArtistPlay={onColumnArtistPlay} onAlbumPlay={onColumnAlbumPlay} />
      </div>
    {/if}
    <div class="tracks-scroll">
      <TrackList tracks={displayedTracks} visibleColumns={songsColumns}
        columnWidths={songsColumnWidths} viewKey="tracks" onTrackClick={onTrackClick}
        onTrackPlay={onTrackPlay} onColumnsChange={handleSongsColumnsChange}
        onColumnWidthsChange={handleSongsColumnWidthsChange} onAddToPlaylist={onAddToPlaylist} />
    </div>
  </div>
{:else if currentSection === 'genres'}
  <GenresView viewMode={genreViewMode} sort={genresSort} onShuffle={onGenreShuffle} />
{:else if currentSection === 'recentlyAdded'}
  <RecentlyAddedView />
{:else if currentSection === 'recentlyPlayed'}
  <RecentlyPlayedView />
{:else if currentSection === 'queue'}
  <DownloadsView />
{/if}

<style>
  /* Wraps AlbumGrid, ArtistGrid or AlbumList, all of which own their own
     scroll (AlbumGrid/ArtistGrid also their own virtualisation) — this must
     never also scroll, or the nested overflow containers fight over the
     wheel/trackpad, which is what made the columns "look like a party disco"
     when scrolling in the first place. */
  .section-padded { padding: 0 var(--space-md); height: 100%; overflow: hidden; }

  .artist-split-pane { display: flex; height: 100%; gap: var(--space-md); }
  .artist-list-panel { width: clamp(9rem, 35%, 250px); flex-shrink: 0; overflow: hidden; }
  /* ArtistView's own .albums-content scrolls itself — this must not
     also scroll, or the nested overflow containers fight over the wheel. */
  .artist-detail-panel { flex: 1; min-width: 0; overflow: hidden; }

  .tracks-layout { display: flex; flex-direction: column; height: 100%; }
  .tracks-browser { display: flex; flex-direction: column; flex: 0 1 auto; min-height: 0; max-height: 60%; }
  .tracks-browser :global(.column-browser) { min-height: 0; }
  .tracks-scroll { flex: 1; overflow-y: auto; min-height: 0; }

  .state-container {
    display: flex; flex-direction: column; align-items: center; justify-content: center;
    padding: var(--space-3xl) var(--space-xl); text-align: center; min-height: 300px;
  }
  .state-text { margin-top: var(--space-lg); color: var(--p11-text-secondary); }
  .state-subtext { margin-top: var(--space-sm); max-width: 360px; color: var(--p11-text-muted); font-size: var(--font-size-sm); line-height: var(--line-height-normal); }
  .error-message { color: var(--p11-error); margin-bottom: var(--space-lg); }

  .retry-btn {
    padding: var(--space-sm) var(--space-lg);
    background-color: rgb(var(--p11-overlay-rgb) / 0.08);
    border: 1px solid rgb(var(--p11-overlay-rgb) / 0.1);
    border-radius: var(--radius-full);
    color: var(--p11-text-primary);
    font-size: var(--font-size-md); font-family: var(--font-family);
    cursor: pointer;
    transition: background-color var(--transition-fast), border-color var(--transition-fast);
  }
  .retry-btn:hover { background-color: rgb(var(--p11-overlay-rgb) / 0.12); border-color: rgb(var(--p11-overlay-rgb) / 0.15); }

  .empty-title { font-size: var(--font-size-xl); font-weight: var(--font-weight-semibold); color: var(--p11-text-primary); margin-bottom: var(--space-sm); }
  .empty-description { max-width: 400px; color: var(--p11-text-secondary); line-height: var(--line-height-normal); }
</style>
