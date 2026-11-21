import type { JellyfinPlaylistTrack, Track } from '#lib/types/models.js';

/**
 * Convert a JellyfinPlaylistTrack to a Track for playback.
 *
 * Returns `null` for an entry with no usable Jellyfin id — mirrors
 * `importedPlaylistTrackToTrack`'s guard: there is nothing to stream,
 * download or scrobble without one, so it must never be handed to playback
 * as if it were a real track.
 */
export function jellyfinPlaylistTrackToTrack(pt: JellyfinPlaylistTrack): Track | null {
  if (!pt.jellyfinId) return null;
  return {
    jellyfinId: pt.jellyfinId,
    name: pt.name,
    artistName: pt.artistName,
    albumName: pt.albumName,
    albumJellyfinId: pt.albumJellyfinId,
    trackNumber: 1,
    discNumber: 1,
    durationMs: pt.durationMs,
    artworkUrl: pt.artworkUrl,
    mbid: pt.mbid,
    genre: undefined,
    year: undefined,
    cachedFilePath: undefined,
    fileSizeBytes: undefined,
    normalizationGainDb: undefined,
  };
}
