import type { Track } from '#lib/types/models.js';
import { getAlbumTracks, getArtistAlbums } from '#lib/services/tauri.js';

/**
 * Every track across one or more albums, flattened in call order — the one
 * resolver every "act on this album (or this whole grid selection of
 * albums)" entry point uses: drag onto a sidebar playlist and an album
 * tile's context menu both go through it, never a fetch loop of their own.
 */
export async function getAlbumsTracks(albumIds: string[]): Promise<Track[]> {
  const perAlbum = await Promise.all(albumIds.map((id) => getAlbumTracks(id)));
  return perAlbum.flat();
}

/**
 * Every track across one or more artists' albums, flattened in artist then
 * album order — the one resolver every "act on this artist (or this whole
 * grid selection of artists)" entry point uses: drag onto a sidebar
 * playlist and an artist tile's context menu both go through it, never a
 * sequential-await loop of their own.
 */
export async function getArtistsTracks(artistIds: string[]): Promise<Track[]> {
  const albumLists = await Promise.all(artistIds.map((id) => getArtistAlbums(id)));
  return getAlbumsTracks(albumLists.flat().map((album) => album.jellyfinId));
}
