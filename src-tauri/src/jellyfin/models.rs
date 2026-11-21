//! Jellyfin API response models.
//!
//! These types represent the raw JSON responses from the Jellyfin API.
//! They are converted to our domain models (`crate::models`) before
//! being returned to the frontend.

use serde::Deserialize;

/// Jellyfin paginated items response.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ItemsResponse {
    pub items: Vec<JellyfinItem>,
}

/// A single item from the Jellyfin library.
///
/// This represents albums, artists, tracks, etc. The `item_type` field
/// discriminates between them.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinItem {
    pub id: String,
    pub name: String,
    /// Jellyfin's own sort form of `name` (e.g. "Beatles" for "The Beatles"),
    /// when the server computed one. Feeds the artist sort order.
    pub sort_name: Option<String>,
    #[serde(rename = "Type")]
    pub item_type: String,
    /// For tracks: the album artist
    pub album_artist: Option<String>,
    /// For tracks: the album name
    pub album: Option<String>,
    /// For tracks: the album ID
    pub album_id: Option<String>,
    /// Track number within disc
    pub index_number: Option<u32>,
    /// Disc number
    pub parent_index_number: Option<u32>,
    /// Duration in ticks (10,000 ticks = 1ms)
    pub run_time_ticks: Option<i64>,
    /// Year of release
    pub production_year: Option<u32>,
    /// Genre tags
    pub genres: Option<Vec<String>>,
    /// MusicBrainz and other provider IDs
    pub provider_ids: Option<ProviderIds>,
    /// Artist items (for albums)
    pub artist_items: Option<Vec<ArtistRef>>,
    /// Album artists (for tracks)
    pub album_artists: Option<Vec<ArtistRef>>,
    /// Number of child items (e.g., tracks in an album)
    pub child_count: Option<u32>,
    /// Has primary image
    pub image_tags: Option<ImageTags>,
    /// Backdrop image tags (for artists/albums with background images)
    pub backdrop_image_tags: Option<Vec<String>>,
    /// Media sources (for audio tracks - contains file size info)
    pub media_sources: Option<Vec<MediaSource>>,
    /// ReplayGain normalization value in dB.
    pub normalization_gain: Option<f32>,
    /// Per-user play state (included by default on `/Users/{id}/Items` responses).
    pub user_data: Option<UserData>,
    /// ISO-8601 timestamp of when the item was added to the server's library.
    /// Requested via the `Fields` query param in `library.rs`; without this field
    /// serde silently discarded it, which is why "recently added" had to fall
    /// back to the meaningless `last_synced_at`.
    pub date_created: Option<String>,
}

/// Jellyfin per-user play state for an item.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct UserData {
    /// Number of times the user has played this item.
    #[serde(default)]
    pub play_count: u32,
}

/// MusicBrainz and other external IDs.
///
/// Jellyfin exposes both `MusicBrainzRecording` (the stable recording ID,
/// UFID frame in ID3v2, `MUSICBRAINZ_TRACKID` in Vorbis) and `MusicBrainzTrack`
/// (the release-track ID, which differs per pressing). Bragi must use the
/// recording ID for scrobbling and stats lookup because that's what
/// ListenBrainz and the rest of the ecosystem expect.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct ProviderIds {
    /// The stable MusicBrainz Recording ID. This is the canonical identifier
    /// used across the ecosystem and what Bragi should submit for stats and
    /// scrobbles.
    #[serde(rename = "MusicBrainzRecording")]
    pub musicbrainz_recording: Option<String>,
    #[serde(rename = "MusicBrainzAlbum")]
    pub musicbrainz_album: Option<String>,
    #[serde(rename = "MusicBrainzArtist")]
    pub musicbrainz_artist: Option<String>,
}

/// Artist reference in album/track items.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ArtistRef {
    pub id: String,
    pub name: String,
}

/// Image tags indicating which images are available.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ImageTags {
    pub primary: Option<String>,
}

/// Media source information (file path, size, codec, etc.)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MediaSource {
    /// File size in bytes
    pub size: Option<u64>,
}

// ============================================================================
// Playlist Types
// ============================================================================

/// Response from Jellyfin playlist items endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PlaylistItemsResponse {
    pub items: Vec<PlaylistItem>,
}

/// A track item within a Jellyfin playlist.
///
/// This extends the standard item with playlist-specific fields.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PlaylistItem {
    pub id: String,
    pub name: String,
    /// The playlist item ID (entry ID) for removal/reordering.
    pub playlist_item_id: String,
    /// For tracks: the album artist
    pub album_artist: Option<String>,
    /// For tracks: the album name
    pub album: Option<String>,
    /// For tracks: the album ID
    pub album_id: Option<String>,
    /// Duration in ticks (10,000 ticks = 1ms)
    pub run_time_ticks: Option<i64>,
    /// MusicBrainz and other provider IDs
    pub provider_ids: Option<ProviderIds>,
    /// Artist items (for albums)
    pub artist_items: Option<Vec<ArtistRef>>,
    /// Album artists (for tracks)
    pub album_artists: Option<Vec<ArtistRef>>,
}

impl PlaylistItem {
    /// Convert Jellyfin ticks (100 nanoseconds) to milliseconds.
    pub fn duration_ms(&self) -> u64 {
        self.run_time_ticks
            .filter(|&ticks| ticks > 0)
            .map(|ticks| (ticks / 10_000) as u64)
            .unwrap_or(0)
    }

    /// Get the primary artist name (for tracks).
    pub fn primary_artist_name(&self) -> Option<String> {
        self.album_artist.clone().or_else(|| {
            self.artist_items
                .as_ref()
                .and_then(|artists| artists.first().map(|a| a.name.clone()))
                .or_else(|| {
                    self.album_artists
                        .as_ref()
                        .and_then(|artists| artists.first().map(|a| a.name.clone()))
                })
        })
    }
}

/// Request body for creating a new playlist in Jellyfin.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreatePlaylistRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<String>>,
    pub user_id: String,
    pub media_type: String,
}

/// Response from creating a playlist.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreatePlaylistResponse {
    pub id: String,
}

/// Request body for `POST /Playlists/{playlistId}` (`UpdatePlaylistDto`).
///
/// Only `Name` is ever sent — the other fields the DTO accepts (`Ids`,
/// `IsPublic`, `Users`) are omitted so a rename can never touch membership or
/// sharing as a side effect.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct UpdatePlaylistRequest {
    pub name: String,
}

/// Response from `GET /Audio/{itemId}/Lyrics`.
///
/// Jellyfin's built-in lyrics endpoint (available since server 10.9).
/// All fields are optional/nullable in the Jellyfin OpenAPI schema.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LyricDto {
    /// Metadata about the lyric track (sync flag, offset, etc.).
    pub metadata: Option<LyricMetadata>,
    /// The individual lyric lines.
    pub lyrics: Option<Vec<LyricLine>>,
}

/// A single line of lyrics from Jellyfin.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LyricLine {
    /// The lyric text for this line.
    pub text: Option<String>,
    /// Start time of this line in ticks (100 nanoseconds).
    ///
    /// Absent (`None`) for unsynced lyrics; `Some(0)` is a legitimate
    /// zero-timestamp first line and must not be treated as "missing".
    pub start: Option<i64>,
}

/// Metadata about a lyric track from Jellyfin.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LyricMetadata {
    /// Whether the lyrics carry per-line timestamps.
    pub is_synced: Option<bool>,
}

impl JellyfinItem {
    /// Convert Jellyfin ticks (100 nanoseconds) to milliseconds.
    ///
    /// Returns 0 for missing or negative values (malformed data protection).
    pub fn duration_ms(&self) -> u64 {
        self.run_time_ticks
            .filter(|&ticks| ticks > 0)
            .map(|ticks| (ticks / 10_000) as u64)
            .unwrap_or(0)
    }

    /// The item's whole genre tag, as the server reported it.
    ///
    /// Every element of `Genres` is kept, joined back into one string. Taking
    /// only `.first()` was lossy in both server configurations: with
    /// custom tag delimiters off, Jellyfin returns one element holding the
    /// whole `"Alternative Metal;Heavy Metal;Rock"` tag; with them on, it
    /// returns several elements and only the first survived.
    ///
    /// This is the raw audit value stored in `cached_tracks.genre`. The
    /// normalised memberships every listing and filter reads are derived from
    /// it by [`crate::genres::split_genres`] and live in `track_genres`.
    pub fn genre_tag(&self) -> Option<String> {
        let genres = self.genres.as_ref()?;
        let joined = genres
            .iter()
            .map(|g| g.trim())
            .filter(|g| !g.is_empty())
            .collect::<Vec<_>>()
            .join(";");
        if joined.is_empty() {
            None
        } else {
            Some(joined)
        }
    }

    /// Get the primary artist ID (for albums/tracks).
    pub fn primary_artist_id(&self) -> Option<String> {
        self.artist_items
            .as_ref()
            .and_then(|artists| artists.first().map(|a| a.id.clone()))
            .or_else(|| {
                self.album_artists
                    .as_ref()
                    .and_then(|artists| artists.first().map(|a| a.id.clone()))
            })
    }

    /// Get the primary artist name (for albums/tracks).
    pub fn primary_artist_name(&self) -> Option<String> {
        self.album_artist.clone().or_else(|| {
            self.artist_items
                .as_ref()
                .and_then(|artists| artists.first().map(|a| a.name.clone()))
                .or_else(|| {
                    self.album_artists
                        .as_ref()
                        .and_then(|artists| artists.first().map(|a| a.name.clone()))
                })
        })
    }

    /// The Primary image tag; Jellyfin changes it whenever the image changes.
    pub fn primary_image_tag(&self) -> Option<&str> {
        self.image_tags.as_ref()?.primary.as_deref()
    }

    /// Check if this item has at least one backdrop image.
    pub fn has_backdrop_image(&self) -> bool {
        self.backdrop_image_tags
            .as_ref()
            .is_some_and(|tags| !tags.is_empty())
    }

    /// Get file size in bytes from the first media source.
    pub fn file_size_bytes(&self) -> Option<u64> {
        self.media_sources
            .as_ref()
            .and_then(|sources| sources.first())
            .and_then(|source| source.size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test for the MBID source-of-truth bug.
    ///
    /// Jellyfin exposes both `MusicBrainzRecording` (the stable recording ID,
    /// what the ecosystem uses) and `MusicBrainzTrack` (the release-track ID,
    /// which changes per pressing). Bragi must read `MusicBrainzRecording`.
    /// If anyone swaps this back to `MusicBrainzTrack`, every scrobble and
    /// bulk stats lookup becomes incompatible with ListenBrainz and the scrobble server.
    #[test]
    fn provider_ids_reads_recording_not_track() {
        let json = r#"{
            "MusicBrainzAlbum": "44284255-03c2-4cbf-bd32-3fe38b9babc9",
            "MusicBrainzAlbumArtist": "5b687684-ad34-4a9f-b425-0e7aa81fbd38",
            "MusicBrainzArtist": "5b687684-ad34-4a9f-b425-0e7aa81fbd38",
            "MusicBrainzRecording": "c07c5d52-386b-4f8d-81a6-137b836fd1f9",
            "MusicBrainzReleaseGroup": "fa6b9363-3fe4-490c-a6d4-c148d19af25c",
            "MusicBrainzTrack": "d32eceab-2a84-4458-a3bb-7dfff55a02a9"
        }"#;

        let ids: ProviderIds = serde_json::from_str(json).expect("parse");

        assert_eq!(
            ids.musicbrainz_recording.as_deref(),
            Some("c07c5d52-386b-4f8d-81a6-137b836fd1f9"),
            "recording MBID must be read from MusicBrainzRecording, not MusicBrainzTrack"
        );
        assert_eq!(
            ids.musicbrainz_artist.as_deref(),
            Some("5b687684-ad34-4a9f-b425-0e7aa81fbd38")
        );
        assert_eq!(
            ids.musicbrainz_album.as_deref(),
            Some("44284255-03c2-4cbf-bd32-3fe38b9babc9")
        );
    }

    #[test]
    fn provider_ids_missing_recording_id_is_none() {
        // Track with only a release-track ID (poorly tagged, Picard didn't
        // resolve the recording). We must NOT fall back to the track ID;
        // NULL is the correct representation.
        let json = r#"{
            "MusicBrainzTrack": "d32eceab-2a84-4458-a3bb-7dfff55a02a9"
        }"#;

        let ids: ProviderIds = serde_json::from_str(json).expect("parse");
        assert!(ids.musicbrainz_recording.is_none());
    }
}
