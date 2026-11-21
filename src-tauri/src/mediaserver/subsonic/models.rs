//! OpenSubsonic wire models + wire→domain mappers.
//!
//! Subsonic responses are wrapped in a `{"subsonic-response": {...}}` envelope
//! (handled in `client.rs`); these structs model the inner data objects. The
//! mappers convert them to Bragi's server-agnostic domain models
//! (`crate::models::{Track, Album, Artist}`), exactly as the Jellyfin
//! `item_to_*` mappers do — so nothing downstream (DB, audio, frontend) needs
//! to know which backend produced a record.
//!
//! Key conversions: Subsonic `duration` is **seconds** (Jellyfin uses 100ns
//! ticks) → `duration_ms = duration * 1000`; `size` is bytes → `file_size_bytes`;
//! song `musicBrainzId` is the recording MBID; `coverArt` id becomes a
//! credential-free artwork reference supplied by the caller (a `subsonic-cover:`
//! sentinel resolved to a `/cover` proxy URL at the read boundary).

use serde::Deserialize;

use crate::models::{
    Album, Artist, JellyfinPlaylist, JellyfinPlaylistDetail, JellyfinPlaylistTrack, Track,
};

/// A song (track) as returned by `getAlbum`/`search3`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubsonicSong {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub album_id: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub track: Option<u32>,
    #[serde(default)]
    pub disc_number: Option<u32>,
    /// Duration in **seconds**.
    #[serde(default)]
    pub duration: Option<u64>,
    #[serde(default)]
    pub year: Option<u32>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub cover_art: Option<String>,
    /// File size in bytes.
    #[serde(default)]
    pub size: Option<u64>,
    /// MusicBrainz **recording** id (OpenSubsonic extension).
    #[serde(default)]
    pub music_brainz_id: Option<String>,
    /// Server-side play count for the authenticated user (OpenSubsonic
    /// extension). Absent on servers that don't report it → `None`. Read-only:
    /// Bragi never writes play state back to the Subsonic server.
    #[serde(default)]
    pub play_count: Option<u32>,
}

/// An album as returned by `getAlbumList2`/`getAlbum`/`search3`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubsonicAlbum {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub artist_id: Option<String>,
    #[serde(default)]
    pub year: Option<u32>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub song_count: Option<u32>,
    /// Total duration in **seconds**.
    #[serde(default)]
    pub duration: Option<u64>,
    #[serde(default)]
    pub music_brainz_id: Option<String>,
    /// ISO-8601 timestamp of when the album was added to the server's library.
    /// Drives "recently added" ordering; absent on servers that don't
    /// report it → `None`.
    #[serde(default)]
    pub created: Option<String>,
    /// Present on `getAlbum` (the album's tracks).
    #[serde(default)]
    pub song: Vec<SubsonicSong>,
}

/// An artist as returned by `getArtists`/`getArtist`/`search3`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubsonicArtist {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub album_count: Option<u32>,
    #[serde(default)]
    pub music_brainz_id: Option<String>,
    /// Present on `getArtist` (the artist's albums).
    #[serde(default)]
    pub album: Vec<SubsonicAlbum>,
}

/// One alphabetical index bucket in `getArtists`.
#[derive(Debug, Clone, Deserialize)]
pub struct SubsonicIndex {
    #[serde(default)]
    pub artist: Vec<SubsonicArtist>,
}

// ── Response payloads (the object under the named key in the envelope) ───────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumList2 {
    #[serde(default)]
    pub album: Vec<SubsonicAlbum>,
}

#[derive(Debug, Deserialize)]
pub struct ArtistsIndex {
    #[serde(default)]
    pub index: Vec<SubsonicIndex>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult3 {
    #[serde(default)]
    pub artist: Vec<SubsonicArtist>,
    #[serde(default)]
    pub album: Vec<SubsonicAlbum>,
    #[serde(default)]
    pub song: Vec<SubsonicSong>,
}

// ── Playlists (`getPlaylists` / `getPlaylist` / `createPlaylist`) ────────────

/// A playlist summary as returned by `getPlaylists` (no entries).
///
/// OpenSubsonic playlists have no per-entry id — an entry's identity is its
/// 0-based position in the ordered list (see [`playlist_detail`]). `id` is the
/// only required field; everything else is optional so a sparse server response
/// still deserialises cleanly.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubsonicPlaylist {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub song_count: Option<u32>,
    /// ISO-8601 timestamp of the last change (used to disambiguate the
    /// create fallback's match-by-name — newest wins).
    #[serde(default)]
    pub changed: Option<String>,
}

/// A playlist with its ordered entries, as returned by `getPlaylist` (and by
/// `createPlaylist` on servers that echo the created playlist).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubsonicPlaylistWithSongs {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub song_count: Option<u32>,
    /// The ordered songs. Position in this vec IS the entry identity.
    #[serde(default)]
    pub entry: Vec<SubsonicSong>,
}

/// The `playlists` wrapper object under the `getPlaylists` response key.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlists {
    #[serde(default)]
    pub playlist: Vec<SubsonicPlaylist>,
}

// ── Lyrics (OpenSubsonic `getLyricsBySongId` / songLyrics extension) ─────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsList {
    #[serde(default)]
    pub structured_lyrics: Vec<StructuredLyrics>,
}

#[derive(Debug, Deserialize)]
pub struct StructuredLyrics {
    #[serde(default)]
    pub synced: bool,
    #[serde(default)]
    pub line: Vec<LyricLine>,
}

#[derive(Debug, Deserialize)]
pub struct LyricLine {
    /// Start offset in **milliseconds** (present only for synced lyrics).
    #[serde(default)]
    pub start: Option<u64>,
    #[serde(default)]
    pub value: String,
}

// ── Mappers: wire → domain ───────────────────────────────────────────────────

/// Seconds → milliseconds, saturating.
fn secs_to_ms(secs: Option<u64>) -> u64 {
    secs.unwrap_or(0).saturating_mul(1000)
}

/// Treat an empty string as absent. Navidrome returns `""` (not `null`) for a
/// missing `musicBrainzId`, which must map to `None` — a `Some("")` MBID would
/// pollute scrobble payloads and cache keys.
fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.is_empty())
}

/// Map a Subsonic song to the domain [`Track`]. `artwork_url` is supplied by the
/// caller (a proxied cover URL) since Subsonic artwork requires auth.
pub fn song_to_track(song: &SubsonicSong, artwork_url: Option<String>) -> Track {
    Track {
        jellyfin_id: song.id.clone(),
        mbid: non_empty(song.music_brainz_id.clone()),
        artist_mbid: None,
        release_mbid: None,
        name: song.title.clone(),
        artist_name: song.artist.clone().unwrap_or_default(),
        album_name: song.album.clone().unwrap_or_default(),
        album_jellyfin_id: song.album_id.clone().unwrap_or_default(),
        track_number: song.track.unwrap_or(0),
        disc_number: song.disc_number.unwrap_or(1),
        duration_ms: secs_to_ms(song.duration),
        genre: song.genre.clone(),
        year: song.year,
        artwork_url,
        cached_file_path: None,
        file_size_bytes: song.size,
        normalization_gain_db: None,
    }
}

/// Map a Subsonic album to the domain [`Album`].
pub fn album_to_album(album: &SubsonicAlbum, artwork_url: Option<String>) -> Album {
    Album {
        jellyfin_id: album.id.clone(),
        release_mbid: non_empty(album.music_brainz_id.clone()),
        name: album.name.clone(),
        artist_name: album.artist.clone().unwrap_or_default(),
        artist_jellyfin_id: album.artist_id.clone().unwrap_or_default(),
        year: album.year,
        genre: album.genre.clone(),
        artwork_url,
        track_count: album.song_count.unwrap_or(0),
        duration_ms: secs_to_ms(album.duration),
        downloaded_track_count: 0,
        date_created: album
            .created
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.timestamp()),
    }
}

/// Map a Subsonic artist to the domain [`Artist`].
pub fn artist_to_artist(artist: &SubsonicArtist, artwork_url: Option<String>) -> Artist {
    Artist {
        jellyfin_id: artist.id.clone(),
        mbid: non_empty(artist.music_brainz_id.clone()),
        name: artist.name.clone(),
        artwork_url,
        backdrop_url: None,
        album_count: artist.album_count.unwrap_or(0),
        // OpenSubsonic has no SortName equivalent; the ordering model falls
        // back to stripping the article itself.
        sort_name: None,
    }
}

/// Map a Subsonic playlist summary to the domain [`JellyfinPlaylist`].
pub fn playlist_to_summary(p: &SubsonicPlaylist) -> JellyfinPlaylist {
    JellyfinPlaylist {
        jellyfin_id: p.id.clone(),
        name: p.name.clone(),
        item_count: p.song_count.unwrap_or(0),
    }
}

/// Map one ordered playlist entry to a domain [`JellyfinPlaylistTrack`].
///
/// Subsonic has no per-entry id, so the 0-based `index` becomes the synthetic
/// `playlist_item_id` — the value the frontend passes back to remove or reorder
/// the entry. `artwork_url` is supplied by the caller (a credential-free cover
/// reference) exactly as [`song_to_track`] does.
pub fn playlist_song_to_track(
    song: &SubsonicSong,
    index: usize,
    artwork_url: Option<String>,
) -> JellyfinPlaylistTrack {
    JellyfinPlaylistTrack {
        jellyfin_id: song.id.clone(),
        mbid: non_empty(song.music_brainz_id.clone()),
        playlist_item_id: index.to_string(),
        name: song.title.clone(),
        artist_name: song.artist.clone().unwrap_or_default(),
        album_name: song.album.clone().unwrap_or_default(),
        album_jellyfin_id: song.album_id.clone().unwrap_or_default(),
        duration_ms: secs_to_ms(song.duration),
        artwork_url,
    }
}

/// Map a full `getPlaylist` payload to a domain [`JellyfinPlaylistDetail`].
///
/// Entries are enumerated so each track's `playlist_item_id` is its 0-based
/// position. `cover_for` resolves each entry's `coverArt` id to a credential-free
/// artwork reference (the caller passes the client's `cover_art_url`).
pub fn playlist_detail(
    playlist_id: &str,
    pl: &SubsonicPlaylistWithSongs,
    mut cover_for: impl FnMut(Option<&str>) -> Option<String>,
) -> JellyfinPlaylistDetail {
    let tracks = pl
        .entry
        .iter()
        .enumerate()
        .map(|(i, s)| playlist_song_to_track(s, i, cover_for(s.cover_art.as_deref())))
        .collect();
    JellyfinPlaylistDetail {
        jellyfin_id: playlist_id.to_string(),
        tracks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn song_maps_seconds_to_ms_and_ids() {
        let song: SubsonicSong = serde_json::from_str(
            r#"{"id":"tr-1","title":"Bleed Out","album":"Bleed Out","albumId":"al-9",
                "artist":"Within Temptation","track":3,"discNumber":1,"duration":215,
                "year":2023,"genre":"Metal","coverArt":"al-9","size":8384021,
                "musicBrainzId":"rec-abc"}"#,
        )
        .unwrap();
        let t = song_to_track(&song, Some("art://x".to_string()));
        assert_eq!(t.jellyfin_id, "tr-1");
        assert_eq!(t.duration_ms, 215_000);
        assert_eq!(t.album_jellyfin_id, "al-9");
        assert_eq!(t.track_number, 3);
        assert_eq!(t.mbid.as_deref(), Some("rec-abc"));
        assert_eq!(t.file_size_bytes, Some(8_384_021));
        assert_eq!(t.artwork_url.as_deref(), Some("art://x"));
    }

    #[test]
    fn album_maps_counts_and_duration() {
        let album: SubsonicAlbum = serde_json::from_str(
            r#"{"id":"al-9","name":"Bleed Out","artist":"Within Temptation","artistId":"ar-2",
                "year":2023,"songCount":12,"duration":2760,"musicBrainzId":"rel-xyz"}"#,
        )
        .unwrap();
        let a = album_to_album(&album, None);
        assert_eq!(a.jellyfin_id, "al-9");
        assert_eq!(a.artist_jellyfin_id, "ar-2");
        assert_eq!(a.track_count, 12);
        assert_eq!(a.duration_ms, 2_760_000);
        assert_eq!(a.release_mbid.as_deref(), Some("rel-xyz"));
    }

    #[test]
    fn empty_mbid_maps_to_none() {
        // Navidrome returns musicBrainzId:"" — must become None, not Some("").
        let song: SubsonicSong =
            serde_json::from_str(r#"{"id":"x","title":"Y","musicBrainzId":""}"#).unwrap();
        assert_eq!(song_to_track(&song, None).mbid, None);
        let album: SubsonicAlbum =
            serde_json::from_str(r#"{"id":"a","name":"N","musicBrainzId":""}"#).unwrap();
        assert_eq!(album_to_album(&album, None).release_mbid, None);
    }

    #[test]
    fn cover_sentinel_round_trips_through_album_mapper() {
        // The mappers are transparent to the artwork value: a credential-free
        // `subsonic-cover:{id}` sentinel supplied by the caller must survive
        // unchanged into the domain model.
        let album: SubsonicAlbum =
            serde_json::from_str(r#"{"id":"al-9","name":"Bleed Out","coverArt":"al-9"}"#).unwrap();
        let mapped = album_to_album(&album, Some("subsonic-cover:al-9".to_string()));
        assert_eq!(mapped.artwork_url.as_deref(), Some("subsonic-cover:al-9"));
    }

    #[test]
    fn song_parses_server_play_count() {
        // Navidrome-style JSON: playCount + played inline on the song.
        let song: SubsonicSong =
            serde_json::from_str(r#"{"id":"tr-1","title":"Bleed Out","playCount":7}"#).unwrap();
        assert_eq!(song.play_count, Some(7));
    }

    #[test]
    fn song_play_count_absent_is_none_and_zero_is_some_zero() {
        // Absent → None (server doesn't report play counts).
        let absent: SubsonicSong = serde_json::from_str(r#"{"id":"x","title":"Y"}"#).unwrap();
        assert_eq!(absent.play_count, None);
        // playCount:0 → Some(0) (a never-played track, distinct from absent).
        let zero: SubsonicSong =
            serde_json::from_str(r#"{"id":"x","title":"Y","playCount":0}"#).unwrap();
        assert_eq!(zero.play_count, Some(0));
    }

    #[test]
    fn missing_optionals_default_cleanly() {
        // A sparse song (only id + title) must not panic and gets sane defaults.
        let song: SubsonicSong = serde_json::from_str(r#"{"id":"x","title":"Y"}"#).unwrap();
        let t = song_to_track(&song, None);
        assert_eq!(t.duration_ms, 0);
        assert_eq!(t.disc_number, 1);
        assert_eq!(t.track_number, 0);
        assert!(t.artist_name.is_empty());
    }

    #[test]
    fn playlist_detail_indexes_entries_and_maps_durations() {
        // A getPlaylist payload: ordered entries with per-song coverArt. Each
        // entry's playlist_item_id must be its 0-based position and durations
        // convert seconds→ms via song_to_track's mapping.
        let pl: SubsonicPlaylistWithSongs = serde_json::from_str(
            r#"{"id":"pl-1","name":"Roadtrip","songCount":3,"duration":600,
                "entry":[
                  {"id":"tr-a","title":"A","duration":100,"coverArt":"al-a"},
                  {"id":"tr-b","title":"B","duration":200,"artist":"X"},
                  {"id":"tr-c","title":"C","duration":300}
                ]}"#,
        )
        .unwrap();
        let detail = playlist_detail("pl-1", &pl, |c| c.map(|id| format!("cover:{id}")));
        assert_eq!(detail.jellyfin_id, "pl-1");
        assert_eq!(detail.tracks.len(), 3);
        assert_eq!(detail.tracks[0].playlist_item_id, "0");
        assert_eq!(detail.tracks[1].playlist_item_id, "1");
        assert_eq!(detail.tracks[2].playlist_item_id, "2");
        assert_eq!(detail.tracks[0].jellyfin_id, "tr-a");
        assert_eq!(detail.tracks[0].duration_ms, 100_000);
        assert_eq!(detail.tracks[1].duration_ms, 200_000);
        assert_eq!(detail.tracks[1].artist_name, "X");
        // Artwork resolves through the caller-supplied closure.
        assert_eq!(detail.tracks[0].artwork_url.as_deref(), Some("cover:al-a"));
        // No coverArt → no artwork.
        assert_eq!(detail.tracks[2].artwork_url, None);
    }

    #[test]
    fn playlists_list_maps_song_count_and_defaults_sparse() {
        // getPlaylists list → summaries; item_count comes from songCount and a
        // sparse playlist (no coverArt/duration/songCount) defaults cleanly.
        let list: Playlists = serde_json::from_str(
            r#"{"playlist":[
                {"id":"pl-1","name":"Full","songCount":12,"duration":2400,"coverArt":"pl-1","changed":"2026-07-01T00:00:00Z"},
                {"id":"pl-2","name":"Sparse"}
            ]}"#,
        )
        .unwrap();
        let summaries: Vec<_> = list.playlist.iter().map(playlist_to_summary).collect();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].jellyfin_id, "pl-1");
        assert_eq!(summaries[0].item_count, 12);
        assert_eq!(summaries[1].jellyfin_id, "pl-2");
        assert_eq!(
            summaries[1].item_count, 0,
            "missing songCount defaults to 0"
        );
        assert_eq!(summaries[1].name, "Sparse");
    }

    #[test]
    fn empty_playlists_response_deserialises() {
        // A server with no playlists returns an object with no `playlist` key.
        let list: Playlists = serde_json::from_str("{}").unwrap();
        assert!(list.playlist.is_empty());
    }
}
