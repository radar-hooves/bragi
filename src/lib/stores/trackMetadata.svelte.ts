// Per-track supplementary metadata that doesn't live on the Track model.
//
// Merges three data sources:
// 1. Scrobble-server stats (play count, skip count, last played/skipped) cached in SQLite
// 2. Server-side play counts the media server itself reports (Jellyfin
//    UserData.PlayCount / OpenSubsonic inline playCount), keyed by server id
// 3. Local session data (recently played timestamps from this session)
//
// Play-count precedence mirrors the ai_playlist "auto" tier: the media
// server's own count wins when it has recorded any plays, else the scrobble server's. This
// lets the "Plays" column work for a backend-only user with no scrobble service
// configured (no scrobble-server stats) — while still preferring those when the server
// reports nothing.
//
// Column renderers and sort functions read from getTrackMeta() which
// provides O(1) lookups by Jellyfin ID.

import { untrack } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import { getRecentlyPlayedState } from '#lib/stores/recentlyPlayed.svelte.js';
import { getLibraryState } from '#lib/stores/library.svelte.js';

export interface TrackMeta {
  playCount: number;
  skipCount: number;
  lastPlayedMs: number;
}

/** Stats entry as returned by the refresh_track_stats Tauri command. */
interface CachedTrackStats {
  playCount: number;
  skipCount: number;
  lastPlayedAt: number; // Unix seconds
  lastSkippedAt: number;
}

const EMPTY: TrackMeta = { playCount: 0, skipCount: 0, lastPlayedMs: 0 };

// Scrobble-server stats keyed by MBID (populated via loadEarwormStats)
let earwormStats = $state<Map<string, CachedTrackStats>>(new Map());

// Server-side play counts keyed by server (Jellyfin) id, from the media server
// itself (populated via loadServerPlayCounts). Read-only.
let serverPlayCounts = $state<Map<string, number>>(new Map());

// jellyfinId -> MBID lookup, rebuilt when library tracks change
let jellyfinToMbid = $derived.by(() => {
  // Track the array ref, but iterate the 23k+ deeply-reactive tracks under
  // untrack(): reading each track's props inside a $derived otherwise registers
  // a per-property dependency and hits Svelte 5's O(n^2) dependency-tracking
  // cliff (recomputed on every setTracks, i.e. initial load and each sync).
  const tracks = getLibraryState().tracks;
  return untrack(() => {
    const map = new Map<string, string>();
    for (const track of tracks) {
      if (track.mbid) {
        map.set(track.jellyfinId, track.mbid);
      }
    }
    return map;
  });
});

// Recently played index: jellyfinId -> most recent playedAt (ms)
let recentIndex = $derived.by(() => {
  const map = new Map<string, number>();
  for (const entry of getRecentlyPlayedState().entries) {
    const id = entry.track.jellyfinId;
    const existing = map.get(id);
    if (!existing || entry.playedAt > existing) {
      map.set(id, entry.playedAt);
    }
  }
  return map;
});

/** O(1) lookup of supplementary metadata for a track. */
export function getTrackMeta(jellyfinId: string): TrackMeta {
  const mbid = jellyfinToMbid.get(jellyfinId);
  const stats = mbid ? earwormStats.get(mbid) : undefined;
  const recentMs = recentIndex.get(jellyfinId) ?? 0;
  // Server count is keyed by the server id (not MBID); 0/absent means "no info".
  const serverCount = serverPlayCounts.get(jellyfinId) ?? 0;

  if (!stats) {
    // No scrobble-server data. Surface the server's own count if it has one; otherwise
    // fall back to local session data only.
    return serverCount > 0 || recentMs > 0
      ? { playCount: serverCount, skipCount: 0, lastPlayedMs: recentMs }
      : EMPTY;
  }

  // The scrobble server lastPlayedAt is Unix seconds; convert to ms for comparison
  const earwormLastPlayedMs = stats.lastPlayedAt * 1000;

  return {
    // Precedence: the media server's own count wins when it recorded plays,
    // else the scrobble server (mirrors the ai_playlist "auto" tier).
    playCount: serverCount > 0 ? serverCount : stats.playCount,
    skipCount: stats.skipCount,
    lastPlayedMs: Math.max(earwormLastPlayedMs, recentMs),
  };
}

/** Load track stats from the scrobble server (fetches from network + SQLite cache). */
export async function loadEarwormStats(): Promise<void> {
  try {
    const result = await invoke<Record<string, CachedTrackStats>>('refresh_track_stats');
    const map = new Map<string, CachedTrackStats>();
    for (const [mbid, stats] of Object.entries(result)) {
      map.set(mbid, stats);
    }
    earwormStats = map;
  } catch (e) {
    console.error('Failed to load scrobble-server stats:', e);
  }
}

/**
 * Load server-side play counts from the active media server's cache (populated
 * during library sync). Keyed by server (Jellyfin) id. Read-only; safe to call
 * alongside loadEarwormStats.
 */
export async function loadServerPlayCounts(): Promise<void> {
  try {
    const result = await invoke<Record<string, number>>('get_server_play_counts');
    const map = new Map<string, number>();
    for (const [id, count] of Object.entries(result)) {
      map.set(id, count);
    }
    serverPlayCounts = map;
  } catch (e) {
    console.error('Failed to load server play counts:', e);
  }
}

/** Optimistic play count increment after a successful scrobble. */
export function incrementPlayCount(jellyfinId: string): void {
  const mbid = jellyfinToMbid.get(jellyfinId);
  if (!mbid) return;

  const existing = earwormStats.get(mbid);
  const now = Math.floor(Date.now() / 1000);

  const updated = new Map(earwormStats);
  updated.set(mbid, {
    playCount: (existing?.playCount ?? 0) + 1,
    skipCount: existing?.skipCount ?? 0,
    lastPlayedAt: now,
    lastSkippedAt: existing?.lastSkippedAt ?? 0,
  });
  earwormStats = updated;
}

/** Optimistic skip count increment after a successful skip. */
export function incrementSkipCount(jellyfinId: string): void {
  const mbid = jellyfinToMbid.get(jellyfinId);
  if (!mbid) return;

  const existing = earwormStats.get(mbid);
  const now = Math.floor(Date.now() / 1000);

  const updated = new Map(earwormStats);
  updated.set(mbid, {
    playCount: existing?.playCount ?? 0,
    skipCount: (existing?.skipCount ?? 0) + 1,
    lastPlayedAt: existing?.lastPlayedAt ?? 0,
    lastSkippedAt: now,
  });
  earwormStats = updated;
}
