/**
 * Tauri event handlers for playback and sync events.
 * Sets up listeners for events emitted by the Rust backend.
 */

import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  getPlaybackState,
  updatePlaybackState,
  setCurrentTrack,
  setPlaying,
  setCurrentTime,
} from '#lib/stores/player.svelte.js';
import { setQueueState } from '#lib/stores/queue.svelte.js';
import {
  scrobbleTrack,
  skipTrack,
  shouldScrobble,
  refreshSyncStatus,
} from '#lib/stores/scrobble.svelte.js';
import { addToRecentlyPlayed } from '#lib/stores/recentlyPlayed.svelte.js';
import {
  handleDownloadProgress,
  handleDownloadCompleted,
  handleDownloadFailed,
  handleDownloadCancelled,
  handleQueueChanged as handleDownloadQueueChanged,
  handleStateChanged as handleDownloadStateChanged,
  handleCacheEvicted,
  refreshDownloadQueue,
  refreshCacheSettings,
} from '#lib/stores/downloads.svelte.js';
import { toast } from 'svelte-sonner';
import { setCurrentSection, requestSettingsCategory } from '#lib/stores/ui.svelte.js';
import { navigateTo } from '#lib/stores/navigation.svelte.js';
import { getAuthExpired, setAuthExpired } from '#lib/stores/jellyfin.svelte.js';
import { handleSyncStarted, handleSyncCompleted, handleSyncFailed } from '#lib/stores/syncStatus.svelte.js';
import { setNetworkReachable } from '#lib/stores/offlineMode.svelte.js';
import type { Track, PlaybackState, QueueEntrySource } from '#lib/types/models.js';
import type {
  ScrobbleData,
  SkipData,
  DownloadProgressEvent,
  DownloadCompletedEvent,
  DownloadFailedEvent,
  DownloadCancelledEvent,
  DownloadQueueChangedEvent,
  DownloadStateChangedEvent,
  CacheEvictedEvent,
  AudioDevice,
  AudioDevicesChangedEvent,
  LibrarySyncStartedEvent,
  LibrarySyncCompletedEvent,
  LibrarySyncFailedEvent,
  ConnectivityChangedEvent,
} from '#lib/types/tauri.js';
import { setAudioDevices } from '#lib/stores/audioDevices.svelte.js';

/** Playback state changed event payload */
interface StateChangedEvent {
  type: 'stateChanged';
  currentTrack: Track | null;
  isPlaying: boolean;
  currentTimeMs: number;
  durationMs: number;
  volume: number;
  shuffle: boolean;
  repeat: 'off' | 'all' | 'one';
  outputDevice: string | null;
}

/** Progress update event payload */
interface ProgressEvent {
  type: 'progress';
  currentTimeMs: number;
  durationMs: number;
}

/** Queue changed event payload */
interface QueueChangedEvent {
  type: 'queueChanged';
  tracks: Track[];
  currentIndex: number;
  sources: QueueEntrySource[];
}

/** Track finished event payload */
interface TrackFinishedEvent {
  type: 'trackFinished';
  track: Track;
  playedMs: number;
  finishedAt: number; // Unix seconds
}

/** Playback error event payload */
interface PlaybackErrorEvent {
  type: 'error';
  message: string;
}

type PlaybackEvent =
  | StateChangedEvent
  | ProgressEvent
  | QueueChangedEvent
  | TrackFinishedEvent
  | PlaybackErrorEvent;

/** Unlisten functions for cleanup */
let unlisteners: UnlistenFn[] = [];

/**
 * Whether this window owns the single-fire side effects (scrobble/skip on
 * track-finished, recently-played history, error/auth toasts, download + device
 * events). The main window owns them; the MiniPlayer window does NOT — it is a
 * second webview that receives the same broadcast events, so running the side
 * effects there too would double-scrobble and clobber shared state.
 */
let ownsSideEffects = true;

/** Track the last played track ID to detect track changes */
let lastPlayedTrackId: string | null = null;

/**
 * Handle playback state changed event.
 */
function handleStateChanged(event: StateChangedEvent): void {
  // Track when a new track starts playing (add to recently played). Only the
  // side-effect-owning window records history, so the MiniPlayer never writes.
  if (
    ownsSideEffects &&
    event.currentTrack &&
    event.isPlaying &&
    event.currentTrack.jellyfinId !== lastPlayedTrackId
  ) {
    lastPlayedTrackId = event.currentTrack.jellyfinId;
    addToRecentlyPlayed(event.currentTrack);
  }

  updatePlaybackState({
    currentTrack: event.currentTrack,
    isPlaying: event.isPlaying,
    currentTimeMs: event.currentTimeMs,
    durationMs: event.durationMs,
    volume: event.volume,
    // shuffle and repeat intentionally omitted: the frontend
    // toggles + persisted preferences are the source of truth for these flags,
    // while the engine owns the resulting queue order / next-track behaviour
    // (driven via setShuffle/setRepeat). Echoing the engine's flags here would
    // revert a just-set toggle on the next state event, since the engine defaults
    // them off at startup and only learns the preference once playback begins.
  });
}

/**
 * Handle progress update event.
 */
function handleProgress(event: ProgressEvent): void {
  updatePlaybackState({
    currentTimeMs: event.currentTimeMs,
    durationMs: event.durationMs,
  });
}

/**
 * Handle queue changed event.
 */
function handleQueueChanged(event: QueueChangedEvent): void {
  setQueueState({
    tracks: event.tracks,
    currentIndex: event.currentIndex,
    sources: event.sources,
  });
}

/** Last scrobbled track ID for frontend deduplication */
let lastScrobbledId: string | null = null;
/** Timestamp of last scrobble for frontend deduplication */
let lastScrobbledAt = 0;

/**
 * Handle track finished event.
 * This is where scrobbling and skip tracking are triggered.
 */
async function handleTrackFinished(event: TrackFinishedEvent): Promise<void> {
  const { track, playedMs, finishedAt } = event;

  // Dedup: ignore if same track finished within 2 seconds
  const now = Date.now();
  if (track.jellyfinId === lastScrobbledId && now - lastScrobbledAt < 2000) {
    return;
  }
  lastScrobbledId = track.jellyfinId;
  lastScrobbledAt = now;

  // Check if we should scrobble (played >50% or >4 minutes)
  if (shouldScrobble(playedMs, track.durationMs)) {
    const scrobbleData: ScrobbleData = {
      trackMbid: track.mbid,
      artistMbid: track.artistMbid,
      releaseMbid: track.releaseMbid,
      jellyfinId: track.jellyfinId,
      artistName: track.artistName,
      trackName: track.name,
      albumName: track.albumName,
      durationMs: playedMs,
      finishedAt,
    };

    try {
      await scrobbleTrack(scrobbleData);
    } catch (e) {
      console.error('Failed to scrobble track:', e);
    }
  } else if (playedMs > 0 && track.durationMs > 0) {
    // Track was skipped (played < threshold)
    const skipData: SkipData = {
      trackMbid: track.mbid,
      artistMbid: track.artistMbid,
      releaseMbid: track.releaseMbid,
      jellyfinId: track.jellyfinId,
      artistName: track.artistName,
      trackName: track.name,
      albumName: track.albumName,
      playedMs: playedMs,
      trackDurationMs: track.durationMs,
      skippedAt: finishedAt,
    };

    try {
      await skipTrack(skipData);
    } catch (e) {
      console.error('Failed to record skip:', e);
    }
  }
}

/**
 * Handle playback error event.
 * Shows a user-facing notification for playback failures.
 */
function handlePlaybackError(event: PlaybackErrorEvent): void {
  console.error('Playback error:', event.message);

  const message = event.message;
  let title = 'Playback failed';
  let description = message;

  if (message.includes('Authentication failed') || message.includes('401')) {
    title = 'Authentication failed';
    description = 'Try reconnecting to Jellyfin in Settings.';
  } else if (message.includes('Access denied') || message.includes('403')) {
    title = 'Access denied';
    description = 'Check your Jellyfin permissions.';
  } else if (message.includes('not found') || message.includes('404')) {
    title = 'Track not found';
    description = 'The track may have been removed. Try syncing your library.';
  } else if (message.includes('timed out') || message.includes('timeout')) {
    title = 'Connection timed out';
    description = 'Check your network connection and try again.';
  } else if (message.includes('Connection failed') || message.includes('connect')) {
    title = 'Connection failed';
    description = 'Unable to reach Jellyfin server. Check your network.';
  } else if (message.includes('stalled')) {
    title = 'Stream stalled';
    description = 'Network too slow or disconnected. Try again.';
  } else if (message.includes('decode') || message.includes('Decoder')) {
    title = 'Cannot play track';
    description = 'Audio format not supported or file is corrupt.';
  }

  toast.error(title, { description });
}

/** Jellyfin session-expiry event payload. */
interface AuthExpiredEvent {
  message: string;
}

// Id of the persistent "reconnect" toast, kept so it can be dismissed once the
// user successfully reconnects (see dismissAuthExpiredToast).
let authExpiredToastId: string | number | undefined;

// Id of the "can't reach your server" toast, dismissed once
// connectivity is restored rather than left to linger or stack.
let connectivityLostToastId: string | number | undefined;

/**
 * Handle the backend's automatic connectivity signal: distinct
 * from, and never overriding, the user's manual offline-mode toggle. A lost
 * connection surfaces a toast the same way Spotify shows "Spotify is
 * offline" — visible, but the app keeps working from the local cache and
 * downloads rather than blocking.
 */
function handleConnectivityChanged(event: ConnectivityChangedEvent): void {
  setNetworkReachable(event.reachable);

  if (!event.reachable) {
    connectivityLostToastId = toast.warning("Can't reach your server", {
      description: 'Playing from your downloads and cached library until the connection returns.',
      duration: Infinity,
    });
    return;
  }

  if (connectivityLostToastId !== undefined) {
    toast.dismiss(connectivityLostToastId);
    connectivityLostToastId = undefined;
    toast.success('Back online');
  }
}

/** Route the user to the Library settings page, where the servers live, to reconnect. */
function routeToConnections(): void {
  requestSettingsCategory('library');
  setCurrentSection('settings');
  navigateTo({ section: 'settings', selectedArtistId: null, selectedAlbumId: null });
}

/**
 * Handle a Jellyfin session-expiry event.
 *
 * The backend deduplicates emission (one event per expired session), and we
 * additionally guard on the store flag so a stale duplicate can never stack a
 * second toast. Surfaces a persistent prompt whose action routes to the Library pane
 * instead of letting sync/stream calls fail silently.
 */
function handleAuthExpired(_event: AuthExpiredEvent): void {
  if (getAuthExpired()) return;
  setAuthExpired(true);
  authExpiredToastId = toast.error('Jellyfin session expired', {
    description: 'Your login is no longer valid. Reconnect to resume syncing and streaming.',
    duration: Infinity,
    action: {
      label: 'Reconnect',
      onClick: () => routeToConnections(),
    },
  });
}

/**
 * Dismiss the persistent auth-expiry toast if one is showing.
 *
 * Called from the reconnect success path (jellyfin store `resumeAfterReconnect`)
 * so the infinite-duration toast does not linger after a successful reconnect.
 */
export function dismissAuthExpiredToast(): void {
  if (authExpiredToastId !== undefined) {
    toast.dismiss(authExpiredToastId);
    authExpiredToastId = undefined;
  }
}

/**
 * Set up all Tauri event listeners.
 * Call this once when the app initialises.
 */
export async function setupEventListeners(
  options: { ownsSideEffects?: boolean } = {}
): Promise<void> {
  // Clean up any existing listeners
  await cleanupEventListeners();

  ownsSideEffects = options.ownsSideEffects ?? true;

  // ── UI-sync listeners (every window, incl. the MiniPlayer) ──────────────────
  const unlistenState = await listen<StateChangedEvent>('playback-state-changed', (event) => {
    handleStateChanged(event.payload);
  });
  unlisteners.push(unlistenState);

  const unlistenProgress = await listen<ProgressEvent>('playback-progress', (event) => {
    handleProgress(event.payload);
  });
  unlisteners.push(unlistenProgress);

  const unlistenQueue = await listen<QueueChangedEvent>('queue-changed', (event) => {
    handleQueueChanged(event.payload);
  });
  unlisteners.push(unlistenQueue);

  // Playback errors surface a toast; it belongs in whichever window is visible.
  // The replace model keeps exactly one of {main, mini} on screen, so registering
  // it in both can't produce a visible duplicate — and without it the MiniPlayer
  // would show playback simply stop with no explanation. (The mini renders its
  // own <Toaster /> so the toast has somewhere to go.)
  const unlistenError = await listen<PlaybackErrorEvent>('playback-error', (event) => {
    handlePlaybackError(event.payload);
  });
  unlisteners.push(unlistenError);

  // ── Single-fire side effects (main window only) ─────────────────────────────
  // The backend broadcasts these events to ALL windows, so registering them in
  // the MiniPlayer too would double-scrobble/skip and race shared state. Only the
  // owning (main) window handles them.
  if (!ownsSideEffects) {
    return;
  }

  const unlistenFinished = await listen<TrackFinishedEvent>('track-finished', (event) => {
    handleTrackFinished(event.payload);
  });
  unlisteners.push(unlistenFinished);

  const unlistenAuthExpired = await listen<AuthExpiredEvent>('jellyfin-auth-expired', (event) => {
    handleAuthExpired(event.payload);
  });
  unlisteners.push(unlistenAuthExpired);

  const unlistenDevices = await listen<AudioDevicesChangedEvent>(
    'audio-devices-changed',
    (event) => {
      setAudioDevices(event.payload.devices);
    }
  );
  unlisteners.push(unlistenDevices);

  // The background playlist-cache prefetch finished warming the local
  // cache. Re-broadcast as a DOM event so the sidebar (PlaylistList) re-reads
  // the now-populated cache and swaps the generic icon for the collage — without
  // it, the once-per-session cache-read guard would keep the icon until the next
  // relaunch.
  const unlistenPlaylistsWarmed = await listen('playlists-cache-warmed', () => {
    window.dispatchEvent(new CustomEvent('bragi-playlists-warmed'));
  });
  unlisteners.push(unlistenPlaylistsWarmed);

  await setupDownloadEventListeners();
  await setupSyncEventListeners();
}

/**
 * Clean up all event listeners.
 */
export async function cleanupEventListeners(): Promise<void> {
  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners = [];
}

/**
 * Initialise all app state from backend.
 */
export async function initAppState(): Promise<void> {
  await refreshSyncStatus();
  await refreshDownloadQueue();
  await refreshCacheSettings();
}

// Download event handlers

async function setupDownloadEventListeners(): Promise<void> {
  const unlistenProgress = await listen<DownloadProgressEvent>('download-progress', (event) => {
    handleDownloadProgress(event.payload);
  });
  unlisteners.push(unlistenProgress);

  const unlistenCompleted = await listen<DownloadCompletedEvent>('download-completed', (event) => {
    handleDownloadCompleted(event.payload);
  });
  unlisteners.push(unlistenCompleted);

  const unlistenFailed = await listen<DownloadFailedEvent>('download-failed', (event) => {
    handleDownloadFailed(event.payload);
  });
  unlisteners.push(unlistenFailed);

  const unlistenCancelled = await listen<DownloadCancelledEvent>('download-cancelled', (event) => {
    handleDownloadCancelled(event.payload.trackId);
  });
  unlisteners.push(unlistenCancelled);

  const unlistenQueueChanged = await listen<DownloadQueueChangedEvent>(
    'download-queue-changed',
    () => {
      handleDownloadQueueChanged();
    }
  );
  unlisteners.push(unlistenQueueChanged);

  const unlistenStateChanged = await listen<DownloadStateChangedEvent>(
    'download-state-changed',
    (event) => {
      handleDownloadStateChanged(event.payload.state);
    }
  );
  unlisteners.push(unlistenStateChanged);

  const unlistenCacheEvicted = await listen<CacheEvictedEvent>('download-cache-evicted', (event) => {
    handleCacheEvicted(event.payload.evictedTrackIds);
  });
  unlisteners.push(unlistenCacheEvicted);
}

// Sync lifecycle + connectivity event handlers

async function setupSyncEventListeners(): Promise<void> {
  const unlistenSyncStarted = await listen<LibrarySyncStartedEvent>(
    'library-sync-started',
    (event) => {
      handleSyncStarted(event.payload.backend);
    }
  );
  unlisteners.push(unlistenSyncStarted);

  const unlistenSyncCompleted = await listen<LibrarySyncCompletedEvent>(
    'library-sync-completed',
    (event) => {
      handleSyncCompleted(event.payload.backend, event.payload.metrics);
    }
  );
  unlisteners.push(unlistenSyncCompleted);

  const unlistenSyncFailed = await listen<LibrarySyncFailedEvent>(
    'library-sync-failed',
    (event) => {
      handleSyncFailed(event.payload.backend, event.payload.error);
    }
  );
  unlisteners.push(unlistenSyncFailed);

  const unlistenScrobbleDrained = await listen('scrobble-queue-drained', async () => {
    await refreshSyncStatus();
  });
  unlisteners.push(unlistenScrobbleDrained);

  const unlistenConnectivity = await listen<ConnectivityChangedEvent>(
    'connectivity-changed',
    (event) => {
      handleConnectivityChanged(event.payload);
    }
  );
  unlisteners.push(unlistenConnectivity);
}
