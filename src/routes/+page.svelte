<script lang="ts">
  import { untrack } from 'svelte';
  import {
    getUiState, requestSettingsCategory, clearSearch, setViewMode, setArtistViewMode, setGenreViewMode, setCurrentSection,
    toggleColumnBrowser,
    setAlbumsSort, setArtistsSort, setGenresSort, setArtistAlbumsSort, setTracksSort, setRecentlyAddedSort,
    getGridTileSize, setGridTileSize,
    MIN_GRID_TILE_SIZE, MAX_GRID_TILE_SIZE, GRID_TILE_SIZE_STEP,
    type ViewMode, type AlbumsSort, type ArtistsSort, type GenresSort, type ArtistAlbumsSort, type TracksSort,
    type RecentlyAddedSort, type GridViewId,
  } from '#lib/stores/ui.svelte.js';
  import {
    getLibraryState, setLibraryLoading, beginSyncing, endSyncing, setAlbums, setArtists, setTracks,
    getOfflineFilteredAlbums, getOfflineFilteredArtists, getLibraryVersion,
  } from '#lib/stores/library.svelte.js';
  import { getFilteredTracks, refreshLibraryView, getColumnBrowserState, syncColumnBrowserToSelection } from '#lib/stores/columnBrowser.svelte.js';
  import { getOfflineMode } from '#lib/stores/offlineMode.svelte.js';
  import { invalidateRecentlyAdded } from '#lib/stores/recentlyAdded.svelte.js';
  import { getJellyfinPlaylistState } from '#lib/stores/jellyfinPlaylist.svelte.js';
  import { getImportedPlaylistState } from '#lib/stores/importedPlaylists.svelte.js';
  import { getIsJellyfinConnected, getActiveBackend, getConnectionGeneration } from '#lib/stores/jellyfin.svelte.js';
  import {
    navigateTo, navigateBack, navigateForward, getCanGoBack, getCanGoForward, initNavigation,
  } from '#lib/stores/navigation.svelte.js';

  import {
    getLibraryTracks, getCachedAlbums, getCachedArtists,
    getJellyfinStatus, syncLibrary,
  } from '#lib/services/tauri.js';
  import { createSearchSession } from '#lib/utils/searchSession.svelte.js';
  import type { RankedSearchResults } from '#lib/utils/searchRanking.js';

  import { StatusBar, type StatusBarData } from '#lib/components/common/index.js';
  import { LibraryContent } from '#lib/components/library/index.js';
  import { LibraryToolbar } from '#lib/components/layout/index.js';
  import { AddToJellyfinPlaylistDialog, AddDuplicateTracksDialog } from '#lib/components/playlist/index.js';

  import {
    sortAlbums, sortArtists, sortTracks,
    albumsSortOptions, artistsSortOptions, genresSortOptions, artistAlbumsSortOptions, tracksSortOptions,
    recentlyAddedSortOptions,
  } from '#lib/utils/sorting.js';
  import { startPlayback, playTrackFromList, shuffleAndPlay } from '#lib/utils/playback.js';
  import { queueTrackNext, queueTrackAtEnd } from '#lib/utils/queueActions.js';
  import { getAlbumsTracks, getArtistsTracks } from '#lib/utils/collectionTracks.js';
  import { tracksFooterData, albumsFooterData, artistsFooterData } from '#lib/utils/footer.js';
  import { jellyfinPlaylistTrackToTrack } from '#lib/utils/jellyfinPlaylist.js';
  import { importedPlaylistTrackToTrack } from '#lib/utils/importedPlaylist.js';
  import { playGenre, shuffleGenre, playColumnArtist, playColumnAlbum } from '#lib/utils/columnBrowserPlayback.js';
  import type { Album, Artist, Track } from '#lib/types/models.js';

  let showAddToJellyfinPlaylistDialog = $state(false);
  let tracksToAddToPlaylist = $state<Track[]>([]);
  let selectedArtist = $state<Artist | null>(null);
  let isExplicitlySelectingArtist = $state(false);
  const search = createSearchSession(() => ({ isSyncing: lib.isSyncing }));
  let isNotConnected = $state(false);

  const sectionTitles: Record<string, string> = {
    albums: 'Albums', artists: 'Artists', tracks: 'Songs', genres: 'Genres',
    recentlyAdded: 'Recently Added', recentlyPlayed: 'Recently Played',
    queue: 'Download Queue', playlists: 'Playlists', settings: 'Settings',
  };
  const currentSortOptions = $derived.by(() => {
    // Search results are always relevance-ranked (Apple Music/Spotify convention) — neither offers a sort control over a search result, so
    // showing the last-active section's dropdown here would let it silently
    // discard that ranking.
    if (isShowingSearchResults) return [];
    const s = ui.currentSection;
    if (s === 'albums') return albumsSortOptions;
    if (s === 'artists') return selectedArtist ? artistAlbumsSortOptions : artistsSortOptions;
    if (s === 'recentlyAdded') return recentlyAddedSortOptions;
    // The genre LIST sorts from its own column headers, so the toolbar
    // dropdown would be inert there — hide it rather than show a dead control.
    if (s === 'genres') return ui.genreViewMode === 'grid' ? genresSortOptions : [];
    return s === 'tracks' ? tracksSortOptions : [];
  });
  const currentSort = $derived.by(() => {
    const s = ui.currentSection;
    if (s === 'albums') return ui.albumsSort;
    if (s === 'artists') return selectedArtist ? ui.artistAlbumsSort : ui.artistsSort;
    if (s === 'recentlyAdded') return ui.recentlyAddedSort;
    if (s === 'genres') return ui.genresSort;
    return s === 'tracks' ? ui.tracksSort : '';
  });
  function handleSortChange(event: Event): void {
    const v = (event.target as HTMLSelectElement).value;
    const s = ui.currentSection;
    if (s === 'albums') setAlbumsSort(v as AlbumsSort);
    else if (s === 'artists') { selectedArtist ? setArtistAlbumsSort(v as ArtistAlbumsSort) : setArtistsSort(v as ArtistsSort); }
    else if (s === 'tracks') setTracksSort(v as TracksSort);
    else if (s === 'recentlyAdded') setRecentlyAddedSort(v as RecentlyAddedSort);
    else if (s === 'genres') setGenresSort(v as GenresSort);
  }

  const isShowingSearchResults = $derived(search.results !== null);
  // Consolidated state references: read once per render, not 34+ times
  const ui = $derived(getUiState());
  const lib = $derived(getLibraryState());
  const currentSection = $derived(ui.currentSection);

  // Only compute data for the active section (or search results).
  // Previously all three (albums, artists, tracks) were computed on every
  // state change regardless of which view was visible, causing unnecessary
  // sorting/filtering of 3,800+ items on view transitions.
  // Search results keep the relevance order the search session already
  // computed (exact match, then prefix, then a mid-string hit) rather than being re-sorted by whatever library sort was last
  // active; there is no sort control over a search result (see
  // currentSortOptions above), so nothing else can reorder them.
  const displayedAlbums = $derived.by(() => {
    if (isShowingSearchResults) return search.results!.albums;
    if (currentSection !== 'albums') return [];
    return sortAlbums(getOfflineFilteredAlbums(), ui.albumsSort);
  });
  const displayedArtists = $derived.by(() => {
    if (isShowingSearchResults) return search.results!.artists;
    if (currentSection !== 'artists') return [];
    return sortArtists(getOfflineFilteredArtists(), ui.artistsSort);
  });
  const displayedTracks = $derived.by(() => {
    if (isShowingSearchResults) return search.results!.tracks;
    if (currentSection !== 'tracks') return [];
    return getFilteredTracks();
  });

  const librarySections = new Set(['albums', 'artists', 'tracks', 'genres', 'recentlyAdded', 'recentlyPlayed']);
  const showToolbar = $derived(librarySections.has(ui.currentSection) && !isNotConnected && !lib.isLoading);
  let libraryLoaded = $state(false);
  // Backends whose library we've loaded+synced at least once this session.
  // A backend not in this set triggers a one-time sync on first view, so a
  // freshly-connected or toggled-to backend (whose cache is still empty) isn't
  // shown blank until the ~24h scheduler fires. Keyed by backend id.
  const syncedBackends = new Set<string>();

  async function loadLibrary(): Promise<void> {
    setLibraryLoading(true);
    isNotConnected = false;
    try {
      const status = await getJellyfinStatus();
      if (!status.connected) { isNotConnected = true; setLibraryLoading(false); return; }
      // Cache-first: load from SQLite instantly (scoped to the active backend).
      const [ca, cr, ct] = await Promise.all([getCachedAlbums(), getCachedArtists(), getLibraryTracks()]);
      // First view of THIS backend (not just the first load overall) syncs, so a
      // newly-connected/toggled backend populates instead of showing an empty cache.
      const activeBackend = status.serverType ?? 'jellyfin';
      const isFirstLoad = !syncedBackends.has(activeBackend);
      const cacheWasEmpty = ca.length === 0 && cr.length === 0;
      syncedBackends.add(activeBackend);
      libraryLoaded = true;
      setAlbums(ca); setArtists(cr); setTracks(ct);
      // Only sync on a backend's first view, not on every $effect re-run.
      // The sync scheduler handles periodic refreshes (default 24h).
      if (isFirstLoad) {
        // When the backend's cache is still empty (first connect, or a toggle to
        // a backend the migration couldn't pre-populate), show a "Setting up your
        // library…" state during the sync instead of a blank/empty grid.
        // begin/endSyncing are reference-counted and paired 1:1 here.
        const trackSync = cacheWasEmpty;
        if (trackSync) beginSyncing();
        syncLibrary().then(async () => {
          const [fa, fr, ft] = await Promise.all([getCachedAlbums(), getCachedArtists(), getLibraryTracks()]);
          setAlbums(fa); setArtists(fr); setTracks(ft);
          // A sync is the only thing that changes what "recently added" holds.
          invalidateRecentlyAdded();
        }).catch((e) => console.warn('Background library sync failed:', e))
          .finally(() => { if (trackSync) endSyncing(); });
      }
    } catch { isNotConnected = true; setLibraryLoading(false); }
  }
  function goToSettings(): void {
    requestSettingsCategory('library');
    setCurrentSection('settings');
    navigateTo({ section: 'settings', selectedArtistId: null, selectedAlbumId: null });
  }
  async function handleSearch(query: string, activate = false): Promise<void> {
    const results = await search.search(query);
    // Return in the search field: this same query's answer, never whatever
    // the previous query's top result was.
    if (activate && results) handleSearchSubmit(results);
  }

  /** Return in the search field, mirroring Apple Music/Spotify: jump straight
   * to the top result rather than merely re-submitting the same query. A
   * track plays immediately (it has no page of its own); an album or artist
   * opens, the same as clicking it in the results view. */
  function handleSearchSubmit(results: RankedSearchResults): void {
    const top = results.topResult;
    if (!top) return;
    if (top.type === 'track') {
      startPlayback([top.item], 0).catch((e) => console.error('Failed to play top result:', e));
    } else if (top.type === 'album') {
      handleAlbumClick(top.item);
    } else {
      handleArtistClick(top.item);
    }
  }
  function handleAlbumClick(album: Album): void {
    syncColumnBrowserToSelection({ artist: album.artistName, albumId: album.jellyfinId });
    setCurrentSection('tracks');
    if (!ui.columnBrowserVisible) toggleColumnBrowser();
    navigateTo({ section: 'tracks', selectedArtistId: null, selectedAlbumId: album.jellyfinId, columnBrowserVisible: true });
  }
  // getAlbumsTracks is a live Jellyfin fetch per album (get_album_tracks
  // releases its connection lock before awaiting the network call), so
  // a disconnect or backend switch can complete while it's in flight. Fence
  // against applying a now-stale result: if the connection has moved on by
  // the time this resolves, drop it rather than act on a backend that's no
  // longer live. The one fetch every album menu/play action goes through —
  // one album or a whole grid selection of them, alike (Apple Music's tile menu acts on the selection, not just the tile
  // under the pointer).
  async function fetchAlbumsTracks(albums: Album[]): Promise<Track[] | null> {
    const generation = getConnectionGeneration();
    try {
      const tracks = await getAlbumsTracks(albums.map((a) => a.jellyfinId));
      return getConnectionGeneration() === generation ? tracks : null;
    } catch (e) {
      console.error('Failed to load album tracks:', e);
      return null;
    }
  }
  const handleAlbumsPlay = (albums: Album[]) =>
    fetchAlbumsTracks(albums).then((t) => t && startPlayback(t, 0));
  // AlbumList and SearchResultsView have no selection concept and only ever
  // act on the one album clicked — the plural resolver with a single-item list.
  const handleAlbumPlay = (album: Album) => handleAlbumsPlay([album]);
  const handleAlbumQueue = (albums: Album[], position: 'next' | 'end') =>
    fetchAlbumsTracks(albums).then((t) => t && (position === 'next' ? queueTrackNext(t) : queueTrackAtEnd(t)));
  const handleAlbumAddToPlaylist = (albums: Album[]) =>
    fetchAlbumsTracks(albums).then((t) => t && handleAddToPlaylist(t));

  async function fetchArtistsTracks(artists: Artist[]): Promise<Track[] | null> {
    const generation = getConnectionGeneration();
    try {
      const tracks = await getArtistsTracks(artists.map((a) => a.jellyfinId));
      return getConnectionGeneration() === generation ? tracks : null;
    } catch (e) {
      console.error('Failed to load artist tracks:', e);
      return null;
    }
  }
  const handleArtistTilePlay = (artists: Artist[]) =>
    fetchArtistsTracks(artists).then((t) => t && startPlayback(t, 0));
  const handleArtistTileQueue = (artists: Artist[], position: 'next' | 'end') =>
    fetchArtistsTracks(artists).then((t) => t && (position === 'next' ? queueTrackNext(t) : queueTrackAtEnd(t)));
  const handleArtistTileAddToPlaylist = (artists: Artist[]) =>
    fetchArtistsTracks(artists).then((t) => t && handleAddToPlaylist(t));
  function handleArtistClick(artist: Artist): void {
    search.dismiss();
    isExplicitlySelectingArtist = true;
    selectedArtist = artist;
    syncColumnBrowserToSelection({ artist: artist.name });
    setCurrentSection('artists');
    navigateTo({ section: 'artists', selectedArtistId: artist.jellyfinId, selectedAlbumId: null });
    setTimeout(() => { isExplicitlySelectingArtist = false; }, 100);
  }

  const handleArtistTrackPlay = (track: Track, albumTracks: Track[]) =>
    playTrackFromList(track, albumTracks).catch((e) => console.error('Failed to play track:', e));
  const handleArtistPlayAll = (tracks: Track[]) =>
    startPlayback(tracks, 0).catch((e) => console.error('Failed to play all:', e));
  const handleArtistShuffleAll = (tracks: Track[]) =>
    shuffleAndPlay(tracks).catch((e) => console.error('Failed to shuffle all:', e));
  const handleTrackClick = (_track: Track) => {};
  const handleTrackPlay = (track: Track) =>
    playTrackFromList(track, displayedTracks).catch((e) => console.error('Failed to play track:', e));

  async function handlePlaylistTrackPlay(pt: import('#lib/types/models.js').JellyfinPlaylistTrack): Promise<void> {
    try {
      const track = jellyfinPlaylistTrackToTrack(pt);
      if (!track) return;
      const playlist = getJellyfinPlaylistState().currentPlaylist;
      if (!playlist?.tracks) return;
      const all = playlist.tracks.map(jellyfinPlaylistTrackToTrack).filter((t) => t !== null);
      await startPlayback(all, Math.max(0, all.findIndex((t) => t.jellyfinId === track.jellyfinId)));
    } catch (e) { console.error('Failed to play playlist track:', e); }
  }

  // Shuffle-play the whole current playlist, mirroring the artist
  // "Shuffle all" one-shot: shuffleAndPlay does a client-side Fisher-Yates and
  // starts from the top of the shuffled order.
  function handlePlaylistShuffleAll(): void {
    const playlist = getJellyfinPlaylistState().currentPlaylist;
    if (!playlist?.tracks?.length) return;
    const all = playlist.tracks.map(jellyfinPlaylistTrackToTrack).filter((t) => t !== null);
    if (all.length === 0) return;
    shuffleAndPlay(all).catch((e) => console.error('Failed to shuffle playlist:', e));
  }

  // Shared/imported playlists: only resolved tracks (a local library
  // match) are playable — importedPlaylistTrackToTrack returns null for the
  // rest, filtered out here before handing the list to playback.
  async function handleSharedPlaylistTrackPlay(
    pt: import('#lib/types/models.js').ImportedPlaylistTrack
  ): Promise<void> {
    try {
      const playlist = getImportedPlaylistState().currentPlaylist;
      if (!playlist?.tracks) return;
      const all = playlist.tracks.map(importedPlaylistTrackToTrack).filter((t) => t !== null);
      const track = importedPlaylistTrackToTrack(pt);
      if (!track) return;
      await startPlayback(all, Math.max(0, all.findIndex((t) => t.jellyfinId === track.jellyfinId)));
    } catch (e) {
      console.error('Failed to play shared playlist track:', e);
    }
  }

  function handleSharedPlaylistShuffleAll(): void {
    const playlist = getImportedPlaylistState().currentPlaylist;
    if (!playlist?.tracks?.length) return;
    const all = playlist.tracks.map(importedPlaylistTrackToTrack).filter((t) => t !== null);
    if (all.length === 0) return;
    shuffleAndPlay(all).catch((e) => console.error('Failed to shuffle shared playlist:', e));
  }

  const handleAddToPlaylist = (tracks: Track[]) => { tracksToAddToPlaylist = tracks; showAddToJellyfinPlaylistDialog = true; };
  const handleCloseDialog = () => { showAddToJellyfinPlaylistDialog = false; tracksToAddToPlaylist = []; };

  const handleGenrePlay = (genre: string) => playGenre(genre).catch((e) => console.error('Failed to play genre:', e));
  const handleGenreShuffle = (genre: string) => shuffleGenre(genre).catch((e) => console.error('Failed to shuffle genre:', e));
  const handleColumnArtistPlay = (artist: string) => playColumnArtist(artist).catch((e) => console.error('Failed to play artist:', e));
  const handleColumnAlbumPlay = (albumId: string) => playColumnAlbum(albumId).catch((e) => console.error('Failed to play album:', e));
  // Which view's tile size the toolbar slider reads/writes — null hides the
  // slider (each grid-shaped view remembers its own size,
  // the way Apple Music's View Options does per view rather than one shared
  // setting). Artists only shows it in its own grid, never the split/list
  // view or a selected artist's hero view; Genres only in grid mode.
  const activeGridSizeView = $derived.by((): GridViewId | null => {
    const s = ui.currentSection;
    if (s === 'albums') return 'albums';
    if (s === 'artists') return ui.artistViewMode === 'grid' && !selectedArtist ? 'artists' : null;
    if (s === 'genres') return ui.genreViewMode === 'grid' ? 'genres' : null;
    if (s === 'recentlyAdded') return 'recentlyAdded';
    return null;
  });
  const activeGridTileSize = $derived(activeGridSizeView ? getGridTileSize(activeGridSizeView) : null);
  const handleGridTileSizeChange = (event: Event) => {
    if (!activeGridSizeView) return;
    setGridTileSize(activeGridSizeView, parseInt((event.target as HTMLInputElement).value, 10));
  };

  $effect(() => {
    const onSearch = (e: Event) => {
      const detail = (e as CustomEvent<{ query: string; activate: boolean }>).detail;
      handleSearch(detail.query, detail.activate);
    };
    const onNavRestore = (e: Event) => {
      const id = (e as CustomEvent<string | null>).detail;
      selectedArtist = id ? (lib.artists.find((a) => a.jellyfinId === id) ?? null) : null;
    };
    // The now-playing bar's own "Add to Playlist" (LcdPanel, mounted in the
    // root layout, out of prop reach from this route's dialog state).
    const onAddToPlaylist = (e: Event) => {
      handleAddToPlaylist((e as CustomEvent<Track[]>).detail);
    };
    const onKeydown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !e.defaultPrevented && ui.searchQuery) {
        clearSearch();
        search.dismiss();
        e.preventDefault();
      }
    };
    window.addEventListener('keydown', onKeydown);
    window.addEventListener('bragi-search', onSearch);
    window.addEventListener('bragi-nav-restore-artist', onNavRestore);
    window.addEventListener('bragi-add-to-playlist', onAddToPlaylist);
    return () => {
      window.removeEventListener('keydown', onKeydown);
      window.removeEventListener('bragi-search', onSearch);
      window.removeEventListener('bragi-nav-restore-artist', onNavRestore);
      window.removeEventListener('bragi-add-to-playlist', onAddToPlaylist);
    };
  });

  $effect(() => { if (!ui.searchQuery) search.dismiss(); });

  $effect(() => {
    const sort = ui.tracksSort;
    const section = currentSection;
    const _offline = getOfflineMode();
    const searching = isShowingSearchResults;
    // Destructure to track each selection Set reference
    const { selectedGenres, selectedArtists, selectedAlbums } = getColumnBrowserState();
    void selectedGenres;
    void selectedArtists;
    void selectedAlbums;
    const _libVersion = getLibraryVersion();
    if (section === 'tracks' && !searching && libraryLoaded) {
      refreshLibraryView(sort);
    }
  });

  let navInitialised = false;
  $effect(() => { const s = ui.currentSection; if (!navInitialised) { navInitialised = true; initNavigation(s); } });

  $effect(() => {
    // Re-run when the active backend changes so a toggle re-loads the
    // library from the newly-active backend's cache — getIsJellyfinConnected()
    // stays true across a switch, so it alone would not re-trigger.
    void getActiveBackend();
    if (getIsJellyfinConnected()) loadLibrary();
    else isNotConnected = true;
  });

  let previousSection: string | null = null;
  let previousArtistViewMode: ViewMode | null = null;
  $effect(() => {
    const section = ui.currentSection;
    const vm = ui.artistViewMode;
    const entering = previousSection !== 'artists' && section === 'artists';
    const toGrid = previousArtistViewMode === 'list' && vm === 'grid';
    previousSection = section; previousArtistViewMode = vm;
    if (section !== 'artists') { if (selectedArtist) selectedArtist = null; return; }
    // Search is an overlay, not a replacement for the underlying selection.
    const artists = lib.artists;
    if (vm === 'grid') {
      if ((entering || toGrid) && !isExplicitlySelectingArtist) selectedArtist = null;
      const cur = untrack(() => selectedArtist);
      if (cur && !artists.some((a) => a.jellyfinId === cur.jellyfinId)) selectedArtist = null;
      return;
    }
    if (artists.length === 0) { selectedArtist = null; return; }
    const cur = untrack(() => selectedArtist);
    if (!cur || !artists.some((a) => a.jellyfinId === cur.jellyfinId)) selectedArtist = artists[0] ?? null;
  });

  const footerData = $derived.by((): StatusBarData | null => {
    const section = ui.currentSection;
    if (!librarySections.has(section) || isNotConnected || lib.isLoading || lib.error) return null;
    // Read the reactive triggers (array refs + selection) here so the footer
    // still recomputes when they change, but run the heavy per-element
    // iteration under untrack(): reading 23k+ deeply-reactive tracks inside a
    // $derived otherwise registers a dependency per property access and hits
    // Svelte 5's O(n^2) dependency-tracking cliff (a 13-20s freeze per switch).
    if (section === 'tracks') {
      const t = displayedTracks;
      return untrack(() => tracksFooterData(t));
    }
    if (section === 'albums') {
      const a = displayedAlbums;
      const t = lib.tracks;
      return untrack(() => albumsFooterData(a, t));
    }
    if (section === 'artists') {
      const a = displayedArtists;
      const sel = selectedArtist;
      const t = lib.tracks;
      return untrack(() => artistsFooterData(a, t, sel));
    }
    return null;
  });
</script>

<div class="page">
  <LibraryToolbar
    {showToolbar}
    currentSection={ui.currentSection}
    {isShowingSearchResults}
    sectionTitle={sectionTitles[ui.currentSection] ?? ui.currentSection}
    viewMode={ui.viewMode}
    artistViewMode={ui.artistViewMode}
    genreViewMode={ui.genreViewMode}
    onViewModeChange={setViewMode}
    onArtistViewModeChange={setArtistViewMode}
    onGenreViewModeChange={setGenreViewMode}
    columnBrowserVisible={ui.columnBrowserVisible}
    onToggleColumnBrowser={toggleColumnBrowser}
    gridTileSize={activeGridTileSize}
    minGridTileSize={MIN_GRID_TILE_SIZE}
    maxGridTileSize={MAX_GRID_TILE_SIZE}
    gridTileSizeStep={GRID_TILE_SIZE_STEP}
    onGridTileSizeChange={handleGridTileSizeChange}
    {currentSortOptions}
    {currentSort}
    onSortChange={handleSortChange}
    canGoBack={getCanGoBack()}
    canGoForward={getCanGoForward()}
    onNavigateBack={navigateBack}
    onNavigateForward={navigateForward}
  />

  <div class="page-content">
    <LibraryContent
      currentSection={ui.currentSection}
      isLoading={lib.isLoading}
      isSyncing={lib.isSyncing}
      {isNotConnected}
      error={lib.error}
      isSearching={search.searching}
      libraryEmpty={lib.albums.length === 0 && lib.artists.length === 0}
      {isShowingSearchResults}
      searchQuery={ui.searchQuery}
      viewMode={ui.viewMode}
      artistViewMode={ui.artistViewMode}
      genreViewMode={ui.genreViewMode}
      genresSort={ui.genresSort}
      columnBrowserVisible={ui.columnBrowserVisible}
      artistAlbumsSort={ui.artistAlbumsSort}
      {displayedAlbums}
      {displayedArtists}
      {displayedTracks}
      {selectedArtist}
      onGoToSettings={goToSettings}
      onLoadLibrary={loadLibrary}
      onAlbumClick={handleAlbumClick}
      onAlbumPlay={handleAlbumPlay}
      onAlbumsPlay={handleAlbumsPlay}
      onAlbumQueue={handleAlbumQueue}
      onAlbumAddToPlaylist={handleAlbumAddToPlaylist}
      onArtistClick={handleArtistClick}
      onArtistTilePlay={handleArtistTilePlay}
      onArtistTileQueue={handleArtistTileQueue}
      onArtistTileAddToPlaylist={handleArtistTileAddToPlaylist}
      onArtistTrackPlay={handleArtistTrackPlay}
      onArtistPlayAll={handleArtistPlayAll}
      onArtistShuffleAll={handleArtistShuffleAll}
      onTrackClick={handleTrackClick}
      onTrackPlay={handleTrackPlay}
      onPlaylistTrackPlay={handlePlaylistTrackPlay}
      onPlaylistShuffleAll={handlePlaylistShuffleAll}
      onSharedPlaylistTrackPlay={handleSharedPlaylistTrackPlay}
      onSharedPlaylistShuffleAll={handleSharedPlaylistShuffleAll}
      onAddToPlaylist={handleAddToPlaylist}
      onGenrePlay={handleGenrePlay}
      onGenreShuffle={handleGenreShuffle}
      onColumnArtistPlay={handleColumnArtistPlay}
      onColumnAlbumPlay={handleColumnAlbumPlay}
    />
  </div>

  <StatusBar data={footerData} />
</div>

{#if showAddToJellyfinPlaylistDialog && tracksToAddToPlaylist.length > 0}
  <AddToJellyfinPlaylistDialog tracks={tracksToAddToPlaylist} onClose={handleCloseDialog} />
{/if}

<!-- Mounted once: the "already in this playlist?" question every
     add-to-playlist entry point (this dialogue, sidebar drag) shares. -->
<AddDuplicateTracksDialog />

<style>
  .page { display: flex; flex-direction: column; height: 100%; }
  .page-content { flex: 1; min-height: 0; }
</style>
