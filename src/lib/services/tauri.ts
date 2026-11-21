/**
 * Type-safe Tauri IPC wrapper for frontend-backend communication.
 * All Rust commands should be called through these functions.
 */

import { invoke } from '@tauri-apps/api/core';
import type {
  Track,
  Album,
  Artist,
  PlaybackState,
  QueueState,
  JellyfinPlaylist,
  JellyfinPlaylistDetail,
  TrackLyrics,
  ImportPlaylistTrackInput,
  ImportedPlaylist,
  ImportedPlaylistDetail,
} from '#lib/types/models.js';
import type {
  AppInfo,
  AppSettings,
  AudioDevice,
  CacheSettings,
  CacheStats,
  ConnectionStatus,
  DeveloperInfo,
  DownloadState,
  QuickConnectInfo,
  QueuedDownload,
  QueuedOperation,
  SyncMetrics,
} from '#lib/types/tauri.js';

// Jellyfin connection commands

export async function loginJellyfin(
  serverUrl: string,
  username: string,
  password: string
): Promise<void> {
  return invoke('login_jellyfin', { serverUrl, username, password });
}

export async function quickConnectCheck(serverUrl: string): Promise<boolean> {
  return invoke<boolean>('quick_connect_check', { serverUrl });
}

export async function quickConnectStart(serverUrl: string): Promise<QuickConnectInfo> {
  return invoke<QuickConnectInfo>('quick_connect_start', { serverUrl });
}

export async function quickConnectStatus(serverUrl: string, secret: string): Promise<boolean> {
  return invoke<boolean>('quick_connect_status', { serverUrl, secret });
}

export async function quickConnectComplete(serverUrl: string, secret: string): Promise<void> {
  return invoke('quick_connect_complete', { serverUrl, secret });
}

/** @deprecated Use `loginJellyfin` or Quick Connect instead. */
export async function connectJellyfin(
  serverUrl: string,
  apiKey: string,
  userId: string
): Promise<void> {
  return invoke('connect_jellyfin', { serverUrl, apiKey, userId });
}

export async function disconnectJellyfin(): Promise<void> {
  return invoke('disconnect_jellyfin');
}

export async function getJellyfinStatus(): Promise<ConnectionStatus> {
  return invoke<ConnectionStatus>('get_jellyfin_status');
}

export async function testJellyfinConnection(): Promise<void> {
  return invoke('test_jellyfin_connection');
}

// Library commands
export async function getLibraryAlbums(): Promise<Album[]> {
  return invoke<Album[]>('get_library_albums');
}

export async function getLibraryArtists(): Promise<Artist[]> {
  return invoke<Artist[]>('get_library_artists');
}

export async function getAlbumTracks(albumId: string): Promise<Track[]> {
  return invoke<Track[]>('get_album_tracks', { albumId });
}

export async function getArtistAlbums(artistId: string): Promise<Album[]> {
  return invoke<Album[]>('get_artist_albums', { artistId });
}

/**
 * Fetch built-in lyrics for a track from Jellyfin.
 *
 * Returns `null` when the track has no lyrics (or the server predates the
 * lyrics endpoint) — the backend maps a 404 to `Ok(None)`, so this is not
 * an error.
 */
export async function getTrackLyrics(trackId: string): Promise<TrackLyrics | null> {
  return invoke<TrackLyrics | null>('get_track_lyrics', { trackId });
}

/**
 * Fetch precomputed waveform peaks for a track.
 *
 * Returns a normalised (0–1) peak array for drawing a seekable waveform, or
 * `null` when peaks can't be produced (offline with no local copy, undecodable
 * codec with no server, etc.). Peaks are cached on disk after the first call,
 * so a subsequent request — including a prefetched upcoming track — is instant.
 */
export async function getTrackWaveform(trackId: string): Promise<number[] | null> {
  return invoke<number[] | null>('get_track_waveform', { trackId });
}

/**
 * Search the local SQLite cache (FTS5-indexed) rather than the media server.
 *
 * The cache is kept current by `sync_library`/background sync, so this
 * returns results in single-digit milliseconds regardless of library size.
 * Prefer `performLibrarySearch` (`#lib/utils/search.js`), which also knows when
 * to fall back to `searchLibraryLive` and ranks the result — call this
 * directly only from that orchestration layer.
 */
export async function searchLibrary(
  query: string
): Promise<{ tracks: Track[]; albums: Album[]; artists: Artist[] }> {
  return invoke('search_cached_library', { query });
}

/**
 * Search live on the media server instead of the local cache.
 *
 * Fallback path for when the cache can't be trusted yet — e.g. a search fired
 * during a backend's first-ever sync, before anything has landed in SQLite.
 * One request either way (Jellyfin's `/Items?SearchTerm=` returns every match
 * inline; Subsonic's `search3` always has); slower than the cache only for
 * the network round trip itself, not for a chain of per-result fetches.
 */
export async function searchLibraryLive(
  query: string
): Promise<{ tracks: Track[]; albums: Album[]; artists: Artist[] }> {
  return invoke('search_library', { query });
}

export async function getLibraryTracks(): Promise<Track[]> {
  return invoke<Track[]>('get_all_cached_tracks');
}

export async function getCachedAlbums(): Promise<Album[]> {
  return invoke<Album[]>('get_cached_albums');
}

export async function getRecentlyAddedAlbums(days: number): Promise<Album[]> {
  return invoke<Album[]>('get_recently_added_albums', { days });
}

export async function getCachedArtists(): Promise<Artist[]> {
  return invoke<Artist[]>('get_cached_artists');
}

export async function getCachedArtistAlbums(artistId: string): Promise<Album[]> {
  return invoke<Album[]>('get_cached_artist_albums', { artistId });
}

export async function getCachedAlbumTracks(albumId: string): Promise<Track[]> {
  return invoke<Track[]>('get_cached_tracks', { albumId });
}

export async function getStreamUrl(trackId: string): Promise<string> {
  return invoke<string>('get_stream_url', { trackId });
}

export async function getArtworkUrl(itemId: string, size?: number): Promise<string> {
  return invoke<string>('get_artwork_url', { itemId, size });
}

// Playback commands
export async function playTrack(track: Track, clearQueue?: boolean): Promise<void> {
  return invoke('play_track', { track, clearQueue });
}

/**
 * Play a full context (a Songs table, an album, a playlist, ...): replaces the
 * queue with `tracks` in order and starts playback at `startIndex`, in one
 * atomic backend operation. The one mechanism every "play this row" call site
 * goes through — see `startPlayback` in `#lib/utils/playback.js`.
 *
 * `shuffle` is applied by the engine while it builds the queue, before the
 * first `queue-changed` broadcast — pass the current shuffle preference here
 * rather than calling `setShuffle` afterwards, which would shuffle a second
 * time and visibly reorder the queue a moment after it first appeared.
 */
export async function playContext(
  tracks: Track[],
  startIndex: number,
  shuffle: boolean
): Promise<void> {
  return invoke('play_context', { tracks, startIndex, shuffle });
}

export async function pause(): Promise<void> {
  return invoke('pause');
}

export async function resume(): Promise<void> {
  return invoke('resume');
}

export async function seek(positionMs: number): Promise<void> {
  return invoke('seek', { positionMs });
}

export async function setVolume(volume: number): Promise<void> {
  return invoke('set_volume', { volume });
}

export async function getPlaybackState(): Promise<PlaybackState> {
  return invoke<PlaybackState>('get_playback_state');
}

// MiniPlayer window commands
export async function toggleMiniPlayer(): Promise<void> {
  return invoke('toggle_mini_player');
}

export async function openMiniPlayer(): Promise<void> {
  return invoke('open_mini_player');
}

export async function closeMiniPlayer(): Promise<void> {
  return invoke('close_mini_player');
}

export async function setMiniAlwaysOnTop(enabled: boolean): Promise<void> {
  return invoke('set_mini_always_on_top', { enabled });
}

// Audio device commands

export async function getOutputDevices(): Promise<AudioDevice[]> {
  return invoke<AudioDevice[]>('get_output_devices');
}

export async function setOutputDevice(deviceName: string | null): Promise<void> {
  return invoke('set_output_device', { deviceName });
}

// Queue commands
export async function getQueue(): Promise<QueueState> {
  return invoke<QueueState>('get_queue');
}

export async function addToQueue(tracks: Track[], position: 'next' | 'end'): Promise<void> {
  return invoke('add_to_queue', { tracks, position });
}

export async function clearQueue(): Promise<void> {
  return invoke('clear_queue');
}

/** Remove one queue entry by its absolute index (the Up Next panel's remove action). */
export async function removeFromQueue(index: number): Promise<void> {
  return invoke('remove_from_queue', { index });
}

/** Reorder a queue entry from one absolute index to another (the Up Next panel's drag-to-reorder). */
export async function reorderQueue(fromIndex: number, toIndex: number): Promise<void> {
  return invoke('reorder_queue', { fromIndex, toIndex });
}

/**
 * Jump straight to a queue entry by index — the Up Next panel's
 * double-click/Return activation. The one queue-navigation path alongside
 * `skipNext`/`skipPrevious`.
 */
export async function jumpToQueueIndex(index: number): Promise<void> {
  return invoke('jump_to_queue_index', { index });
}

export async function skipNext(): Promise<void> {
  return invoke('skip_next');
}

export async function skipPrevious(): Promise<void> {
  return invoke('skip_previous');
}

/**
 * Set shuffle on the audio engine. This is the bridge the shuffle toggle
 * was missing: the engine owns the queue order (one-time Fisher-Yates permutation
 * with the current track pinned, restorable on toggle-off), and this is what
 * actually reorders it. Enabling re-permutes the current queue every call, so a
 * freshly built queue (Play All) is shuffled when the toggle is on.
 */
export async function setShuffle(shuffle: boolean): Promise<void> {
  return invoke('set_shuffle', { shuffle });
}

/**
 * Set the repeat mode on the audio engine. Mirrors {@link setShuffle}:
 * the engine's queue layer already honours the mode (wrap at end for `all`,
 * replay for `one`), this is the bridge that was missing to reach it.
 */
export async function setRepeat(repeat: 'off' | 'all' | 'one'): Promise<void> {
  return invoke('set_repeat', { repeat });
}

// Settings commands
export async function getSettings(): Promise<AppSettings> {
  return invoke<AppSettings>('get_settings');
}

export async function updateSettings(settings: Partial<AppSettings>): Promise<void> {
  return invoke('update_settings', { settings });
}

// AI Playlists — LLM provider config. API keys go to the keyring and are
// never read back; only whether one is set (AppSettings.llmHasApiKey).
export async function setLlmApiKey(provider: string, apiKey: string): Promise<void> {
  return invoke('set_llm_api_key', { provider, apiKey });
}

export async function clearLlmApiKey(provider: string): Promise<void> {
  return invoke('clear_llm_api_key', { provider });
}

/** Result of a connection test: outcome + the models to populate the dropdown. */
export interface TestLlmConnectionResult {
  ok: boolean;
  models: string[];
  message: string;
}

/**
 * Test an LLM provider connection using the values the user is editing (each
 * optional; falls back to the saved config), returning the available models in
 * one call. Never rejects on a connection failure — check `ok` + `message`.
 */
export async function testLlmConnection(args?: {
  provider?: string;
  baseUrl?: string;
  apiKey?: string;
}): Promise<TestLlmConnectionResult> {
  return invoke<TestLlmConnectionResult>('test_llm_connection', {
    provider: args?.provider ?? null,
    baseUrl: args?.baseUrl ?? null,
    apiKey: args?.apiKey ?? null,
  });
}

/** A generated playlist draft the user reviews before saving. */
export interface AiPlaylistDraft {
  suggestedName: string;
  tracks: Track[];
  suggestedCount: number;
  matchedCount: number;
  prompt: string;
}

/** Generate a playlist draft from a natural-language prompt. */
export async function generateAiPlaylist(prompt: string): Promise<AiPlaylistDraft> {
  return invoke<AiPlaylistDraft>('generate_ai_playlist', { prompt });
}

// Telemetry — where this machine's OTLP exporter points. A Dock-launched app
// inherits no fleet environment, so this is the only door on a machine the
// fleet does not configure. No header value ever crosses the IPC boundary; the
// helper command string is all that is stored.

/** The exporter in force, and what this machine has saved. */
export interface TelemetrySettings {
  /** The collector Bragi exports to. Empty means local only. */
  endpoint: string;
  /** The header helper in force. Empty means no authorisation headers. */
  headersHelper: string;
  /** The environment set the exporter, so the fields are read-only. */
  fromEnv: boolean;
  /** The endpoint saved on this machine — what the fields edit. */
  savedEndpoint: string;
  /** The helper command saved on this machine. */
  savedHeadersHelper: string;
}

/** A probe's outcome: accepted, or the class that stopped it. */
export interface TelemetryProbeResult {
  ok: boolean;
  /** The failure class or HTTP status; empty on success. Never a URL. */
  message: string;
}

export async function getTelemetrySettings(): Promise<TelemetrySettings> {
  return invoke<TelemetrySettings>('plugin:telemetry|telemetry_get');
}

/** Save the collector and repoint the live exporter at it. */
export async function setTelemetrySettings(
  endpoint: string,
  headersHelper: string
): Promise<TelemetrySettings> {
  return invoke<TelemetrySettings>('plugin:telemetry|telemetry_set', { endpoint, headersHelper });
}

/**
 * Send one log record to the values being edited, without saving them. Never
 * rejects on a collector failure — check `ok` + `message`.
 */
export async function probeTelemetry(
  endpoint: string,
  headersHelper: string
): Promise<TelemetryProbeResult> {
  return invoke<TelemetryProbeResult>('plugin:telemetry|telemetry_probe', {
    endpoint,
    headersHelper,
  });
}

/** Enable or disable launching Bragi on system login. */
export async function setLaunchOnLogin(enabled: boolean): Promise<void> {
  return invoke('set_launch_on_login', { enabled });
}

/** Quit the application unconditionally, bypassing close-to-tray. */
export async function quitApp(): Promise<void> {
  return invoke('quit_app');
}

// Sync commands

export async function syncLibrary(): Promise<SyncMetrics> {
  return invoke<SyncMetrics>('sync_library');
}

// Download commands
export async function downloadTrack(
  trackId: string,
  trackName: string,
  artistName: string
): Promise<void> {
  return invoke('download_track', { trackId, trackName, artistName });
}

export async function downloadAlbum(albumId: string): Promise<number> {
  return invoke<number>('download_album', { albumId });
}

export async function cancelDownload(trackId: string): Promise<void> {
  return invoke('cancel_download', { trackId });
}

export async function pauseDownloads(): Promise<void> {
  return invoke('pause_downloads');
}

export async function resumeDownloads(): Promise<void> {
  return invoke('resume_downloads');
}

export async function getDownloadQueue(): Promise<QueuedDownload[]> {
  return invoke<QueuedDownload[]>('get_download_queue');
}

export async function getDownloadState(): Promise<DownloadState> {
  return invoke<DownloadState>('get_download_state');
}

export async function clearCompletedDownloads(): Promise<void> {
  return invoke('clear_completed_downloads');
}

export async function getCacheSettings(): Promise<CacheSettings> {
  return invoke<CacheSettings>('get_cache_settings');
}

export async function setCacheSettings(maxSizeBytes: number): Promise<void> {
  return invoke('set_cache_settings', { maxSizeBytes });
}

export async function clearCache(): Promise<void> {
  return invoke('clear_cache');
}

export async function getDownloadedTracks(): Promise<Track[]> {
  return invoke<Track[]>('get_downloaded_tracks');
}

export async function removeDownloadedTrack(trackId: string): Promise<void> {
  return invoke('remove_downloaded_track', { trackId });
}

// ============================================================================
// Jellyfin playlist commands
// ============================================================================

export async function getJellyfinPlaylists(): Promise<JellyfinPlaylist[]> {
  return invoke<JellyfinPlaylist[]>('get_jellyfin_playlists');
}

export async function getJellyfinPlaylist(playlistId: string): Promise<JellyfinPlaylistDetail> {
  return invoke<JellyfinPlaylistDetail>('get_jellyfin_playlist', { playlistId });
}

/**
 * Read a playlist's tracks from the local SQLite cache only (no network).
 * Returns null when the playlist was never cached. Used to hydrate the view
 * instantly on first click and to browse playlists offline.
 */
export async function getCachedJellyfinPlaylist(
  playlistId: string
): Promise<JellyfinPlaylistDetail | null> {
  return invoke<JellyfinPlaylistDetail | null>('get_cached_jellyfin_playlist', { playlistId });
}

export async function createJellyfinPlaylist(name: string): Promise<JellyfinPlaylist> {
  return invoke<JellyfinPlaylist>('create_jellyfin_playlist', { name });
}

export async function deleteJellyfinPlaylist(playlistId: string): Promise<void> {
  return invoke('delete_jellyfin_playlist', { playlistId });
}

export async function renameJellyfinPlaylist(playlistId: string, name: string): Promise<void> {
  return invoke('rename_jellyfin_playlist', { playlistId, name });
}

export async function addTracksToJellyfinPlaylist(
  playlistId: string,
  trackIds: string[]
): Promise<JellyfinPlaylistDetail> {
  return invoke<JellyfinPlaylistDetail>('add_tracks_to_jellyfin_playlist', {
    playlistId,
    trackIds,
  });
}

export async function removeTracksFromJellyfinPlaylist(
  playlistId: string,
  entryIds: string[]
): Promise<JellyfinPlaylistDetail> {
  return invoke<JellyfinPlaylistDetail>('remove_tracks_from_jellyfin_playlist', {
    playlistId,
    entryIds,
  });
}

export async function moveJellyfinPlaylistItem(
  playlistId: string,
  itemId: string,
  newIndex: number
): Promise<JellyfinPlaylistDetail> {
  return invoke<JellyfinPlaylistDetail>('move_jellyfin_playlist_item', {
    playlistId,
    itemId,
    newIndex,
  });
}

// ============================================================================
// Shared/imported playlist commands
// ============================================================================

export async function importPlaylist(
  name: string,
  tracks: ImportPlaylistTrackInput[]
): Promise<ImportedPlaylistDetail> {
  return invoke<ImportedPlaylistDetail>('import_playlist', { name, tracks });
}

/**
 * Import a playlist straight from someone else's plain Jellyfin server (no
 * Bragi share code involved) — they need to have given the caller a login on
 * their server first. Credentials are used once, server-side, to fetch this
 * one playlist and are never persisted.
 */
export async function importPlaylistFromJellyfinLink(
  serverUrl: string,
  username: string,
  password: string,
  itemId: string
): Promise<ImportedPlaylistDetail> {
  return invoke<ImportedPlaylistDetail>('import_playlist_from_jellyfin_link', {
    serverUrl,
    username,
    password,
    itemId,
  });
}

export async function getImportedPlaylists(): Promise<ImportedPlaylist[]> {
  return invoke<ImportedPlaylist[]>('get_imported_playlists');
}

export async function getImportedPlaylist(
  playlistId: string
): Promise<ImportedPlaylistDetail | null> {
  return invoke<ImportedPlaylistDetail | null>('get_imported_playlist', { playlistId });
}

export async function deleteImportedPlaylist(playlistId: string): Promise<void> {
  return invoke('delete_imported_playlist', { playlistId });
}

// ============================================================================
// Cache stats commands
// ============================================================================

export async function getCacheStats(): Promise<CacheStats> {
  return invoke<CacheStats>('get_cache_stats');
}

// ============================================================================
// App info commands
// ============================================================================

export async function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>('get_app_info');
}

// ============================================================================
// Developer commands
// ============================================================================

export async function getDeveloperInfo(): Promise<DeveloperInfo> {
  return invoke<DeveloperInfo>('get_developer_info');
}

export async function openDataDirectory(): Promise<void> {
  return invoke('open_data_directory');
}

export async function openCacheDirectory(): Promise<void> {
  return invoke('open_cache_directory');
}

export async function getPendingSyncOperations(): Promise<QueuedOperation[]> {
  return invoke<QueuedOperation[]>('get_pending_sync_operations');
}

export async function clearFailedSyncOperations(): Promise<number> {
  return invoke<number>('clear_failed_sync_operations');
}

export async function sendTestNotification(): Promise<void> {
  return invoke('send_test_notification');
}

// ============================================================================
// Library view commands (SQLite-backed filtering/sorting)
// ============================================================================

export interface LibraryViewParams {
  genres?: string[];
  artists?: string[];
  albumIds?: string[];
  offlineOnly?: boolean;
  sort?: string;
}

export interface ColumnBrowserAlbum {
  id: string;
  name: string;
  artistName: string;
}

export interface ColumnBrowserData {
  genres: string[];
  artists: string[];
  albums: ColumnBrowserAlbum[];
  totalTrackCount: number;
}

export interface LibraryViewResult {
  columnBrowser: ColumnBrowserData;
  tracks: Track[];
}

export async function queryLibraryView(params: LibraryViewParams): Promise<LibraryViewResult> {
  return invoke<LibraryViewResult>('query_library_view', { params });
}

export async function queryTracks(params: LibraryViewParams): Promise<Track[]> {
  return invoke<Track[]>('query_tracks', { params });
}

/**
 * Per-genre counts, aggregated in SQL over the normalised genre memberships.
 * `name` is both the label and the value a genre filter is bound with.
 */
export interface GenreStat {
  name: string;
  trackCount: number;
  artistCount: number;
  albumCount: number;
  durationMs: number;
  yearMin: number | null;
  yearMax: number | null;
  /** Up to four album ids for the grid tile's cover mosaic, deterministic. */
  albumIds: string[];
}

export async function getGenreStats(): Promise<GenreStat[]> {
  return invoke<GenreStat[]>('get_genre_stats');
}

// ============================================================================
// Equalizer commands
// ============================================================================

import type { EqPresetInfo, EqualizerStatus } from '#lib/types/tauri.js';

export async function getEqPresets(): Promise<EqPresetInfo[]> {
  return invoke<EqPresetInfo[]>('get_eq_presets');
}

export async function getEqStatus(): Promise<EqualizerStatus> {
  return invoke<EqualizerStatus>('get_eq_status');
}

export async function setEqEnabled(enabled: boolean): Promise<void> {
  return invoke('set_eq_enabled', { enabled });
}

export async function setEqPreset(presetName: string): Promise<void> {
  return invoke('set_eq_preset', { presetName });
}

export async function setEqBands(bands: number[]): Promise<void> {
  return invoke('set_eq_bands', { bands });
}

/** Save the current bands as a named user profile; returns the refreshed list. */
export async function saveEqProfile(name: string, bands: number[]): Promise<EqPresetInfo[]> {
  return invoke<EqPresetInfo[]>('save_eq_profile', { name, bands });
}

/** Delete a user profile by name; returns the refreshed preset list. */
export async function deleteEqProfile(name: string): Promise<EqPresetInfo[]> {
  return invoke<EqPresetInfo[]>('delete_eq_profile', { name });
}

export async function getEqPresetInfo(presetName: string): Promise<EqPresetInfo> {
  return invoke<EqPresetInfo>('get_eq_preset_info', { presetName });
}

export async function resetEq(): Promise<void> {
  return invoke('reset_eq');
}

/** Enable/disable the pause-stop volume fade and set its ramp duration. */
export async function setFadeSettings(enabled: boolean, durationMs: number): Promise<void> {
  return invoke('set_fade_settings', { enabled, durationMs });
}

// Re-export EQ types for convenience
export type { EqPresetInfo } from '#lib/types/tauri.js';
