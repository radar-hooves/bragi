import type { StatusBarData } from '#lib/components/common/StatusBar.svelte';
import type { Album, Artist, Track } from '#lib/types/models.js';

/** Compute footer status bar data for the tracks section. */
export function tracksFooterData(tracks: Track[]): StatusBarData {
  return {
    itemCount: tracks.length,
    itemLabel: 'song',
    durationMs: tracks.reduce((sum, t) => sum + t.durationMs, 0),
    sizeBytes: tracks.reduce((sum, t) => sum + (t.fileSizeBytes ?? 0), 0),
  };
}

/** Compute footer status bar data for the albums section. */
export function albumsFooterData(albums: Album[], allTracks: Track[]): StatusBarData {
  const albumIds = new Set(albums.map((a) => a.jellyfinId));
  const albumTracks = allTracks.filter((t) => albumIds.has(t.albumJellyfinId));
  return {
    itemCount: albums.length,
    itemLabel: 'album',
    durationMs: albums.reduce((sum, a) => sum + a.durationMs, 0),
    sizeBytes: albumTracks.reduce((sum, t) => sum + (t.fileSizeBytes ?? 0), 0),
  };
}

/** Compute footer status bar data for the artists section. */
export function artistsFooterData(
  artists: Artist[],
  allTracks: Track[],
  selectedArtist: Artist | null
): StatusBarData {
  if (selectedArtist) {
    const artistTracks = allTracks.filter((t) => t.artistName === selectedArtist.name);
    return {
      itemCount: artistTracks.length,
      itemLabel: 'song',
      durationMs: artistTracks.reduce((sum, t) => sum + t.durationMs, 0),
      sizeBytes: artistTracks.reduce((sum, t) => sum + (t.fileSizeBytes ?? 0), 0),
    };
  }
  const artistNames = new Set(artists.map((a) => a.name));
  const artistTracks = allTracks.filter((t) => artistNames.has(t.artistName));
  return {
    itemCount: artists.length,
    itemLabel: 'artist',
    durationMs: artistTracks.reduce((sum, t) => sum + t.durationMs, 0),
    sizeBytes: artistTracks.reduce((sum, t) => sum + (t.fileSizeBytes ?? 0), 0),
  };
}
