import type { Track } from '#lib/types/models.js';
import { getSettings, updateSettings } from '#lib/services/tauri.js';

/** Maximum number of recently played tracks to keep */
const MAX_RECENTLY_PLAYED = 200;

interface RecentlyPlayedEntry {
  track: Track;
  playedAt: number; // Unix timestamp in ms
}

interface RecentlyPlayedState {
  entries: RecentlyPlayedEntry[];
  isLoaded: boolean;
}

let recentlyPlayedState = $state<RecentlyPlayedState>({
  entries: [],
  isLoaded: false,
});

/** Get the recently played state */
export function getRecentlyPlayedState(): RecentlyPlayedState {
  return recentlyPlayedState;
}

/** Get just the tracks (most recent first) */
export function getRecentlyPlayedTracks(): Track[] {
  return recentlyPlayedState.entries.map((entry) => entry.track);
}

/** Get entry count */
export function getRecentlyPlayedCount(): number {
  return recentlyPlayedState.entries.length;
}

// Persist to SQLite (debounced)
let saveTimeout: ReturnType<typeof setTimeout> | null = null;

function persistEntries(entries: RecentlyPlayedEntry[]): void {
  if (saveTimeout) clearTimeout(saveTimeout);
  saveTimeout = setTimeout(() => {
    const serialised = JSON.stringify(entries);
    updateSettings({ recentlyPlayedJson: serialised }).catch((err) =>
      console.error('Failed to persist recently played:', err)
    );
  }, 500);
}

/**
 * Add a track to recently played.
 * If the track already exists, it moves to the top with updated timestamp.
 */
export function addToRecentlyPlayed(track: Track): void {
  // Never persist before the stored history has loaded: doing so would clobber
  // the on-disk list with a near-empty one. This guards both the pre-Phase-2
  // startup gap and the MiniPlayer window, which never calls loadRecentlyPlayed.
  if (!recentlyPlayedState.isLoaded) return;

  // Remove existing entry for this track (if any)
  const filtered = recentlyPlayedState.entries.filter(
    (entry) => entry.track.jellyfinId !== track.jellyfinId
  );

  // Add new entry at the beginning
  const newEntry: RecentlyPlayedEntry = {
    track,
    playedAt: Date.now(),
  };

  const entries = [newEntry, ...filtered].slice(0, MAX_RECENTLY_PLAYED);
  persistEntries(entries);

  recentlyPlayedState.entries = entries;
}

/**
 * Clear all recently played entries.
 */
export function clearRecentlyPlayed(): void {
  recentlyPlayedState = { entries: [], isLoaded: true };
  persistEntries([]);
}

/**
 * Load recently played from SQLite settings. Call once at app startup.
 */
export async function loadRecentlyPlayed(): Promise<void> {
  try {
    const settings = await getSettings();
    const json = settings.recentlyPlayedJson;

    if (json) {
      const entries = JSON.parse(json) as RecentlyPlayedEntry[];
      // Validate entries have required fields
      const validEntries = entries.filter(
        (entry) =>
          entry.track?.jellyfinId && entry.track?.name && typeof entry.playedAt === 'number'
      );
      recentlyPlayedState = { entries: validEntries, isLoaded: true };
    } else {
      recentlyPlayedState = { entries: [], isLoaded: true };
    }
  } catch (err) {
    console.error('Failed to load recently played:', err);
    recentlyPlayedState = { entries: [], isLoaded: true };
  }
}
