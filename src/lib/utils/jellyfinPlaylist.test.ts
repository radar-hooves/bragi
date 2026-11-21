// A playlist entry with no usable Jellyfin id must never become a playable
// Track: there is nothing to stream, download or scrobble without one.
import { describe, it, expect } from 'vitest';
import { jellyfinPlaylistTrackToTrack } from './jellyfinPlaylist';
import type { JellyfinPlaylistTrack } from '#lib/types/models.js';

function playlistTrack(overrides: Partial<JellyfinPlaylistTrack> = {}): JellyfinPlaylistTrack {
  return {
    jellyfinId: 'track-1',
    playlistItemId: 'item-1',
    name: 'Dimes',
    artistName: 'Bloodhound Gang',
    albumName: 'Hard-Off',
    albumJellyfinId: 'album-1',
    durationMs: 180_000,
    ...overrides,
  };
}

describe('jellyfinPlaylistTrackToTrack', () => {
  it('converts a normal entry to a playable Track', () => {
    const track = jellyfinPlaylistTrackToTrack(playlistTrack());
    expect(track).not.toBeNull();
    expect(track?.jellyfinId).toBe('track-1');
    expect(track?.name).toBe('Dimes');
  });

  it('returns null for an entry with an empty Jellyfin id', () => {
    expect(jellyfinPlaylistTrackToTrack(playlistTrack({ jellyfinId: '' }))).toBeNull();
  });
});
