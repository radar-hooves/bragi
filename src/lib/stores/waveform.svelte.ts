/**
 * Waveform store.
 *
 * In-memory, per-session cache of precomputed peak arrays keyed by track ID,
 * plus a lookahead prefetch so advancing to the next queued track paints its
 * waveform instantly instead of stalling on a decode/download.
 *
 * The peaks themselves are computed and cached on disk by the backend
 * (`get_track_waveform`); this store just deduplicates in-flight requests and
 * remembers results (including a genuine `null` "no waveform") for the session.
 */

import { getTrackWaveform } from '#lib/services/tauri.js';

/** `number[]` = peaks, `null` = definitively no waveform (don't refetch). */
const cache = new Map<string, number[] | null>();
/** In-flight loads, so concurrent callers (display + prefetch) share one call. */
const inflight = new Map<string, Promise<number[] | null>>();

/**
 * How many upcoming queued tracks to precompute ahead of the current one.
 *
 * One is enough to make a normal track change seamless while keeping the
 * background decode/transcode cost modest.
 */
const PREFETCH_AHEAD = 1;

/**
 * Load a track's waveform peaks, using the session cache when available.
 *
 * Returns `null` when the track has no drawable waveform. A successful result
 * (peaks or a genuine `null`) is cached; a transient failure (offline, server
 * error) is NOT cached, so it recovers on a later attempt — fully independent
 * of playback, which never waits on this.
 */
export async function loadWaveform(trackId: string): Promise<number[] | null> {
  if (cache.has(trackId)) return cache.get(trackId) ?? null;
  const existing = inflight.get(trackId);
  if (existing) return existing;

  const request = (async () => {
    try {
      const peaks = await getTrackWaveform(trackId);
      cache.set(trackId, peaks);
      return peaks;
    } catch {
      return null; // transient — leave uncached so it retries later
    } finally {
      inflight.delete(trackId);
    }
  })();

  inflight.set(trackId, request);
  return request;
}

/**
 * Warm the cache for the next queued track(s) so advancing is seamless.
 *
 * Fire-and-forget: it kicks off (at most `PREFETCH_AHEAD`) background loads for
 * upcoming track IDs that aren't already cached or in flight, then returns
 * immediately. Errors are swallowed inside `loadWaveform`.
 */
export function prefetchWaveforms(upcomingTrackIds: string[]): void {
  for (const id of upcomingTrackIds.slice(0, PREFETCH_AHEAD)) {
    if (id && !cache.has(id) && !inflight.has(id)) {
      void loadWaveform(id);
    }
  }
}
