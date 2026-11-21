import type { ImportedPlaylistTrack, Track } from '#lib/types/models.js';

/**
 * Convert a resolved ImportedPlaylistTrack to a Track for playback.
 *
 * Returns `null` for a track that hasn't matched anything in the local
 * library yet (`resolvedJellyfinId` absent) — there is nothing to play.
 */
export function importedPlaylistTrackToTrack(t: ImportedPlaylistTrack): Track | null {
  if (!t.resolvedJellyfinId) return null;
  return {
    jellyfinId: t.resolvedJellyfinId,
    mbid: t.mbid,
    name: t.title,
    artistName: t.artistName,
    albumName: t.albumName ?? '',
    // Not carried by the imported-playlist join — artwork is looked up by
    // track id there instead, so no album id is needed for playback.
    albumJellyfinId: '',
    trackNumber: 1,
    discNumber: 1,
    durationMs: t.durationMs ?? 0,
    artworkUrl: t.artworkUrl,
    cachedFilePath: t.cachedFilePath,
    genre: undefined,
    year: undefined,
    fileSizeBytes: undefined,
    normalizationGainDb: undefined,
  };
}
