/**
 * Core domain models for Bragi music player.
 * These types represent the fundamental entities in the music library.
 */

export interface Track {
  jellyfinId: string;
  mbid?: string;
  /** MusicBrainz artist ID (denormalised for scrobble payloads). */
  artistMbid?: string;
  /** MusicBrainz release ID (denormalised from album). */
  releaseMbid?: string;
  name: string;
  artistName: string;
  albumName: string;
  albumJellyfinId: string;
  trackNumber: number;
  discNumber: number;
  durationMs: number;
  genre?: string;
  year?: number;
  artworkUrl?: string;
  cachedFilePath?: string;
  fileSizeBytes?: number;
  /** ReplayGain normalization value in dB (from Jellyfin). */
  normalizationGainDb?: number;
}

export interface Album {
  jellyfinId: string;
  /** MusicBrainz release ID. */
  releaseMbid?: string;
  name: string;
  artistName: string;
  artistJellyfinId: string;
  year?: number;
  genre?: string;
  artworkUrl?: string;
  trackCount: number;
  durationMs: number;
  downloadedTrackCount: number;
  /**
   * When the media server first saw this album, epoch seconds. Absent for rows
   * cached before schema v15, until the next full sync backfills them.
   */
  dateCreated?: number;
}

export interface Artist {
  jellyfinId: string;
  /** MusicBrainz artist ID. */
  mbid?: string;
  name: string;
  artworkUrl?: string;
  backdropUrl?: string;
  albumCount: number;
  /** Jellyfin's own SortName, when the backend reports one. */
  sortName?: string;
}

export interface PlaybackState {
  currentTrack: Track | null;
  isPlaying: boolean;
  currentTimeMs: number;
  durationMs: number;
  volume: number;
  shuffle: boolean;
  repeat: 'off' | 'all' | 'one';
}

/** Where a queue entry came from — drives the Up Next panel's "Playing Next" grouping. */
export type QueueEntrySource = 'context' | 'userQueued';

export interface QueueState {
  tracks: Track[];
  currentIndex: number;
  /** Parallel to `tracks` — where each entry came from. */
  sources: QueueEntrySource[];
}

// ============================================================================
// Jellyfin Playlist Types
// ============================================================================

/** A Jellyfin playlist summary. */
export interface JellyfinPlaylist {
  /** Jellyfin playlist ID. */
  jellyfinId: string;
  /** Playlist name. */
  name: string;
  /** Number of items in the playlist. */
  itemCount: number;
}

/** A track within a Jellyfin playlist. */
export interface JellyfinPlaylistTrack {
  /** Jellyfin track ID. */
  jellyfinId: string;
  /** MusicBrainz recording ID — used to build a shareable playlist link. */
  mbid?: string;
  /** Playlist item ID (entry ID for removal/reordering). */
  playlistItemId: string;
  /** Track name. */
  name: string;
  /** Artist name. */
  artistName: string;
  /** Album name. */
  albumName: string;
  /** Album Jellyfin ID (for artwork). */
  albumJellyfinId: string;
  /** Track duration in milliseconds. */
  durationMs: number;
  /** Artwork URL. */
  artworkUrl?: string;
}

/** A Jellyfin playlist with its tracks. */
export interface JellyfinPlaylistDetail {
  /** Jellyfin playlist ID. */
  jellyfinId: string;
  /**
   * Tracks in the playlist.
   *
   * The playlist name is intentionally not part of the detail payload — the
   * backend avoids a redundant metadata request for it. Read the name from the
   * matching `JellyfinPlaylist` summary (keyed by `jellyfinId`) instead.
   */
  tracks: JellyfinPlaylistTrack[];
}

// ============================================================================
// Shared/Imported Playlist Types
// ============================================================================

/**
 * One track's worth of metadata carried by a playlist share code.
 *
 * Deliberately backend-agnostic and self-contained — no Jellyfin ID, since the
 * importing instance has its own, unrelated library. `mbid` is the primary
 * resolution key; the rest are human-readable/fuzzy-match fallback.
 */
export interface ImportPlaylistTrackInput {
  mbid?: string;
  title: string;
  artistName: string;
  albumName?: string;
  durationMs?: number;
}

/** Summary of a locally-stored imported playlist (sidebar listing). */
export interface ImportedPlaylist {
  id: string;
  name: string;
  importedAt: number;
}

/**
 * One track of an imported playlist, resolved against THIS instance's own
 * library. `resolvedJellyfinId` is absent when the track isn't (yet) in the
 * local library — render it as unavailable.
 */
export interface ImportedPlaylistTrack {
  mbid?: string;
  title: string;
  artistName: string;
  albumName?: string;
  durationMs?: number;
  resolvedJellyfinId?: string;
  artworkUrl?: string;
  cachedFilePath?: string;
}

/** An imported playlist with its (live-resolved) tracks. */
export interface ImportedPlaylistDetail {
  id: string;
  name: string;
  importedAt: number;
  tracks: ImportedPlaylistTrack[];
}

// ============================================================================
// Lyrics Types
// ============================================================================

/** A single line of track lyrics. */
export interface LyricsLine {
  /**
   * Start time of the line in milliseconds.
   *
   * Absent for unsynced (plain-text) lyrics. `0` is a legitimate
   * zero-timestamp first line.
   */
  startMs?: number;
  /** The lyric text for this line. */
  text: string;
}

/** Lyrics for a track. */
export interface TrackLyrics {
  /** Whether the lyrics carry per-line timestamps for synced highlighting. */
  synced: boolean;
  /** The lyric lines in order. */
  lines: LyricsLine[];
}
