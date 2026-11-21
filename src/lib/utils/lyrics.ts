// Pure helpers for synced-lyrics highlighting.
//
// Kept free of Svelte reactivity so the logic is trivially testable and the
// LyricsPanel component stays thin.

import type { LyricsLine, TrackLyrics } from '#lib/types/models.js';

/**
 * Index of the currently-active lyric line for a given playback position.
 *
 * Returns the index of the last line whose `startMs` is defined and less than
 * or equal to `positionMs`, or `-1` when no line is active yet (position before
 * the first timestamped line, empty input, or fully unsynced lyrics where every
 * `startMs` is undefined).
 *
 * Lines are assumed to be in ascending time order (as Jellyfin returns them).
 * A `startMs` of `0` is a valid, active-able timestamp — it is never skipped.
 */
export function activeLyricIndex(lines: LyricsLine[], positionMs: number): number {
  let active = -1;
  for (let i = 0; i < lines.length; i++) {
    const startMs = lines[i]?.startMs;
    if (startMs === undefined) continue;
    if (startMs <= positionMs) {
      active = i;
    } else {
      // Ascending order: once a line starts in the future, all later lines do too.
      break;
    }
  }
  return active;
}

/**
 * Resolve the active lyric line index for the whole panel, applying the manual
 * timing offset. Shared by LyricsPanel and FullScreenPlayer, which
 * previously duplicated this block verbatim.
 *
 * `offsetMs` is added to the playback position for SYNCED lyrics, so a positive
 * offset advances the highlight (lyrics appear earlier, ahead of the audio) and
 * a negative offset delays it. Unsynced lyrics have no real timestamps, so they
 * keep their even progress-based estimate and the offset does not apply.
 */
export function resolveActiveLyricIndex(
  lyrics: TrackLyrics | null,
  positionMs: number,
  durationMs: number,
  offsetMs: number
): number {
  if (!lyrics || lyrics.lines.length === 0) return -1;
  if (lyrics.synced) return activeLyricIndex(lyrics.lines, positionMs + offsetMs);
  if (durationMs > 0) {
    const idx = Math.floor((positionMs / durationMs) * lyrics.lines.length);
    return Math.max(0, Math.min(lyrics.lines.length - 1, idx));
  }
  return -1;
}
