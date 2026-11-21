//! Jellyfin library browsing endpoints.
//!
//! Fetches albums, artists, tracks, and search results from Jellyfin.

use crate::error::AppError;
use crate::models::{Album, Artist, SearchResults, Track, TrackLyrics, TrackLyricsLine};

use super::client::JellyfinClient;
use super::models::{
    CreatePlaylistRequest, CreatePlaylistResponse, ItemsResponse, JellyfinItem, LyricDto,
    PlaylistItem, PlaylistItemsResponse, UpdatePlaylistRequest,
};

/// Attempts (initial + retries) for a transient request on the sync path.
const SYNC_RETRY_ATTEMPTS: u32 = 3;

/// Send a GET request builder with bounded exponential backoff on transient
/// failures, for the high-volume library-sync path.
///
/// Retries only on genuinely transient conditions — connect/timeout errors and
/// 5xx/429 responses — so a self-hosted Jellyfin that briefly wobbles under sync
/// load self-heals instead of dropping an album (which would also defer stale
/// cleanup). It never retries a 4xx: a 401 must still flow through
/// `handle_error_status` to fire auth-expiry exactly once, and a 404 is
/// definitive. On the final attempt the response (even a 5xx) is returned for
/// the caller's normal status handling, preserving existing behaviour.
async fn send_with_retry(
    builder: reqwest_middleware::RequestBuilder,
) -> Result<reqwest::Response, AppError> {
    let mut attempt: u32 = 1;
    loop {
        // Clone for a possible retry; these are simple query GETs so cloning
        // always succeeds. If a body ever made it uncloneable, send once.
        let this_try = match builder.try_clone() {
            Some(b) => b,
            None => {
                return builder.send().await.map_err(|e| {
                    // Strip the URL before it reaches either destination: a
                    // Subsonic request signs its auth token into the query
                    // string (review), and this message goes to the
                    // user-facing `AppError` and to telemetry alike.
                    let e = e.without_url();
                    telemetry::report_error_with_cause(
                        "Jellyfin or media-server request failed or timed out",
                        &e,
                    );
                    AppError::Network(format!("Request failed: {e}"))
                });
            }
        };

        match this_try.send().await {
            Ok(response) => {
                let status = response.status();
                let transient =
                    status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS;
                if transient && attempt < SYNC_RETRY_ATTEMPTS {
                    tracing::debug!("Transient {status} on sync request; retry {attempt}");
                    sync_backoff(attempt).await;
                    attempt += 1;
                    continue;
                }
                return Ok(response);
            }
            Err(e) => {
                let transient = e.is_timeout() || e.is_connect();
                if transient && attempt < SYNC_RETRY_ATTEMPTS {
                    tracing::debug!("Transient network error on sync request; retry {attempt}");
                    sync_backoff(attempt).await;
                    attempt += 1;
                    continue;
                }
                let e = e.without_url();
                telemetry::report_error_with_cause(
                    "Jellyfin or media-server request failed or timed out",
                    &e,
                );
                return Err(AppError::Network(format!("Request failed: {e}")));
            }
        }
    }
}

/// Exponential backoff with jitter between transient sync retries: ~250ms then
/// ~500ms (+ up to 100ms jitter) to avoid a thundering herd on the server.
async fn sync_backoff(attempt: u32) {
    let base = 250u64 * (1u64 << (attempt - 1)); // attempt 1 -> 250, 2 -> 500
    let jitter = fastrand::u64(0..100);
    tokio::time::sleep(std::time::Duration::from_millis(base + jitter)).await;
}

/// Strip a trailing year suffix like " (1990)" from a name.
///
/// Jellyfin sometimes embeds the production year in album names.
/// Only strips if the suffix is a 4-digit year in parentheses at the end.
fn strip_year_suffix(name: &str) -> String {
    let trimmed = name.trim_end();
    if trimmed.len() >= 7 && trimmed.ends_with(')') {
        if let Some(open) = trimmed.rfind(" (") {
            let inner = &trimmed[open + 2..trimmed.len() - 1];
            if inner.len() == 4 && inner.bytes().all(|b| b.is_ascii_digit()) {
                return trimmed[..open].to_string();
            }
        }
    }
    name.to_string()
}

/// Parse Jellyfin's `DateCreated` into epoch seconds.
///
/// Jellyfin serialises .NET `DateTime` as ISO-8601 with seven fractional digits
/// (`2023-01-15T10:30:00.0000000Z`), which is valid RFC 3339. Some server
/// versions omit the zone offset, so fall back to parsing it as naive UTC rather
/// than dropping the value — an approximately-right date-added still sorts
/// correctly, whereas `None` sends the album to the bottom of the list.
fn parse_date_created(raw: &str) -> Option<i64> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(dt.timestamp());
    }
    chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f")
        .ok()
        .map(|dt| dt.and_utc().timestamp())
}

/// Convert a raw Jellyfin [`LyricDto`] into the IPC-facing [`TrackLyrics`].
///
/// Returns `None` when there are no usable lyric lines (missing or empty
/// `Lyrics` array), which callers surface as a clean "no lyrics" state.
///
/// # Timestamps
///
/// `Start` is in Jellyfin ticks (100ns). Conversion gates on `Option`
/// *presence*, not `> 0`: a synced line at `[00:00.00]` serializes as
/// `Start = 0` and must keep `start_ms = Some(0)` (absence is `null`, not 0).
///
/// # Sync detection
///
/// A track is treated as synced if `IsSynced == Some(true)` OR any line
/// carries a timestamp (fallback for LRC files that omit `IsSynced`).
///
/// The `Offset` field is intentionally *not* applied: its sign convention is
/// not verified against Jellyfin, and it is rarely present, so applying it
/// with the wrong sign would be worse than ignoring it.
fn lyric_dto_to_track_lyrics(dto: LyricDto) -> Option<TrackLyrics> {
    let raw_lines = dto.lyrics?;
    if raw_lines.is_empty() {
        return None;
    }

    let lines: Vec<TrackLyricsLine> = raw_lines
        .into_iter()
        .map(|line| TrackLyricsLine {
            // Gate on presence, not `t > 0`, so a genuine 0ms first line is kept.
            start_ms: line.start.map(|t| (t.max(0) / 10_000) as u64),
            text: line.text.unwrap_or_default(),
        })
        .collect();

    let is_synced_flag = dto
        .metadata
        .as_ref()
        .and_then(|m| m.is_synced)
        .unwrap_or(false);
    let synced = is_synced_flag || lines.iter().any(|l| l.start_ms.is_some());

    Some(TrackLyrics { synced, lines })
}

async fn parse_json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    what: &str,
) -> Result<T, AppError> {
    response
        .json()
        .await
        .map_err(|e| AppError::Jellyfin(format!("Failed to parse {what}: {}", e.without_url())))
}

impl JellyfinClient {
    /// Check a response's status, firing auth-expiry on a 401.
    fn ok(&self, response: reqwest::Response, what: &str) -> Result<reqwest::Response, AppError> {
        if response.status().is_success() {
            Ok(response)
        } else {
            Err(self.handle_error_status(response.status(), what))
        }
    }

    /// Send a request once and check its status. The URL is stripped from a
    /// transport error: a Subsonic request signs its token into the query.
    async fn send(
        &self,
        request: reqwest_middleware::RequestBuilder,
        what: &str,
    ) -> Result<reqwest::Response, AppError> {
        let response = request
            .send()
            .await
            .map_err(|e| AppError::Network(format!("Failed to {what}: {}", e.without_url())))?;
        self.ok(response, what)
    }

    /// [`Self::send`], parsing the body as JSON.
    async fn send_json<T: serde::de::DeserializeOwned>(
        &self,
        request: reqwest_middleware::RequestBuilder,
        what: &str,
    ) -> Result<T, AppError> {
        parse_json(self.send(request, what).await?, what).await
    }

    /// [`Self::send_json`] through [`send_with_retry`], for sync-path reads.
    async fn send_json_retry<T: serde::de::DeserializeOwned>(
        &self,
        request: reqwest_middleware::RequestBuilder,
        what: &str,
    ) -> Result<T, AppError> {
        parse_json(self.ok(send_with_retry(request).await?, what)?, what).await
    }

    /// Fetch all albums from the library.
    ///
    /// Returns albums sorted by artist then album name.
    pub async fn get_albums(&self) -> Result<Vec<Album>, AppError> {
        let url = self.user_endpoint("/Users/{userId}/Items");

        let items_response: ItemsResponse = self
            .send_json_retry(
                self.http().get(url).query(&[
                    ("IncludeItemTypes", "MusicAlbum"),
                    ("Recursive", "true"),
                    ("SortBy", "AlbumArtist,SortName"),
                    ("SortOrder", "Ascending"),
                    (
                        "Fields",
                        "ProviderIds,Genres,DateCreated,ChildCount,RunTimeTicks,ArtistItems",
                    ),
                ]),
                "fetch albums",
            )
            .await?;

        let albums = items_response
            .items
            .into_iter()
            .filter_map(|item| self.item_to_album(item))
            .collect();

        Ok(albums)
    }

    /// Fetch all tracks for an album.
    ///
    /// Returns tracks sorted by disc number then track number.
    pub async fn get_album_tracks(&self, album_id: &str) -> Result<Vec<Track>, AppError> {
        let url = self.user_endpoint("/Users/{userId}/Items");

        let items_response: ItemsResponse = self
            .send_json_retry(
                self.http().get(url).query(&[
                    ("ParentId", album_id),
                    ("IncludeItemTypes", "Audio"),
                    ("SortBy", "ParentIndexNumber,IndexNumber"),
                    ("SortOrder", "Ascending"),
                    (
                        "Fields",
                        "ProviderIds,Genres,RunTimeTicks,AlbumArtists,MediaSources",
                    ),
                ]),
                "fetch album tracks",
            )
            .await?;

        let tracks = items_response
            .items
            .into_iter()
            .filter_map(|item| self.item_to_track(item))
            .collect();

        Ok(tracks)
    }

    /// Fetch current Jellyfin play counts for the given track IDs, keyed by
    /// Jellyfin id. Batched to keep URLs within limits; `UserData` (with
    /// `PlayCount`) is requested via `EnableUserData`. Read-only — Bragi never
    /// writes play state back to Jellyfin.
    pub async fn get_track_play_counts(
        &self,
        ids: &[String],
    ) -> Result<std::collections::HashMap<String, u32>, AppError> {
        let mut counts = std::collections::HashMap::new();
        // ~200 32-char IDs keeps the query string comfortably within limits.
        for chunk in ids.chunks(200) {
            let ids_param = chunk.join(",");
            let items: ItemsResponse = self
                .send_json(
                    self.http()
                        .get(self.user_endpoint("/Users/{userId}/Items"))
                        .query(&[
                            ("Ids", ids_param.as_str()),
                            ("EnableUserData", "true"),
                            ("Fields", "ProviderIds"),
                        ]),
                    "fetch play counts",
                )
                .await?;

            for item in items.items {
                if let Some(ud) = item.user_data {
                    counts.insert(item.id, ud.play_count);
                }
            }
        }
        Ok(counts)
    }

    /// Fetch current Jellyfin MusicBrainz recording IDs for the given track
    /// IDs, keyed by Jellyfin id.
    ///
    /// Used to re-resolve a scrobble/skip held back for lacking an MBID at
    /// queue time (the scrobble server rejects mbid-less scrobbles/skips)
    /// against LIVE Jellyfin metadata rather than the local cache that
    /// produced the stale value in the first place. `None` for an id present
    /// in the map means Jellyfin still reports no recording ID for that
    /// track (not yet tagged); an id absent from a successful result means
    /// Jellyfin no longer has it. Any failed chunk returns an error, never an
    /// incomplete map that could misidentify a valid item as retired. Read-only;
    /// a 401 still fires the deduplicated auth-expiry notifier.
    pub async fn get_track_mbids(
        &self,
        ids: &[String],
    ) -> Result<std::collections::HashMap<String, Option<String>>, AppError> {
        let mut mbids = std::collections::HashMap::new();
        // Mirrors get_track_play_counts's chunk size: ~200 32-char IDs keeps
        // the query string comfortably within limits.
        for chunk in ids.chunks(200) {
            let ids_param = chunk.join(",");
            let items: ItemsResponse = self
                .send_json(
                    self.http()
                        .get(self.user_endpoint("/Users/{userId}/Items"))
                        .query(&[("Ids", ids_param.as_str()), ("Fields", "ProviderIds")]),
                    "fetch track MBIDs",
                )
                .await?;

            for item in items.items {
                let mbid = item
                    .provider_ids
                    .as_ref()
                    .and_then(|ids| ids.musicbrainz_recording.clone());
                mbids.insert(item.id, mbid);
            }
        }
        Ok(mbids)
    }

    /// Fetch built-in lyrics for a track.
    ///
    /// Uses Jellyfin's `GET /Audio/{itemId}/Lyrics` endpoint (server 10.9+).
    /// The endpoint is not user-scoped, so no `userId` is appended.
    ///
    /// A `404` here means the track simply has no lyrics (or the server
    /// predates the endpoint), so it is mapped to `Ok(None)` *before* the
    /// generic error handler — no auth-expiry toast for a missing-lyrics case.
    /// A `401` still flows through [`Self::handle_error_status`] to
    /// [`AppError::AuthExpired`].
    pub async fn get_lyrics(&self, item_id: &str) -> Result<Option<TrackLyrics>, AppError> {
        let url = self.endpoint(&format!("/Audio/{}/Lyrics", item_id));

        // Short per-request timeout: lyrics are best-effort and must not hang
        // (or keep adding load to a busy server) — fail fast and let the
        // frontend retry in the background without touching playback.
        let response = self
            .http()
            .get(url)
            .timeout(std::time::Duration::from_secs(8))
            .send()
            .await
            .map_err(|e| AppError::Network(format!("Failed to fetch lyrics: {}", e)))?;

        // "No lyrics" is a clean, expected outcome — not an error.
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(self.handle_error_status(response.status(), "fetch lyrics"));
        }

        let dto: LyricDto = response
            .json()
            .await
            .map_err(|e| AppError::Jellyfin(format!("Failed to parse lyrics response: {}", e)))?;

        Ok(lyric_dto_to_track_lyrics(dto))
    }

    /// Fetch all artists from the library.
    ///
    /// Returns album artists (not all contributors).
    pub async fn get_artists(&self) -> Result<Vec<Artist>, AppError> {
        let url = self.endpoint("/Artists/AlbumArtists");

        let items_response: ItemsResponse = self
            .send_json_retry(
                self.http().get(url).query(&[
                    ("UserId", self.user_id()),
                    ("SortBy", "SortName"),
                    ("SortOrder", "Ascending"),
                    ("Fields", "ProviderIds,Genres,ChildCount,ImageTags,SortName"),
                ]),
                "fetch artists",
            )
            .await?;

        let artists = items_response
            .items
            .into_iter()
            .filter_map(|item| self.item_to_artist(item))
            .collect();

        Ok(artists)
    }

    /// Fetch albums by a specific artist.
    pub async fn get_artist_albums(&self, artist_id: &str) -> Result<Vec<Album>, AppError> {
        let url = self.user_endpoint("/Users/{userId}/Items");

        let items_response: ItemsResponse = self
            .send_json(
                self.http().get(url).query(&[
                    ("IncludeItemTypes", "MusicAlbum"),
                    ("Recursive", "true"),
                    ("ArtistIds", artist_id),
                    ("SortBy", "ProductionYear,SortName"),
                    ("SortOrder", "Descending"),
                    (
                        "Fields",
                        "ProviderIds,Genres,ChildCount,RunTimeTicks,ArtistItems",
                    ),
                ]),
                "fetch artist albums",
            )
            .await?;

        let albums = items_response
            .items
            .into_iter()
            .filter_map(|item| self.item_to_album(item))
            .collect();

        Ok(albums)
    }

    /// Search the library for tracks, albums, and artists.
    ///
    /// Was `/Search/Hints` (up to 50 lightweight hits) followed by one
    /// `get_track`/`get_album`/`get_artist` round trip PER hit, awaited
    /// sequentially — up to ~51 sequential requests for one search. `/Items`
    /// with `SearchTerm` returns full items inline, the same shape
    /// `get_albums`/`get_album_tracks`/`get_artists` already rely on, so one
    /// request covers every match across all three item types.
    pub async fn search(&self, query: &str) -> Result<SearchResults, AppError> {
        let url = self.user_endpoint("/Users/{userId}/Items");

        let items_response: ItemsResponse = self
            .send_json(
                self.http().get(url).query(&[
                    ("SearchTerm", query),
                    ("IncludeItemTypes", "Audio,MusicAlbum,MusicArtist"),
                    ("Recursive", "true"),
                    ("Limit", "150"),
                    (
                        "Fields",
                        "ProviderIds,Genres,DateCreated,ChildCount,RunTimeTicks,ArtistItems,\
                     AlbumArtists,MediaSources,ImageTags,BackdropImageTags,SortName",
                    ),
                ]),
                "search library",
            )
            .await?;

        Ok(self.items_to_search_results(items_response.items))
    }

    /// Split a mixed `/Items` response into the three domain collections by
    /// each item's `Type`, converting through the same per-type mappers the
    /// bulk library fetches use. Pure and HTTP-free so the dispatch itself is
    /// unit-testable without a mock server (this crate has none).
    fn items_to_search_results(&self, items: Vec<JellyfinItem>) -> SearchResults {
        let mut tracks = Vec::new();
        let mut albums = Vec::new();
        let mut artists = Vec::new();

        for item in items {
            match item.item_type.as_str() {
                "Audio" => {
                    if let Some(track) = self.item_to_track(item) {
                        tracks.push(track);
                    }
                }
                "MusicAlbum" => {
                    if let Some(album) = self.item_to_album(item) {
                        albums.push(album);
                    }
                }
                "MusicArtist" => {
                    if let Some(artist) = self.item_to_artist(item) {
                        artists.push(artist);
                    }
                }
                _ => {}
            }
        }

        SearchResults {
            tracks,
            albums,
            artists,
        }
    }

    /// Convert a Jellyfin item to a domain Album.
    fn item_to_album(&self, item: JellyfinItem) -> Option<Album> {
        // Extract borrowed values before consuming item fields
        let artist_id = item.primary_artist_id().unwrap_or_default();
        let artist_name = item.primary_artist_name().unwrap_or_default();
        let genre = item.genre_tag();
        let artwork_url = item
            .primary_image_tag()
            .map(|tag| self.get_artwork_url_with_tag(&item.id, tag, 300));
        let duration_ms = item.duration_ms();

        // Strip trailing year suffix like " (1990)" — Jellyfin sometimes
        // embeds the production year in the album name.
        let name = strip_year_suffix(&item.name);

        let release_mbid = item
            .provider_ids
            .as_ref()
            .and_then(|ids| ids.musicbrainz_album.clone());

        Some(Album {
            jellyfin_id: item.id,
            release_mbid,
            name,
            artist_name,
            artist_jellyfin_id: artist_id,
            year: item.production_year,
            genre,
            artwork_url,
            track_count: item.child_count.unwrap_or(0),
            duration_ms,
            downloaded_track_count: 0,
            date_created: item.date_created.as_deref().and_then(parse_date_created),
        })
    }

    /// Convert a Jellyfin item to a domain Track.
    fn item_to_track(&self, item: JellyfinItem) -> Option<Track> {
        // A track with no album is dropped by the `?` below, same rigour
        // applies to the id itself: a track's Jellyfin id is what streaming,
        // downloads and scrobbling key on end to end, so an item Jellyfin
        // somehow returned with a blank one must never be cached and played
        // as if it were a real, resolvable track.
        if item.id.is_empty() {
            tracing::warn!(
                "Skipping Jellyfin item '{}' with an empty Id — cannot be cached or played",
                item.name
            );
            return None;
        }

        // Extract borrowed values before consuming item fields
        let album_id = item.album_id.clone()?;
        let album_name = item.album.clone().unwrap_or_default();
        let artist_name = item.primary_artist_name().unwrap_or_default();
        let genre = item.genre_tag();
        let duration_ms = item.duration_ms();
        let file_size_bytes = item.file_size_bytes();
        let artwork_url = Some(self.get_artwork_url(&album_id, 300));

        // Use MusicBrainzRecording (the stable recording ID) not MusicBrainzTrack
        // (the release-track ID). Only the recording ID matches what ListenBrainz,
        // the scrobble server and the rest of the ecosystem store. Tracks without a recording
        // ID get NULL rather than a pollution fallback.
        let mbid = item
            .provider_ids
            .as_ref()
            .and_then(|ids| ids.musicbrainz_recording.clone());

        let artist_mbid = item
            .provider_ids
            .as_ref()
            .and_then(|ids| ids.musicbrainz_artist.clone());

        let release_mbid = item
            .provider_ids
            .as_ref()
            .and_then(|ids| ids.musicbrainz_album.clone());

        Some(Track {
            jellyfin_id: item.id,
            mbid,
            artist_mbid,
            release_mbid,
            name: item.name,
            artist_name,
            album_name,
            album_jellyfin_id: album_id,
            track_number: item.index_number.unwrap_or(0),
            disc_number: item.parent_index_number.unwrap_or(1),
            duration_ms,
            genre,
            year: item.production_year,
            artwork_url,
            cached_file_path: None,
            file_size_bytes,
            normalization_gain_db: item.normalization_gain,
        })
    }

    /// Convert a Jellyfin item to a domain Artist.
    fn item_to_artist(&self, item: JellyfinItem) -> Option<Artist> {
        let artwork_url = item
            .primary_image_tag()
            .map(|tag| self.get_artwork_url_with_tag(&item.id, tag, 300));

        let backdrop_url = if item.has_backdrop_image() {
            Some(self.get_backdrop_url(&item.id, 0, 1280))
        } else {
            None
        };

        let mbid = item
            .provider_ids
            .as_ref()
            .and_then(|ids| ids.musicbrainz_artist.clone());

        Some(Artist {
            jellyfin_id: item.id,
            mbid,
            name: item.name,
            artwork_url,
            backdrop_url,
            album_count: item.child_count.unwrap_or(0),
            sort_name: item.sort_name,
        })
    }

    /// Handle HTTP error status codes.
    ///
    /// A 401 here is a *session* expiry (this method serves only
    /// already-authenticated library/sync/playlist/search calls, never the
    /// login flow), so it fires the deduplicated auth-expiry notifier and maps
    /// to [`AppError::AuthExpired`] rather than the login-time
    /// [`AppError::Auth`].
    fn handle_error_status(&self, status: reqwest::StatusCode, operation: &str) -> AppError {
        match status {
            reqwest::StatusCode::UNAUTHORIZED => {
                self.fire_auth_expired();
                AppError::AuthExpired("Jellyfin session expired".to_string())
            }
            reqwest::StatusCode::NOT_FOUND => {
                AppError::NotFound(format!("Resource not found during {}", operation))
            }
            _ => AppError::Jellyfin(format!(
                "Jellyfin returned status {} during {}",
                status, operation
            )),
        }
    }

    // ========================================================================
    // Playlist Methods
    // ========================================================================

    /// Fetch all playlists for the current user.
    pub async fn get_playlists(&self) -> Result<Vec<crate::models::JellyfinPlaylist>, AppError> {
        let url = self.user_endpoint("/Users/{userId}/Items");

        let items_response: ItemsResponse = self
            .send_json(
                self.http().get(url).query(&[
                    ("IncludeItemTypes", "Playlist"),
                    ("Recursive", "true"),
                    ("SortBy", "SortName"),
                    ("SortOrder", "Ascending"),
                    ("Fields", "ChildCount"),
                ]),
                "fetch playlists",
            )
            .await?;

        let playlists = items_response
            .items
            .into_iter()
            .filter(|item| item.item_type == "Playlist")
            .map(|item| crate::models::JellyfinPlaylist {
                jellyfin_id: item.id,
                name: item.name,
                item_count: item.child_count.unwrap_or(0),
            })
            .collect();

        Ok(playlists)
    }

    /// Fetch an item's name and type (e.g. `"Playlist"`, `"MusicAlbum"`).
    ///
    /// Used to validate a foreign playlist link before fetching its
    /// tracks: the pasted item id could point at anything, so the caller
    /// checks `item_type == "Playlist"` and surfaces a clear error otherwise.
    /// Every other playlist-reading path already has the name from context
    /// (see `get_playlist_tracks`'s doc comment) and skips this round-trip.
    pub async fn get_item_summary(&self, item_id: &str) -> Result<(String, String), AppError> {
        let url = self.endpoint(&format!("/Users/{}/Items/{}", self.user_id(), item_id));

        let item: JellyfinItem = self.send_json(self.http().get(url), "fetch item").await?;

        Ok((item.name, item.item_type))
    }

    /// Fetch a playlist's tracks.
    ///
    /// Returns only the tracks (and the playlist ID). The playlist name is
    /// deliberately not fetched here: callers already hold it via the
    /// `JellyfinPlaylist` summary (see `get_playlists`), so a second
    /// `/Users/{userId}/Items/{playlistId}` metadata round-trip just to read
    /// the name would be redundant.
    pub async fn get_playlist_tracks(
        &self,
        playlist_id: &str,
    ) -> Result<crate::models::JellyfinPlaylistDetail, AppError> {
        // Fetch the playlist items directly — no metadata round-trip needed.
        let items_url = self.endpoint(&format!("/Playlists/{}/Items", playlist_id));

        let items_response: PlaylistItemsResponse = self
            .send_json(
                self.http().get(items_url).query(&[
                    ("UserId", self.user_id()),
                    // RunTimeTicks (duration) and AlbumArtists (artist fallback) are
                    // consumed by playlist_item_to_track. AlbumId/Album/AlbumArtist come back
                    // as base fields by default. Genres/MediaSources were dropped:
                    // they were deserialized and discarded, and MediaSources in particular
                    // forces the server to stat each item's media file — O(n) work that made
                    // first-open of a playlist take ~5s. ProviderIds is cheap (no file stat)
                    // and re-added to resolve the MusicBrainz recording ID for
                    // shared/imported playlists.
                    ("Fields", "RunTimeTicks,AlbumArtists,ProviderIds"),
                ]),
                "fetch playlist tracks",
            )
            .await?;

        let tracks = items_response
            .items
            .into_iter()
            .filter_map(|item| self.playlist_item_to_track(item))
            .collect();

        Ok(crate::models::JellyfinPlaylistDetail {
            jellyfin_id: playlist_id.to_string(),
            tracks,
        })
    }

    /// Create a new playlist.
    pub async fn create_playlist(
        &self,
        name: &str,
    ) -> Result<crate::models::JellyfinPlaylist, AppError> {
        let url = self.endpoint("/Playlists");

        let request = CreatePlaylistRequest {
            name: name.to_string(),
            ids: None,
            user_id: self.user_id().to_string(),
            media_type: "Audio".to_string(),
        };

        let create_response: CreatePlaylistResponse = self
            .send_json(self.http().post(url).json(&request), "create playlist")
            .await?;

        Ok(crate::models::JellyfinPlaylist {
            jellyfin_id: create_response.id,
            name: name.to_string(),
            item_count: 0,
        })
    }

    /// Delete a playlist.
    /// Rename a playlist (`POST /Playlists/{playlistId}`, `UpdatePlaylistDto`).
    ///
    /// Added to the Jellyfin playlist API alongside the reorder endpoint this
    /// client already calls (`move_playlist_item`, server ≥10.9), so no new
    /// minimum-version concern.
    pub async fn rename_playlist(&self, playlist_id: &str, name: &str) -> Result<(), AppError> {
        let url = self.endpoint(&format!("/Playlists/{}", playlist_id));

        let request = UpdatePlaylistRequest {
            name: name.to_string(),
        };

        self.send(self.http().post(url).json(&request), "rename playlist")
            .await?;

        Ok(())
    }

    pub async fn delete_playlist(&self, playlist_id: &str) -> Result<(), AppError> {
        let url = self.endpoint(&format!("/Items/{}", playlist_id));

        self.send(self.http().delete(url), "delete playlist")
            .await?;

        Ok(())
    }

    /// Add tracks to a playlist.
    ///
    /// `track_ids` are Jellyfin track IDs to add.
    pub async fn add_tracks_to_playlist(
        &self,
        playlist_id: &str,
        track_ids: &[String],
    ) -> Result<(), AppError> {
        if track_ids.is_empty() {
            return Ok(());
        }

        let ids = track_ids.join(",");
        let url = self.endpoint(&format!("/Playlists/{}/Items", playlist_id));

        self.send(
            self.http()
                .post(url)
                .query(&[("Ids", ids.as_str()), ("UserId", self.user_id())]),
            "add tracks to playlist",
        )
        .await?;

        Ok(())
    }

    /// Remove tracks from a playlist.
    ///
    /// `entry_ids` are playlist entry IDs (not track IDs).
    pub async fn remove_tracks_from_playlist(
        &self,
        playlist_id: &str,
        entry_ids: &[String],
    ) -> Result<(), AppError> {
        if entry_ids.is_empty() {
            return Ok(());
        }

        let ids = entry_ids.join(",");
        let url = self.endpoint(&format!("/Playlists/{}/Items", playlist_id));

        self.send(
            self.http().delete(url).query(&[("EntryIds", ids.as_str())]),
            "remove tracks from playlist",
        )
        .await?;

        Ok(())
    }

    /// Move a track within a playlist.
    ///
    /// `item_id` is the playlist **entry** ID (`PlaylistItemId`), not the track's
    /// Jellyfin ID — the same id used to remove an entry.
    /// `new_index` is the 0-based position to move the track to.
    pub async fn move_playlist_item(
        &self,
        playlist_id: &str,
        item_id: &str,
        new_index: u32,
    ) -> Result<(), AppError> {
        let url = self.endpoint(&format!(
            "/Playlists/{}/Items/{}/Move/{}",
            playlist_id, item_id, new_index
        ));

        self.send(self.http().post(url), "move playlist item")
            .await?;

        Ok(())
    }

    /// Convert a playlist item to a domain JellyfinPlaylistTrack.
    fn playlist_item_to_track(
        &self,
        item: PlaylistItem,
    ) -> Option<crate::models::JellyfinPlaylistTrack> {
        // Same rigour as `item_to_track`: a playlist entry whose
        // linked item Jellyfin returns with a blank Id can never be streamed,
        // downloaded or scrobbled, so it must never be handed back as a
        // playable track.
        if item.id.is_empty() {
            tracing::warn!(
                "Skipping playlist item '{}' with an empty Id — cannot be cached or played",
                item.name
            );
            return None;
        }

        let album_id = item.album_id.clone().unwrap_or_default();
        let album_name = item.album.clone().unwrap_or_default();
        let artist_name = item.primary_artist_name().unwrap_or_default();
        let duration_ms = item.duration_ms();
        let artwork_url = if !album_id.is_empty() {
            Some(self.get_artwork_url(&album_id, 300))
        } else {
            None
        };

        // Same recording-not-track-ID rule as `item_to_track` — see its comment.
        let mbid = item
            .provider_ids
            .as_ref()
            .and_then(|ids| ids.musicbrainz_recording.clone());

        Some(crate::models::JellyfinPlaylistTrack {
            jellyfin_id: item.id,
            mbid,
            playlist_item_id: item.playlist_item_id,
            name: item.name,
            artist_name,
            album_name,
            album_jellyfin_id: album_id,
            duration_ms,
            artwork_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn failed_mbid_chunks_never_look_like_retired_ids() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let empty = r#"{"Items":[],"TotalRecordCount":0}"#;
        for (responses, should_fail) in [
            (vec![(503, "")], true),
            (vec![(200, "invalid JSON")], true),
            (vec![(200, empty), (503, "")], true),
            (vec![(200, empty), (200, empty)], false),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                for (status, body) in responses {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut request = [0; 16384];
                    let received = socket.read(&mut request).await.unwrap();
                    assert!(received > 0);
                    let response = format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    socket.write_all(response.as_bytes()).await.unwrap();
                }
            });
            let client = JellyfinClient::new_with_device_id(
                &format!("http://{address}"),
                "test-key",
                "test-user",
                "test-device",
            )
            .unwrap();
            let ids = (0..201).map(|id| format!("track-{id}")).collect::<Vec<_>>();
            let result = client.get_track_mbids(&ids).await;
            assert_eq!(result.is_err(), should_fail);
            if let Ok(items) = result {
                assert!(items.is_empty());
            }
            server.await.unwrap();
        }
    }

    fn test_client() -> JellyfinClient {
        JellyfinClient::new_with_device_id(
            "https://jellyfin.example.com",
            "test-api-key",
            "test-user-id",
            "test-device-id",
        )
        .expect("test client should build")
    }

    /// A 401 on a session (already-authenticated) library call maps to the
    /// distinct `AuthExpired` variant, not the login-time `Auth`.
    /// With no callback installed, `fire_auth_expired` is a no-op, so this is
    /// hermetic (no network, no emit).
    #[test]
    fn unauthorized_maps_to_auth_expired() {
        let client = test_client();
        let err = client.handle_error_status(reqwest::StatusCode::UNAUTHORIZED, "fetch albums");
        assert!(
            matches!(err, AppError::AuthExpired(_)),
            "401 must map to AuthExpired, got {err:?}"
        );
    }

    /// Guards the match-arm ordering: non-401 statuses keep their mapping so the
    /// new arm cannot accidentally swallow other errors.
    #[test]
    fn other_statuses_keep_their_mapping() {
        let client = test_client();
        assert!(
            matches!(
                client.handle_error_status(reqwest::StatusCode::NOT_FOUND, "op"),
                AppError::NotFound(_)
            ),
            "404 must stay NotFound"
        );
        assert!(
            matches!(
                client.handle_error_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR, "op"),
                AppError::Jellyfin(_)
            ),
            "500 must stay Jellyfin"
        );
    }

    /// Deserialize a LyricDto from a Jellyfin-shaped (PascalCase) JSON payload.
    fn parse_dto(json: &str) -> LyricDto {
        serde_json::from_str(json).expect("LyricDto should deserialize")
    }

    /// An item with a blank Id must never be cached and played as if it were
    /// a real track — nothing can stream, download or scrobble without one.
    /// A malformed/degraded Jellyfin API response is the only way
    /// this can happen; Bragi must not trust an id blindly.
    #[test]
    fn item_to_track_skips_an_item_with_empty_id() {
        let client = test_client();
        let item: JellyfinItem = serde_json::from_str(
            r#"{"Id": "", "Name": "Dimes", "Type": "Audio", "AlbumId": "album-1"}"#,
        )
        .expect("JellyfinItem should deserialize");

        assert!(
            client.item_to_track(item).is_none(),
            "an item with an empty Id must never convert to a playable track"
        );
    }

    /// A normal item (non-empty Id) must still convert — the empty-id guard
    /// must not reject everything.
    #[test]
    fn item_to_track_keeps_a_normal_item() {
        let client = test_client();
        let item: JellyfinItem = serde_json::from_str(
            r#"{"Id": "45751bb32ad69b7b887dc7500a6653b7", "Name": "Dimes", "Type": "Audio", "AlbumId": "album-1"}"#,
        )
        .expect("JellyfinItem should deserialize");

        let track = client
            .item_to_track(item)
            .expect("a normal item must convert");
        assert_eq!(track.jellyfin_id, "45751bb32ad69b7b887dc7500a6653b7");
    }

    /// Same guard on the playlist path: a playlist entry whose linked
    /// item comes back with a blank Id must never be handed back as playable.
    #[test]
    fn playlist_item_to_track_skips_an_item_with_empty_id() {
        let client = test_client();
        let item: PlaylistItem =
            serde_json::from_str(r#"{"Id": "", "Name": "Dimes", "PlaylistItemId": "entry-1"}"#)
                .expect("PlaylistItem should deserialize");

        assert!(
            client.playlist_item_to_track(item).is_none(),
            "a playlist item with an empty Id must never convert to a playable track"
        );
    }

    /// `search()` now reads one mixed `/Items` response instead of a
    /// `/Search/Hints` hit plus a per-item detail fetch — this pins the pure
    /// dispatch (by `Type`) that replaced it, split by item type into the
    /// three domain collections, an unknown type dropped rather than erroring.
    #[test]
    fn items_to_search_results_dispatches_by_type() {
        let client = test_client();
        let items: Vec<JellyfinItem> = serde_json::from_str(
            r#"[
                {"Id": "track-1", "Name": "Dimes", "Type": "Audio", "AlbumId": "album-1"},
                {"Id": "album-1", "Name": "Currency", "Type": "MusicAlbum"},
                {"Id": "artist-1", "Name": "Change", "Type": "MusicArtist"},
                {"Id": "playlist-1", "Name": "Mixed Bag", "Type": "Playlist"}
            ]"#,
        )
        .expect("items should deserialize");

        let results = client.items_to_search_results(items);
        assert_eq!(
            results.tracks.len(),
            1,
            "the Audio item must become a track"
        );
        assert_eq!(results.tracks[0].jellyfin_id, "track-1");
        assert_eq!(
            results.albums.len(),
            1,
            "the MusicAlbum item must become an album"
        );
        assert_eq!(results.albums[0].jellyfin_id, "album-1");
        assert_eq!(
            results.artists.len(),
            1,
            "the MusicArtist item must become an artist"
        );
        assert_eq!(results.artists[0].jellyfin_id, "artist-1");
    }

    /// A track item that fails its own conversion (a blank Id) must be
    /// dropped by the dispatch, not surfaced as a broken result.
    #[test]
    fn items_to_search_results_drops_an_unconvertible_track() {
        let client = test_client();
        let items: Vec<JellyfinItem> = serde_json::from_str(
            r#"[{"Id": "", "Name": "Dimes", "Type": "Audio", "AlbumId": "album-1"}]"#,
        )
        .expect("items should deserialize");

        let results = client.items_to_search_results(items);
        assert!(results.tracks.is_empty());
    }

    /// Synced lyrics with Start ticks: ticks→ms, synced flag, order preserved.
    #[test]
    fn synced_lyrics_convert_ticks_to_ms() {
        let dto = parse_dto(
            r#"{
                "Metadata": { "IsSynced": true, "Offset": 0 },
                "Lyrics": [
                    { "Text": "first", "Start": 12300000 },
                    { "Text": "second", "Start": 24600000 }
                ]
            }"#,
        );

        let lyrics = lyric_dto_to_track_lyrics(dto).expect("synced lyrics present");
        assert!(lyrics.synced, "IsSynced=true must be synced");
        assert_eq!(lyrics.lines.len(), 2);
        // 12_300_000 ticks / 10_000 = 1230 ms
        assert_eq!(lyrics.lines[0].start_ms, Some(1230));
        assert_eq!(lyrics.lines[0].text, "first");
        assert_eq!(lyrics.lines[1].start_ms, Some(2460));
        assert_eq!(lyrics.lines[1].text, "second");
    }

    /// A legitimate zero-timestamp first line must keep `start_ms == Some(0)`
    /// (the corrected-behavior guard: absence is null, not 0).
    #[test]
    fn zero_timestamp_first_line_is_preserved() {
        let dto = parse_dto(
            r#"{
                "Metadata": { "IsSynced": true },
                "Lyrics": [
                    { "Text": "intro at zero", "Start": 0 },
                    { "Text": "later", "Start": 10000000 }
                ]
            }"#,
        );

        let lyrics = lyric_dto_to_track_lyrics(dto).expect("lyrics present");
        assert!(lyrics.synced);
        assert_eq!(
            lyrics.lines[0].start_ms,
            Some(0),
            "0ms first line must not be dropped"
        );
        assert_eq!(lyrics.lines[1].start_ms, Some(1000));
    }

    /// Unsynced lyrics (no Start, IsSynced=false): all start_ms None, text kept.
    #[test]
    fn unsynced_lyrics_have_no_timestamps() {
        let dto = parse_dto(
            r#"{
                "Metadata": { "IsSynced": false },
                "Lyrics": [
                    { "Text": "line one" },
                    { "Text": "line two" }
                ]
            }"#,
        );

        let lyrics = lyric_dto_to_track_lyrics(dto).expect("lyrics present");
        assert!(
            !lyrics.synced,
            "no timestamps and IsSynced=false → unsynced"
        );
        assert!(lyrics.lines.iter().all(|l| l.start_ms.is_none()));
        assert_eq!(lyrics.lines[0].text, "line one");
        assert_eq!(lyrics.lines[1].text, "line two");
    }

    /// LRC that omits IsSynced but carries Start times → fallback to synced.
    #[test]
    fn lrc_without_is_synced_but_with_start_is_synced() {
        let dto = parse_dto(
            r#"{
                "Metadata": {},
                "Lyrics": [
                    { "Text": "timed line", "Start": 5000000 }
                ]
            }"#,
        );

        let lyrics = lyric_dto_to_track_lyrics(dto).expect("lyrics present");
        assert!(
            lyrics.synced,
            "a line with a Start time must flip the synced fallback"
        );
        assert_eq!(lyrics.lines[0].start_ms, Some(500));
    }

    /// Empty (or missing) Lyrics array → Ok(None), the clean no-lyrics state.
    #[test]
    fn empty_lyrics_yield_none() {
        assert!(lyric_dto_to_track_lyrics(parse_dto(r#"{ "Lyrics": [] }"#)).is_none());
        assert!(lyric_dto_to_track_lyrics(parse_dto(r#"{ "Metadata": {} }"#)).is_none());
    }
}
