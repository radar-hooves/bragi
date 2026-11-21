import { queryTracks } from '#lib/services/tauri.js';
import { syncColumnBrowserToSelection } from '#lib/stores/columnBrowser.svelte.js';
import { startPlayback, shuffleAndPlay } from '#lib/utils/playback.js';

export async function playGenre(genre: string): Promise<void> {
  syncColumnBrowserToSelection({ genre });
  const tracks = await queryTracks({ genres: [genre], sort: 'track-number' });
  await startPlayback(tracks, 0);
}

/** Genre radio: shuffle every track in a genre into the queue. */
export async function shuffleGenre(genre: string): Promise<void> {
  syncColumnBrowserToSelection({ genre });
  const tracks = await queryTracks({ genres: [genre], sort: 'track-number' });
  await shuffleAndPlay(tracks);
}

export async function playColumnArtist(artist: string): Promise<void> {
  syncColumnBrowserToSelection({ artist });
  const tracks = await queryTracks({ artists: [artist], sort: 'track-number' });
  await startPlayback(tracks, 0);
}

export async function playColumnAlbum(albumId: string): Promise<void> {
  syncColumnBrowserToSelection({ albumId });
  const tracks = await queryTracks({ albumIds: [albumId], sort: 'track-number' });
  await startPlayback(tracks, 0);
}
