/**
 * Lyrics store.
 *
 * Holds the show/hide state of the lyrics panel plus an in-memory,
 * per-session cache of fetched lyrics keyed by track ID.
 *
 * Visibility is intentionally NOT persisted across restarts to keep scope
 * small; if persistence is wanted later, move the flag into `ui.svelte.ts`
 * (the `statusBarVisible` pattern) and back it with a settings key.
 */

import { getTrackLyrics, getSettings, updateSettings } from '#lib/services/tauri.js';
import type { TrackLyrics } from '#lib/types/models.js';

/** Whether the lyrics panel is currently shown. */
let visible = $state(false);

// ── Manual lyrics timing offset ──────────────────────────────────────
// One global, signed correction (ms) applied to synced-lyric highlighting for
// tracks whose timing drifts. Positive shows lyrics earlier (ahead of audio),
// negative delays them. Persisted in SQLite; local-only, never sent to Jellyfin.

/** Step per +/- tap. */
export const LYRICS_OFFSET_STEP_MS = 250;
/** Clamp bounds so a stuck key can't push the offset absurdly far. */
const LYRICS_OFFSET_MIN_MS = -10_000;
const LYRICS_OFFSET_MAX_MS = 10_000;

let offsetMs = $state(0);
let offsetSaveTimeout: ReturnType<typeof setTimeout> | null = null;

/** Current lyrics offset in ms. Read inside reactive contexts for live updates. */
export function getLyricsOffsetMs(): number {
  return offsetMs;
}

function persistLyricsOffset(): void {
  // Debounced: rapid +/- taps collapse into one write, like ui.svelte.ts.
  if (offsetSaveTimeout) clearTimeout(offsetSaveTimeout);
  offsetSaveTimeout = setTimeout(() => {
    updateSettings({ lyricsOffsetMs: offsetMs }).catch((err) =>
      console.error('Failed to persist lyrics offset:', err)
    );
  }, 300);
}

/** Set the offset to an absolute value (clamped) and persist. */
export function setLyricsOffsetMs(value: number): void {
  offsetMs = Math.max(LYRICS_OFFSET_MIN_MS, Math.min(LYRICS_OFFSET_MAX_MS, Math.round(value)));
  persistLyricsOffset();
}

/** Nudge the offset by a delta (e.g. +/- a step) and persist. */
export function adjustLyricsOffsetMs(deltaMs: number): void {
  setLyricsOffsetMs(offsetMs + deltaMs);
}

/** Reset the offset to 0 and persist. */
export function resetLyricsOffset(): void {
  setLyricsOffsetMs(0);
}

/** Load the persisted offset from SQLite. Call once at app startup. */
export async function loadLyricsOffset(): Promise<void> {
  try {
    const value = (await getSettings()).lyricsOffsetMs ?? 0;
    offsetMs = Math.max(LYRICS_OFFSET_MIN_MS, Math.min(LYRICS_OFFSET_MAX_MS, Math.round(value)));
  } catch (err) {
    console.error('Failed to load lyrics offset:', err);
  }
}

/**
 * Per-session cache of fetched lyrics, keyed by track ID.
 *
 * A `null` value records "this track has no lyrics" so we don't refetch.
 */
const cache = new Map<string, TrackLyrics | null>();

/** Whether the lyrics panel is currently shown. */
export function getLyricsVisible(): boolean {
  return visible;
}

/** Toggle the lyrics panel open/closed. */
export function toggleLyrics(): void {
  visible = !visible;
}

/** Explicitly set the lyrics panel visibility. */
export function setLyricsVisible(value: boolean): void {
  visible = value;
}

/**
 * Load lyrics for a track, using the in-memory cache when available.
 *
 * Returns `null` when the track has no lyrics. Caches both hits and misses
 * to avoid redundant round-trips within a session.
 */
export async function loadLyrics(trackId: string): Promise<TrackLyrics | null> {
  if (cache.has(trackId)) {
    return cache.get(trackId) ?? null;
  }

  // A successful result (lyrics, or a genuine null "no lyrics") is cached. A
  // FAILURE (server busy, timeout, transient error) is not cached and is
  // retried with backoff, so lyrics recover once a sync/load settles — fully
  // independent of playback, which never waits on this.
  const maxAttempts = 3;
  let lastError: unknown;
  for (let attempt = 0; attempt < maxAttempts; attempt++) {
    try {
      const lyrics = await getTrackLyrics(trackId);
      cache.set(trackId, lyrics);
      return lyrics;
    } catch (e) {
      lastError = e;
      if (attempt < maxAttempts - 1) {
        await new Promise((r) => setTimeout(r, 600 * 2 ** attempt)); // 0.6s, 1.2s
      }
    }
  }
  throw lastError;
}
