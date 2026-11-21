import type { PlaybackState, Track } from '#lib/types/models.js';
import { getSettings, updateSettings, setShuffle, setRepeat } from '#lib/services/tauri.js';
import { reportTransportError } from '#lib/utils/transportError.js';

const initialPlaybackState: PlaybackState = {
  currentTrack: null,
  isPlaying: false,
  currentTimeMs: 0,
  durationMs: 0,
  volume: 1.0,
  shuffle: false,
  repeat: 'off',
};

let playbackState = $state<PlaybackState>({ ...initialPlaybackState });

// Persist playback preferences to SQLite (debounced)
let saveTimeout: ReturnType<typeof setTimeout> | null = null;
let previousPrefs = {
  shuffle: initialPlaybackState.shuffle,
  repeatMode: initialPlaybackState.repeat,
};

$effect.root(() => {
  $effect(() => {
    const current = {
      shuffle: playbackState.shuffle,
      repeatMode: playbackState.repeat,
    };

    if (
      current.shuffle === previousPrefs.shuffle &&
      current.repeatMode === previousPrefs.repeatMode
    ) {
      return;
    }

    previousPrefs = { ...current };

    if (saveTimeout) clearTimeout(saveTimeout);
    saveTimeout = setTimeout(() => {
      updateSettings(current).catch((err) =>
        console.error('Failed to persist playback preferences:', err)
      );
    }, 300);
  });
});

/** Get the playback state */
export function getPlaybackState(): PlaybackState {
  return playbackState;
}

/** Update the playback state (merge partial updates) */
export function updatePlaybackState(updates: Partial<PlaybackState>): void {
  Object.assign(playbackState, updates);
}

/** Load persisted playback preferences from SQLite. Call once at app startup. */
export async function loadPlaybackPreferences(): Promise<void> {
  try {
    const settings = await getSettings();
    const repeat = (['off', 'all', 'one'] as const).includes(
      settings.repeatMode as 'off' | 'all' | 'one'
    )
      ? (settings.repeatMode as 'off' | 'all' | 'one')
      : 'off';
    previousPrefs = {
      shuffle: settings.shuffle,
      repeatMode: repeat,
    };
    playbackState.shuffle = settings.shuffle;
    playbackState.repeat = repeat;
  } catch (err) {
    console.error('Failed to load playback preferences:', err);
  }
}

/** Progress as a percentage (0-100) */
export function getProgress(): number {
  return playbackState.durationMs
    ? (playbackState.currentTimeMs / playbackState.durationMs) * 100
    : 0;
}

/** Formatted current time (mm:ss) */
export function getCurrentTimeFormatted(): string {
  return formatDuration(playbackState.currentTimeMs);
}

/** Formatted total duration (mm:ss) */
export function getDurationFormatted(): string {
  return formatDuration(playbackState.durationMs);
}

/** Formatted remaining time (mm:ss) */
export function getRemainingTimeFormatted(): string {
  return formatDuration(Math.max(0, playbackState.durationMs - playbackState.currentTimeMs));
}

/** Whether a track is currently loaded */
export function getHasTrack(): boolean {
  return playbackState.currentTrack !== null;
}

function formatDuration(ms: number): string {
  const totalSeconds = Math.floor(ms / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, '0')}`;
}

// Actions
export function setCurrentTrack(track: Track | null): void {
  playbackState.currentTrack = track;
  playbackState.currentTimeMs = 0;
  playbackState.durationMs = track?.durationMs ?? 0;
}

export function setPlaying(isPlaying: boolean): void {
  playbackState.isPlaying = isPlaying;
}

export function setCurrentTime(currentTimeMs: number): void {
  playbackState.currentTimeMs = currentTimeMs;
}

export function toggleShuffle(): void {
  // Optimistically flip the UI flag, then drive the engine, which actually
  // reorders the queue. The persistence $effect saves the preference to
  // SQLite; handleStateChanged intentionally does NOT echo shuffle back, so a
  // slow/failed IPC can't revert this local flip.
  playbackState.shuffle = !playbackState.shuffle;
  setShuffle(playbackState.shuffle).catch((err) => reportTransportError('Shuffle', err));
}

export function cycleRepeat(): void {
  // Optimistically advance the UI mode, then drive the engine, which
  // actually honours it in the queue layer. Mirrors toggleShuffle: the
  // persistence $effect saves it, and handleStateChanged does NOT echo repeat
  // back, so a slow/failed IPC can't revert this local change.
  const modes: PlaybackState['repeat'][] = ['off', 'all', 'one'];
  const currentIndex = modes.indexOf(playbackState.repeat);
  const nextIndex = (currentIndex + 1) % modes.length;
  playbackState.repeat = modes[nextIndex]!;
  setRepeat(playbackState.repeat).catch((err) => reportTransportError('Repeat', err));
}
