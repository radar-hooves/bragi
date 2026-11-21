import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { Album, Track } from '#lib/types/models.js';

const getAlbumTracks = vi.fn();
const getArtistAlbums = vi.fn();
vi.mock('#lib/services/tauri.js', () => ({
  getAlbumTracks: (...args: unknown[]) => getAlbumTracks(...args),
  getArtistAlbums: (...args: unknown[]) => getArtistAlbums(...args),
}));

const { getAlbumsTracks, getArtistsTracks } = await import('./collectionTracks');

function album(id: string): Album {
  return { jellyfinId: id } as Album;
}

function track(id: string): Track {
  return { jellyfinId: id, name: id } as Track;
}

describe('getAlbumsTracks', () => {
  beforeEach(() => {
    getAlbumTracks.mockReset();
  });

  it('flattens one album\'s tracks', async () => {
    getAlbumTracks.mockResolvedValue([track('t1'), track('t2')]);
    const tracks = await getAlbumsTracks(['a1']);
    expect(getAlbumTracks).toHaveBeenCalledWith('a1');
    expect(tracks.map((t) => t.jellyfinId)).toEqual(['t1', 't2']);
  });

  it('flattens several albums\' tracks, in call order — a grid multi-selection', async () => {
    getAlbumTracks.mockImplementation((id: string) =>
      Promise.resolve(id === 'a1' ? [track('t1'), track('t2')] : [track('t3')])
    );
    const tracks = await getAlbumsTracks(['a1', 'a2']);
    expect(tracks.map((t) => t.jellyfinId)).toEqual(['t1', 't2', 't3']);
  });

  it('no albums resolves to no tracks', async () => {
    const tracks = await getAlbumsTracks([]);
    expect(tracks).toEqual([]);
    expect(getAlbumTracks).not.toHaveBeenCalled();
  });
});

describe('getArtistsTracks', () => {
  beforeEach(() => {
    getAlbumTracks.mockReset();
    getArtistAlbums.mockReset();
  });

  it('flattens one artist\'s albums\' tracks, in album order', async () => {
    getArtistAlbums.mockResolvedValue([album('a1'), album('a2')]);
    getAlbumTracks.mockImplementation((id: string) =>
      Promise.resolve(id === 'a1' ? [track('t1'), track('t2')] : [track('t3')])
    );

    const tracks = await getArtistsTracks(['artist-1']);

    expect(getArtistAlbums).toHaveBeenCalledWith('artist-1');
    expect(tracks.map((t) => t.jellyfinId)).toEqual(['t1', 't2', 't3']);
  });

  it('flattens several artists\' albums\' tracks, in artist then album order — a grid multi-selection', async () => {
    getArtistAlbums.mockImplementation((id: string) =>
      Promise.resolve(id === 'artist-1' ? [album('a1')] : [album('a2')])
    );
    getAlbumTracks.mockImplementation((id: string) =>
      Promise.resolve(id === 'a1' ? [track('t1')] : [track('t2')])
    );

    const tracks = await getArtistsTracks(['artist-1', 'artist-2']);

    expect(tracks.map((t) => t.jellyfinId)).toEqual(['t1', 't2']);
  });

  it('an artist with no albums resolves to no tracks', async () => {
    getArtistAlbums.mockResolvedValue([]);
    const tracks = await getArtistsTracks(['artist-empty']);
    expect(tracks).toEqual([]);
    expect(getAlbumTracks).not.toHaveBeenCalled();
  });
});
