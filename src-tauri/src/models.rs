//! Domain models for Bragi.
//!
//! These structs mirror the TypeScript types in `src/lib/types/models.ts`
//! to ensure type-safe IPC between frontend and backend.

use serde::{Deserialize, Serialize};

pub use crate::audio::{QueueEntrySource, RepeatMode};

/// A music track.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub jellyfin_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mbid: Option<String>,
    /// MusicBrainz artist ID (denormalised for scrobble payloads).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist_mbid: Option<String>,
    /// MusicBrainz release ID (denormalised from album).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_mbid: Option<String>,
    pub name: String,
    pub artist_name: String,
    pub album_name: String,
    pub album_jellyfin_id: String,
    pub track_number: u32,
    pub disc_number: u32,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_size_bytes: Option<u64>,
    /// ReplayGain normalization value in dB (from Jellyfin).
    /// Negative values reduce volume, positive values increase it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalization_gain_db: Option<f32>,
}

/// A music album.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub jellyfin_id: String,
    /// MusicBrainz release ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_mbid: Option<String>,
    pub name: String,
    pub artist_name: String,
    pub artist_jellyfin_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork_url: Option<String>,
    pub track_count: u32,
    pub duration_ms: u64,
    /// Number of tracks with cached audio files.
    pub downloaded_track_count: u32,
    /// When the media server first saw this album, as epoch seconds — Jellyfin's
    /// `DateCreated` / OpenSubsonic's `created`. This is what "recently added"
    /// means; `last_synced_at` is a sync bookkeeping stamp and is rewritten
    /// across the whole library on every sync. `None` on rows cached
    /// before the v15 schema, until the next full sync backfills them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_created: Option<i64>,
}

/// A music artist.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artist {
    pub jellyfin_id: String,
    /// MusicBrainz artist ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mbid: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backdrop_url: Option<String>,
    pub album_count: u32,
    /// Jellyfin's own SortName, when the backend reports one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_name: Option<String>,
}

/// Current playback state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackState {
    pub current_track: Option<Track>,
    pub is_playing: bool,
    pub current_time_ms: u64,
    pub duration_ms: u64,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            current_track: None,
            is_playing: false,
            current_time_ms: 0,
            duration_ms: 0,
            volume: 1.0,
            shuffle: false,
            repeat: RepeatMode::Off,
        }
    }
}

/// Current queue state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QueueState {
    pub tracks: Vec<Track>,
    pub current_index: i32,
    /// Parallel to `tracks` — where each entry came from (the Up Next panel's
    /// "Playing Next" vs the rest of the context).
    pub sources: Vec<QueueEntrySource>,
}

/// Library search results.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
}

// ============================================================================
// Jellyfin Playlist Types
// ============================================================================

/// A Jellyfin playlist summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JellyfinPlaylist {
    /// Jellyfin playlist ID.
    pub jellyfin_id: String,
    /// Playlist name.
    pub name: String,
    /// Number of items in the playlist.
    pub item_count: u32,
}

/// A track within a Jellyfin playlist.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JellyfinPlaylistTrack {
    /// Jellyfin track ID.
    pub jellyfin_id: String,
    /// MusicBrainz recording ID — the canonical cross-instance track identity,
    /// used to resolve shared/imported playlists against another library.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mbid: Option<String>,
    /// Playlist item ID (entry ID for removal/reordering).
    pub playlist_item_id: String,
    /// Track name.
    pub name: String,
    /// Artist name.
    pub artist_name: String,
    /// Album name.
    pub album_name: String,
    /// Album Jellyfin ID (for artwork).
    pub album_jellyfin_id: String,
    /// Track duration in milliseconds.
    pub duration_ms: u64,
    /// Artwork URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork_url: Option<String>,
}

/// A Jellyfin playlist with its tracks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JellyfinPlaylistDetail {
    /// Jellyfin playlist ID.
    pub jellyfin_id: String,
    /// Tracks in the playlist.
    ///
    /// The playlist name is intentionally omitted: callers already have it
    /// from the `JellyfinPlaylist` summary, so fetching it here would require
    /// a redundant metadata request.
    pub tracks: Vec<JellyfinPlaylistTrack>,
}

// ============================================================================
// Shared/Imported Playlist Types
// ============================================================================

/// One track's worth of the metadata carried by a playlist share code.
///
/// Deliberately backend-agnostic and self-contained: no Jellyfin ID, since the
/// importing instance has its own, unrelated library. `mbid` is the primary
/// resolution key; the rest are human-readable/fuzzy-match fallback.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPlaylistTrackInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mbid: Option<String>,
    pub title: String,
    pub artist_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Summary of a locally-stored imported playlist (sidebar listing).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedPlaylist {
    pub id: String,
    pub name: String,
    pub imported_at: i64,
}

/// One track of an imported playlist, resolved against THIS instance's own
/// library. `resolved_jellyfin_id` is `None` when the track isn't (yet) in
/// the local library — the frontend renders it as unavailable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedPlaylistTrack {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mbid: Option<String>,
    pub title: String,
    pub artist_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_jellyfin_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_file_path: Option<String>,
}

/// An imported playlist with its (live-resolved) tracks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedPlaylistDetail {
    pub id: String,
    pub name: String,
    pub imported_at: i64,
    pub tracks: Vec<ImportedPlaylistTrack>,
}

// ============================================================================
// Lyrics Types
// ============================================================================

/// A single line of track lyrics (IPC-facing).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackLyricsLine {
    /// Start time of the line in milliseconds.
    ///
    /// `None` for unsynced (plain-text) lyrics. `Some(0)` is a legitimate
    /// zero-timestamp first line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_ms: Option<u64>,
    /// The lyric text for this line.
    pub text: String,
}

/// Lyrics for a track (IPC-facing).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackLyrics {
    /// Whether the lyrics carry per-line timestamps for synced highlighting.
    pub synced: bool,
    /// The lyric lines in order.
    pub lines: Vec<TrackLyricsLine>,
}
