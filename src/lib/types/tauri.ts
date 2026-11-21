/**
 * Tauri command types for type-safe IPC between frontend and Rust backend.
 */

import type { Track, Album, Artist, PlaybackState, QueueState } from './models';

/** Media-server connection status */
export interface ConnectionStatus {
  connected: boolean;
  serverUrl?: string;
  /** Active backend when connected: `jellyfin` | `opensubsonic`. */
  serverType?: 'jellyfin' | 'opensubsonic';
  /** Active connection is cleartext `http://` to a non-loopback host. */
  insecure?: boolean;
}

/** Per-backend connection status. `connected` = credentials are stored. */
export interface BackendConnection {
  connected: boolean;
  serverUrl?: string;
}

/** Both backends' status plus which one is currently live. */
export interface MediaBackendsStatus {
  /** The live media slot, or null when no backend is active. */
  active: 'jellyfin' | 'opensubsonic' | null;
  jellyfin: BackendConnection;
  opensubsonic: BackendConnection;
}

/** Quick Connect initiation info */
export interface QuickConnectInfo {
  code: string;
  secret: string;
}

/** Jellyfin connection-related commands */
export interface JellyfinCommands {
  login_jellyfin: (serverUrl: string, username: string, password: string) => Promise<void>;
  quick_connect_check: (serverUrl: string) => Promise<boolean>;
  quick_connect_start: (serverUrl: string) => Promise<QuickConnectInfo>;
  quick_connect_status: (serverUrl: string, secret: string) => Promise<boolean>;
  quick_connect_complete: (serverUrl: string, secret: string) => Promise<void>;
  /** @deprecated Use `login_jellyfin` or Quick Connect instead. */
  connect_jellyfin: (serverUrl: string, apiKey: string, userId: string) => Promise<void>;
  disconnect_jellyfin: () => Promise<void>;
  get_jellyfin_status: () => Promise<ConnectionStatus>;
  test_jellyfin_connection: () => Promise<void>;
  // Dual-backend
  login_subsonic: (serverUrl: string, username: string, password: string) => Promise<void>;
  disconnect_subsonic: () => Promise<void>;
  set_active_backend: (serverType: 'jellyfin' | 'opensubsonic') => Promise<void>;
  get_media_backends: () => Promise<MediaBackendsStatus>;
}

/** Library-related command responses */
export interface LibraryCommands {
  get_library_albums: () => Promise<Album[]>;
  get_library_artists: () => Promise<Artist[]>;
  get_album_tracks: (albumId: string) => Promise<Track[]>;
  get_artist_albums: (artistId: string) => Promise<Album[]>;
  search_cached_library: (query: string) => Promise<{ tracks: Track[]; albums: Album[]; artists: Artist[] }>;
  search_library: (query: string) => Promise<{ tracks: Track[]; albums: Album[]; artists: Artist[] }>;
  get_stream_url: (trackId: string) => Promise<string>;
  get_artwork_url: (itemId: string, size?: number) => Promise<string>;
}

/** Playback-related command responses */
export interface PlaybackCommands {
  play_track: (trackId: string) => Promise<void>;
  pause: () => Promise<void>;
  resume: () => Promise<void>;
  seek: (positionMs: number) => Promise<void>;
  set_volume: (volume: number) => Promise<void>;
  get_playback_state: () => Promise<PlaybackState>;
}

/** Queue-related command responses */
export interface QueueCommands {
  get_queue: () => Promise<QueueState>;
  add_to_queue: (trackIds: string[], position: 'next' | 'end') => Promise<void>;
  clear_queue: () => Promise<void>;
  skip_next: () => Promise<void>;
  skip_previous: () => Promise<void>;
}

/** Settings-related command responses */
export interface SettingsCommands {
  get_settings: () => Promise<AppSettings>;
  update_settings: (settings: Partial<AppSettings>) => Promise<void>;
}

/** Application settings */
export interface AppSettings {
  jellyfinServerUrl?: string;
  /** Active media-server backend. */
  serverType: 'jellyfin' | 'opensubsonic';
  offlineMode: boolean;
  replayGainMode: 'track' | 'album' | 'off';
  crossfadeDuration: number;
  // Fade on pause/stop
  fadeEnabled: boolean;
  fadeDurationMs: number;
  /** Manual lyrics timing offset in ms. Signed: + shows lyrics earlier. */
  lyricsOffsetMs: number;
  // Playback preferences
  volume: number;
  shuffle: boolean;
  repeatMode: 'off' | 'all' | 'one';
  // UI layout state
  sidebarCollapsed: boolean;
  viewMode: 'grid' | 'list';
  artistViewMode: 'grid' | 'list';
  genreViewMode: 'grid' | 'list';
  columnBrowserVisible: boolean;
  columnBrowserHeight: number;
  columnBrowserColumns: string[];
  queueSidebarVisible: boolean;
  statusBarVisible: boolean;
  /** Per-view track list column state (Songs, Recently Played, Search): each
   * view's own visible columns (order = display order) and any widths
   * dragged away from their default (Apple Music keeps this per view
   * rather than one global layout). */
  trackListViewColumns: Record<string, { columns: string[]; widths: Record<string, number> }>;
  /** Same shape, keyed by Jellyfin playlist ID instead of a fixed view id —
   * Apple Music remembers a playlist's columns per playlist, confirmed live: "the options you select will apply to that
   * playlist only." A playlist missing here uses the frontend's own default. */
  playlistTrackColumns: Record<string, { columns: string[]; widths: Record<string, number> }>;
  // Sort preferences
  albumsSort: string;
  artistsSort: string;
  genresSort: string;
  artistAlbumsSort: string;
  tracksSort: string;
  recentlyAddedSort: string;
  // Library sync
  lastLibrarySync: number | null;
  librarySyncInterval: number;
  /** Per-view grid tile size in px (albums, artists, genres, recentlyAdded):
   * Apple Music's View Options (⌘J) remembers icon size per view rather than
   * sharing one global setting. */
  gridTileSizes: Record<string, number>;
  // Theme
  theme: 'dark' | 'light' | 'system';
  accentColour: 'blue' | 'purple' | 'pink' | 'red' | 'orange' | 'yellow' | 'green' | 'teal';
  // Playback
  gaplessPlayback: boolean;
  replayGainPreamp: number;
  // Scrobbling
  scrobbleEnabled: boolean;
  scrobbleThreshold: number;
  // Display
  uiZoomLevel: number;
  // Window (Linux-only)
  titleBarVisible?: boolean;
  // Library
  recentlyAddedDays: number;
  // Recently played (JSON-serialised array)
  recentlyPlayedJson?: string;
  // Notifications
  songNotificationsEnabled: boolean;
  // Startup behaviour
  startupLaunchOnLogin: boolean;
  startMinimised: boolean;
  // System tray behaviour
  closeToTray: boolean;
  minimiseToTray: boolean;
  // MiniPlayer
  miniPlayerAlwaysOnTop: boolean;
  // Downloads / transcoding
  downloadFormat: 'original' | 'flac' | 'mp3' | 'aac' | 'opus';
  downloadBitrate: number;
  // Where play counts are sourced from. The 'jellyfin' value means "the active
  // media server" (Jellyfin or OpenSubsonic); kept for settings back-compat.
  playCountSource: 'auto' | 'jellyfin' | 'earworm';
  // AI Playlists. API keys are never exposed here.
  aiPlaylistEnabled: boolean;
  llmProvider: 'ollama' | 'openai' | 'anthropic';
  llmModel: string;
  llmBaseUrl: string;
  llmMaxTokens: number;
  /** Optional custom curation system prompt (empty = built-in default). */
  llmSystemPrompt: string;
  /** Whether a key is stored for the selected provider (read-only). */
  llmHasApiKey: boolean;
}

/** Audio output device information */
export interface AudioDevice {
  /** mpv device identifier (e.g. "coreaudio/BuiltInSpeakerDevice"). Sent to backend for selection. */
  name: string;
  /** Human-readable description (e.g. "MacBook Pro Speakers"). Displayed in the UI. */
  description: string;
  isDefault: boolean;
}

/** Audio devices changed event payload */
export interface AudioDevicesChangedEvent {
  type: 'devicesChanged';
  devices: AudioDevice[];
}

/** Music service connection status */
export interface MusicServiceStatus {
  connected: boolean;
  serviceUrl?: string;
}

/** Sync status for offline queue */
export interface SyncStatus {
  isConfigured: boolean;
  isOnline: boolean;
  pendingCount: number;
  lastSyncAt?: number;
  lastError?: string;
}

/** Data for scrobbling a track */
export interface ScrobbleData {
  trackMbid?: string;
  artistMbid?: string;
  releaseMbid?: string;
  jellyfinId: string;
  artistName: string;
  trackName: string;
  albumName?: string;
  durationMs: number;
  /** Unix seconds when the track finished playing. */
  finishedAt: number;
}

/** Data for recording a skipped track */
export interface SkipData {
  trackMbid?: string;
  artistMbid?: string;
  releaseMbid?: string;
  jellyfinId: string;
  artistName: string;
  trackName: string;
  albumName?: string;
  playedMs: number;
  trackDurationMs: number;
  /** Unix seconds when the track was skipped. */
  skippedAt: number;
}

/** A single entry in listening history */
export interface ListenHistoryEntry {
  trackMbid?: string;
  artistName: string;
  trackName: string;
  albumName?: string;
  listenedAt: string;
  durationMs: number;
  jellyfinId?: string;
}

/** Scrobble-related commands */
export interface ScrobbleCommands {
  scrobble_track: (data: ScrobbleData) => Promise<boolean>;
  skip_track: (data: SkipData) => Promise<boolean>;
  sync_scrobbles: () => Promise<number>;
  get_sync_status: () => Promise<SyncStatus>;
  get_listening_history: (limit?: number, offset?: number) => Promise<ListenHistoryEntry[]>;
  connect_music_service: (serviceUrl: string, apiToken: string) => Promise<void>;
  disconnect_music_service: () => Promise<void>;
  get_music_service_status: () => Promise<MusicServiceStatus>;
  test_music_service_connection: () => Promise<boolean>;
}

/** Download queue item status */
export type DownloadStatus = 'pending' | 'downloading' | 'completed' | 'failed' | 'cancelled';

/** Download manager state */
export type DownloadState = 'idle' | 'downloading' | 'paused';

/** A queued download item */
export interface QueuedDownload {
  trackId: string;
  trackName: string;
  artistName: string;
  albumId?: string;
  status: DownloadStatus;
  progressPercent: number;
  bytesDownloaded: number;
  totalBytes?: number;
  error?: string;
  queuedAt: number;
}

/** Cache settings */
export interface CacheSettings {
  maxSizeBytes: number;
  currentSizeBytes: number;
  cachedTrackCount: number;
}

/** Download progress event */
export interface DownloadProgressEvent {
  type: 'progress';
  trackId: string;
  trackName: string;
  artistName: string;
  bytesDownloaded: number;
  totalBytes?: number;
  progressPercent: number;
}

/** Download completed event */
export interface DownloadCompletedEvent {
  type: 'completed';
  trackId: string;
  trackName: string;
  filePath: string;
  fileSizeBytes: number;
}

/** Download failed event */
export interface DownloadFailedEvent {
  type: 'failed';
  trackId: string;
  trackName: string;
  error: string;
}

/** Download cancelled event */
export interface DownloadCancelledEvent {
  type: 'cancelled';
  trackId: string;
}

/** Download queue changed event */
export interface DownloadQueueChangedEvent {
  type: 'queueChanged';
  queueLength: number;
}

/** Download state changed event */
export interface DownloadStateChangedEvent {
  type: 'stateChanged';
  state: DownloadState;
}

/** Cache evicted event (fired by "Clear all cache") */
export interface CacheEvictedEvent {
  type: 'cacheEvicted';
  evictedCount: number;
  freedBytes: number;
  /** Exactly the tracks that lost their cached file — clears only these
   * tracks' downloaded badges, never every badge in the library. */
  evictedTrackIds: string[];
}

/** All download event types */
export type DownloadEvent = DownloadProgressEvent | DownloadCompletedEvent | DownloadFailedEvent | DownloadCancelledEvent | DownloadQueueChangedEvent | DownloadStateChangedEvent | CacheEvictedEvent;

// ============================================================================
// Playlist Types
// ============================================================================

// ============================================================================
// Cache Stats Types
// ============================================================================

/** Cache statistics (library item counts) */
export interface CacheStats {
  albumCount: number;
  trackCount: number;
  artistCount: number;
  pendingSyncCount: number;
}

// ============================================================================
// App Info Types
// ============================================================================

/** Application info for About page */
export interface AppInfo {
  version: string;
  tauriVersion: string;
  platform: string;
  arch: string;
  dataDir: string;
}

// ============================================================================
// Developer Types
// ============================================================================

/** Developer/diagnostic information */
export interface DeveloperInfo {
  /** Path to the SQLite database file */
  databasePath: string;
  /** Size of the database file in bytes */
  databaseSizeBytes: number;
  /** Path to the audio cache directory */
  cacheDir: string;
  /** Whether running in debug mode */
  debugMode: boolean;
  /** Device ID used for scrobbling */
  deviceId?: string;
}

/** A queued sync operation */
export interface QueuedOperation {
  id: number;
  operation: string;
  payload: string;
  createdAt: number;
  attempts: number;
  lastAttemptAt?: number;
  syncedAt?: number;
}

// ============================================================================
// Library Sync Types
// ============================================================================

/** Metrics collected during a library sync operation */
export interface SyncMetrics {
  /** Total sync duration in milliseconds */
  durationMs: number;
  /** Time spent fetching from Jellyfin API (ms) */
  fetchTimeMs: number;
  /** Time spent writing to SQLite cache (ms) */
  dbTimeMs: number;
  /** Number of artists synced */
  artistCount: number;
  /** Number of albums synced */
  albumCount: number;
  /** Number of tracks synced */
  trackCount: number;
  /** Number of Jellyfin API requests made */
  apiRequests: number;
  /** Number of stale artists removed */
  staleArtists: number;
  /** Number of stale albums removed */
  staleAlbums: number;
  /** Number of stale tracks removed */
  staleTracks: number;
  /** Number of album track fetches that failed */
  failedFetches: number;
  /**
   * True when this trigger was coalesced by the single-flight guard because a
   * sync was already running — a benign no-op, not an error.
   */
  skipped?: boolean;
}

/**
 * Library sync lifecycle events: emitted from the one backend path
 * both the manual "Sync Now" trigger and the background scheduler funnel
 * through, so the frontend has a single, trigger-agnostic sync-progress model
 * instead of separate local booleans per trigger site.
 */
export interface LibrarySyncStartedEvent {
  backend: string;
}

export interface LibrarySyncCompletedEvent {
  backend: string;
  metrics: SyncMetrics;
}

export interface LibrarySyncFailedEvent {
  backend: string;
  error: string;
}

/** Automatic network-reachability signal — distinct from, and never
 * overriding, the user's manual offline-mode setting. */
export interface ConnectivityChangedEvent {
  reachable: boolean;
}

// ============================================================================
// Equalizer Types
// ============================================================================

/** Equalizer preset information */
export interface EqPresetInfo {
  /** Machine-readable preset name (snake_case) */
  name: string;
  /** Human-readable preset display name */
  displayName: string;
  /** 10-band gain values in dB (-20 to +20 typical range) */
  bands10: number[];
  /** 18-band gain values in dB (-20 to +20 typical range) */
  bands18: number[];
  /** true for shipped presets, false for user-saved profiles (renamable/deletable) */
  builtin: boolean;
}

/** Equalizer status from the backend */
export interface EqualizerStatus {
  /** Whether the equalizer is currently enabled */
  enabled: boolean;
  /** Name of the current preset */
  currentPreset: string;
  /** Current EQ band values */
  bands: number[];
}

/** Available EQ presets */
export type EqPresetName = 'flat' | 'rock' | 'pop' | 'classical' | 'electronic' | 'jazz' | 'hip_hop' | 'acoustic' | 'bass_boost' | 'treble_boost';

/** Equalizer-related commands */
export interface EqualizerCommands {
  get_eq_presets: () => Promise<EqPresetInfo[]>;
  get_eq_status: () => Promise<EqualizerStatus>;
  set_eq_enabled: (enabled: boolean) => Promise<void>;
  set_eq_preset: (presetName: string) => Promise<void>;
  set_eq_bands: (bands: number[]) => Promise<void>;
  save_eq_profile: (name: string, bands: number[]) => Promise<EqPresetInfo[]>;
  delete_eq_profile: (name: string) => Promise<EqPresetInfo[]>;
  get_eq_preset_info: (presetName: string) => Promise<EqPresetInfo>;
  reset_eq: () => Promise<void>;
}
