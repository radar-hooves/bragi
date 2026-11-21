//! OpenSubsonic API client.
//!
//! Mirrors `JellyfinClient`'s shape (two reqwest clients, an auth-expiry
//! callback, cheap `Clone`) but speaks the Subsonic REST protocol: every call
//! is `GET /rest/{view}.view` signed with query-param token auth (see `auth`),
//! and responses are wrapped in a `{"subsonic-response": {...}}` envelope.

use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

use futures_util::stream::{self, StreamExt};
use url::Url;

use crate::error::AppError;
use crate::models::{
    Album, Artist, JellyfinPlaylist, JellyfinPlaylistDetail, SearchResults, Track, TrackLyrics,
    TrackLyricsLine,
};

use super::auth;
use super::models::{
    album_to_album, artist_to_artist, playlist_detail, playlist_to_summary, song_to_track,
    AlbumList2, ArtistsIndex, LyricsList, Playlists, SearchResult3, SubsonicAlbum, SubsonicArtist,
    SubsonicPlaylistWithSongs, SubsonicSong,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DOWNLOAD_READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Max albums fetched across all `getAlbumList2` pages (safety cap).
const MAX_ALBUMS: usize = 50_000;
/// Server-side page size for `getAlbumList2` (its documented maximum).
const ALBUM_PAGE_SIZE: usize = 500;

/// Upper bound on ids fanned out to `getSong` in [`SubsonicClient::get_track_play_counts`].
///
/// Subsonic has no bulk `getSong`, so play counts are fetched one request per
/// id. Mirrors the ai_playlist `MAX_JELLYFIN_PLAY_COUNT_FETCH` ceiling so a
/// "most played over a huge library" request can't fan out unboundedly.
const MAX_PLAY_COUNT_FETCH: usize = 1000;
/// In-flight `getSong` requests when fetching play counts (bounded so a large
/// pool doesn't hammer the server).
const PLAY_COUNT_CONCURRENCY: usize = 8;

/// Credentials for an OpenSubsonic server.
#[derive(Clone)]
pub struct SubsonicCredentials {
    pub server_url: String,
    pub username: String,
    pub password: String,
}

impl fmt::Debug for SubsonicCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubsonicCredentials")
            .field("server_url", &self.server_url)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

/// OpenSubsonic API client. Cheaply cloneable (reqwest clients are `Arc`-backed).
#[derive(Clone)]
pub struct SubsonicClient {
    base_url: Url,
    username: String,
    password: String,
    http: reqwest_middleware::ClientWithMiddleware,
    download_http: reqwest::Client,
    auth_expired_cb: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl fmt::Debug for SubsonicClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubsonicClient")
            .field("base_url", &self.base_url)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

/// Normalise a user-entered server URL (prepend https, strip trailing slash).
fn normalise_server_url(raw: &str) -> Result<Url, AppError> {
    let trimmed = raw.trim().trim_end_matches('/');
    let with_scheme = if !trimmed.contains("://") {
        format!("https://{trimmed}")
    } else {
        trimmed.to_string()
    };
    Url::parse(&with_scheme)
        .map_err(|e| AppError::Config(format!("Invalid server URL '{raw}': {e}")))
}

impl SubsonicClient {
    /// Create a client for `server_url` authenticating as `username`/`password`.
    pub fn new(server_url: &str, username: &str, password: &str) -> Result<Self, AppError> {
        let base_url = normalise_server_url(server_url)?;
        let http = crate::http::client(crate::http::default_headers(), REQUEST_TIMEOUT);
        let download_http = reqwest::Client::builder()
            .default_headers(crate::http::default_headers())
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(DOWNLOAD_READ_TIMEOUT)
            .build()
            .map_err(|e| {
                AppError::Network(format!("Failed to create download HTTP client: {e}"))
            })?;
        Ok(Self {
            base_url,
            username: username.to_string(),
            password: password.to_string(),
            http,
            download_http,
            auth_expired_cb: None,
        })
    }

    /// Create a client from stored credentials.
    pub fn from_credentials(c: &SubsonicCredentials) -> Result<Self, AppError> {
        Self::new(&c.server_url, &c.username, &c.password)
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }
    pub fn username(&self) -> &str {
        &self.username
    }
    pub fn password(&self) -> &str {
        &self.password
    }
    pub fn http(&self) -> &reqwest_middleware::ClientWithMiddleware {
        &self.http
    }
    pub fn download_http(&self) -> &reqwest::Client {
        &self.download_http
    }

    /// Install the session-expiry callback, shared across clones.
    pub fn set_auth_expired_callback(&mut self, cb: std::sync::Arc<dyn Fn() + Send + Sync>) {
        self.auth_expired_cb = Some(cb);
    }

    pub(crate) fn fire_auth_expired(&self) {
        if let Some(cb) = &self.auth_expired_cb {
            cb();
        }
    }

    /// Build a `/rest/{view}.view` URL (without auth params yet).
    pub fn rest_endpoint(&self, view: &str) -> Url {
        let mut url = self.base_url.clone();
        url.set_path(&format!("/rest/{view}.view"));
        url
    }

    /// A credential-free cover-art reference for a `coverArt` id.
    ///
    /// Returns the port-free sentinel `subsonic-cover:{id}` rather than a signed
    /// `getCoverArt` URL. The salted token must never sit in a cached DB row or a
    /// frontend `<img src>`, and a proxy URL can't be persisted either (the proxy
    /// port is OS-assigned per launch). The sentinel is rewritten to a live
    /// `/cover/{id}` proxy URL at the read boundary
    /// ([`MediaClient::resolve_artwork_url`]); the proxy injects auth server-side.
    /// Returns `None` when there's no cover id.
    fn cover_art_url(&self, cover_id: Option<&str>) -> Option<String> {
        let id = cover_id.filter(|s| !s.is_empty())?;
        Some(format!(
            "{}{id}",
            crate::mediaserver::client::SUBSONIC_COVER_SENTINEL
        ))
    }

    /// A direct, authenticated `getCoverArt` URL for the internal disk-cache warm.
    ///
    /// NEVER returned to the frontend or persisted: the salted token rides in the
    /// query string. Used only by [`SubsonicClient::fetch_artwork_bytes`], which
    /// downloads the bytes with the client's own authenticated HTTP client.
    pub(crate) fn authed_cover_url(&self, id: &str, size: Option<u32>) -> String {
        let mut url = self.rest_endpoint("getCoverArt");
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("id", id);
            if let Some(s) = size {
                q.append_pair("size", &s.to_string());
            }
        }
        auth::append_auth(&mut url, &self.username, &self.password);
        url.to_string()
    }

    /// Send a signed GET and return the inner `subsonic-response` object,
    /// mapping a `status="failed"` body to an `AppError`.
    async fn get(
        &self,
        view: &str,
        params: &[(&str, &str)],
    ) -> Result<serde_json::Value, AppError> {
        let mut url = self.rest_endpoint(view);
        {
            let mut q = url.query_pairs_mut();
            for (k, v) in params {
                q.append_pair(k, v);
            }
        }
        auth::append_auth(&mut url, &self.username, &self.password);

        let response = self
            .http
            .get(url)
            .send()
            .await
            // Strip the URL: it carries the salted auth token (review).
            // The stripped error goes to the user-facing `AppError` and to
            // telemetry alike, so neither ever sees the token.
            .map_err(|e| {
                let e = e.without_url();
                telemetry::report_error_with_cause(
                    "Jellyfin or media-server request failed or timed out",
                    &e,
                );
                AppError::Network(format!("Subsonic request failed: {}", e))
            })?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.fire_auth_expired();
            return Err(AppError::AuthExpired(
                "Subsonic session expired".to_string(),
            ));
        }
        if !response.status().is_success() {
            return Err(AppError::Network(format!(
                "Subsonic HTTP {}",
                response.status()
            )));
        }

        let body: serde_json::Value = response.json().await.map_err(|e| {
            AppError::MediaServer(format!("Failed to parse Subsonic response: {e}"))
        })?;
        let resp = body.get("subsonic-response").ok_or_else(|| {
            AppError::MediaServer("missing subsonic-response envelope".to_string())
        })?;
        let status = resp
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("failed");
        if status != "ok" {
            let code = resp
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(|c| c.as_u64())
                .unwrap_or(0);
            let msg = resp
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            // 40 = wrong username/password, 41 = token auth not supported.
            if code == 40 || code == 41 {
                self.fire_auth_expired();
                return Err(AppError::AuthExpired(format!(
                    "Subsonic auth failed: {msg}"
                )));
            }
            return Err(AppError::MediaServer(format!(
                "Subsonic error {code}: {msg}"
            )));
        }
        Ok(resp.clone())
    }

    /// Deserialize a named sub-object of the response into `T`.
    fn extract<T: serde::de::DeserializeOwned>(
        resp: &serde_json::Value,
        key: &str,
    ) -> Result<T, AppError> {
        let value = resp.get(key).cloned().unwrap_or(serde_json::Value::Null);
        serde_json::from_value(value)
            .map_err(|e| AppError::MediaServer(format!("Failed to parse '{key}': {e}")))
    }

    /// Verify the server is reachable and the credentials are valid.
    pub async fn test_connection(&self) -> Result<(), AppError> {
        self.get("ping", &[]).await.map(|_| ())
    }

    /// All albums, via paginated `getAlbumList2`.
    pub async fn get_albums(&self) -> Result<Vec<Album>, AppError> {
        let mut albums = Vec::new();
        let mut offset = 0usize;
        loop {
            let offset_str = offset.to_string();
            let size_str = ALBUM_PAGE_SIZE.to_string();
            let resp = self
                .get(
                    "getAlbumList2",
                    &[
                        ("type", "alphabeticalByName"),
                        ("size", &size_str),
                        ("offset", &offset_str),
                    ],
                )
                .await?;
            let list: AlbumList2 = Self::extract(&resp, "albumList2")?;
            let n = list.album.len();
            for a in &list.album {
                albums.push(album_to_album(
                    a,
                    self.cover_art_url(a.cover_art.as_deref()),
                ));
            }
            if n < ALBUM_PAGE_SIZE || albums.len() >= MAX_ALBUMS {
                break;
            }
            offset += n;
        }
        Ok(albums)
    }

    /// An album's tracks (`getAlbum` returns them inline).
    pub async fn get_album_tracks(&self, album_id: &str) -> Result<Vec<Track>, AppError> {
        let resp = self.get("getAlbum", &[("id", album_id)]).await?;
        let album: SubsonicAlbum = Self::extract(&resp, "album")?;
        Ok(album
            .song
            .iter()
            .map(|s| song_to_track(s, self.cover_art_url(s.cover_art.as_deref())))
            .collect())
    }

    /// An album's tracks plus a `song_id → server play count` map.
    ///
    /// `getAlbum` already returns each song's `playCount` inline, so the map is
    /// captured for free during a browse/sync with no extra request. Only
    /// songs the server actually reported a count for appear in the map.
    pub async fn get_album_tracks_with_play_counts(
        &self,
        album_id: &str,
    ) -> Result<(Vec<Track>, HashMap<String, u32>), AppError> {
        let resp = self.get("getAlbum", &[("id", album_id)]).await?;
        let album: SubsonicAlbum = Self::extract(&resp, "album")?;
        let tracks = album
            .song
            .iter()
            .map(|s| song_to_track(s, self.cover_art_url(s.cover_art.as_deref())))
            .collect();
        let counts = album
            .song
            .iter()
            .filter_map(|s| s.play_count.map(|c| (s.id.clone(), c)))
            .collect();
        Ok((tracks, counts))
    }

    /// A single song (`getSong`), carrying its `playCount`/`played` fields.
    pub async fn get_song(&self, id: &str) -> Result<SubsonicSong, AppError> {
        let resp = self.get("getSong", &[("id", id)]).await?;
        Self::extract(&resp, "song")
    }

    /// Server-side play counts for the given song ids, keyed by song id.
    ///
    /// Mirrors `JellyfinClient::get_track_play_counts`: read-only (Bragi never
    /// writes play state back to the server) and best-effort — a `getSong` error
    /// for one id is skipped rather than failing the whole call. Subsonic has no
    /// bulk `getSong`, so ids are fanned out one request each with bounded
    /// concurrency and capped at [`MAX_PLAY_COUNT_FETCH`]. Prefer the cached
    /// `server_play_count` column (captured for free during sync) over this live
    /// fan-out where possible.
    pub async fn get_track_play_counts(
        &self,
        ids: &[String],
    ) -> Result<HashMap<String, u32>, AppError> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let capped = &ids[..ids.len().min(MAX_PLAY_COUNT_FETCH)];

        let pairs: Vec<(String, u32)> = stream::iter(capped.iter().cloned())
            .map(|id| async move {
                match self.get_song(&id).await {
                    Ok(song) => Some((id, song.play_count.unwrap_or(0))),
                    Err(e) => {
                        // Best-effort: one bad id must not drop the whole map.
                        tracing::debug!("getSong play-count fetch failed for '{id}': {e}");
                        None
                    }
                }
            })
            .buffer_unordered(PLAY_COUNT_CONCURRENCY)
            .filter_map(|pair| async move { pair })
            .collect()
            .await;

        Ok(pairs.into_iter().collect())
    }

    /// All album-artists, flattened from the `getArtists` index.
    pub async fn get_artists(&self) -> Result<Vec<Artist>, AppError> {
        let resp = self.get("getArtists", &[]).await?;
        let idx: ArtistsIndex = Self::extract(&resp, "artists")?;
        let mut artists = Vec::new();
        for bucket in &idx.index {
            for a in &bucket.artist {
                artists.push(artist_to_artist(
                    a,
                    self.cover_art_url(a.cover_art.as_deref()),
                ));
            }
        }
        Ok(artists)
    }

    /// Albums by a specific artist (`getArtist` returns them inline).
    pub async fn get_artist_albums(&self, artist_id: &str) -> Result<Vec<Album>, AppError> {
        let resp = self.get("getArtist", &[("id", artist_id)]).await?;
        let artist: SubsonicArtist = Self::extract(&resp, "artist")?;
        Ok(artist
            .album
            .iter()
            .map(|a| album_to_album(a, self.cover_art_url(a.cover_art.as_deref())))
            .collect())
    }

    /// Search tracks/albums/artists (`search3` returns full objects inline).
    pub async fn search(&self, query: &str) -> Result<SearchResults, AppError> {
        // search3 defaults each type's count to 20; request a useful cap so a
        // common query isn't silently truncated (review).
        let resp = self
            .get(
                "search3",
                &[
                    ("query", query),
                    ("songCount", "100"),
                    ("albumCount", "50"),
                    ("artistCount", "50"),
                ],
            )
            .await?;
        let r: SearchResult3 = Self::extract(&resp, "searchResult3")?;
        Ok(SearchResults {
            tracks: r
                .song
                .iter()
                .map(|s| song_to_track(s, self.cover_art_url(s.cover_art.as_deref())))
                .collect(),
            albums: r
                .album
                .iter()
                .map(|a| album_to_album(a, self.cover_art_url(a.cover_art.as_deref())))
                .collect(),
            artists: r
                .artist
                .iter()
                .map(|a| artist_to_artist(a, self.cover_art_url(a.cover_art.as_deref())))
                .collect(),
        })
    }

    /// Synced/plain lyrics via the OpenSubsonic `getLyricsBySongId` extension.
    ///
    /// A server without the extension is treated as "no lyrics" (`Ok(None)`)
    /// rather than an error, mirroring the Jellyfin lyrics behaviour.
    pub async fn get_lyrics(&self, song_id: &str) -> Result<Option<TrackLyrics>, AppError> {
        let resp = match self.get("getLyricsBySongId", &[("id", song_id)]).await {
            Ok(r) => r,
            // Extension unsupported / no lyrics → clean empty state.
            Err(AppError::MediaServer(_)) | Err(AppError::NotFound(_)) => return Ok(None),
            Err(e) => return Err(e),
        };
        let list: LyricsList = match Self::extract(&resp, "lyricsList") {
            Ok(l) => l,
            Err(_) => return Ok(None),
        };
        let Some(structured) = list.structured_lyrics.into_iter().next() else {
            return Ok(None);
        };
        if structured.line.is_empty() {
            return Ok(None);
        }
        let synced = structured.synced;
        let lines = structured
            .line
            .into_iter()
            .map(|l| TrackLyricsLine {
                start_ms: if synced { l.start } else { None },
                text: l.value,
            })
            .collect();
        Ok(Some(TrackLyrics { synced, lines }))
    }

    // ---- playlists --------------------------------------------------
    //
    // Mirrors `JellyfinClient`'s playlist surface through the shared domain
    // models. OpenSubsonic has no per-entry id (an entry's identity is its
    // 0-based position) and no move/reorder endpoint, so `playlist_item_id` is
    // the index, removal takes indices, and reorder is a `createPlaylist`
    // overwrite with the songs re-listed in the new order. Read-mostly and
    // credential-free: cover ids become `subsonic-cover:` sentinels.

    /// All playlists for the authenticated user (`getPlaylists`).
    pub async fn get_playlists(&self) -> Result<Vec<JellyfinPlaylist>, AppError> {
        let resp = self.get("getPlaylists", &[]).await?;
        let list: Playlists = Self::extract(&resp, "playlists")?;
        Ok(list.playlist.iter().map(playlist_to_summary).collect())
    }

    /// A playlist's ordered tracks (`getPlaylist`), each tagged with its 0-based
    /// index as the synthetic `playlist_item_id`.
    pub async fn get_playlist_tracks(
        &self,
        playlist_id: &str,
    ) -> Result<JellyfinPlaylistDetail, AppError> {
        let pl = self.fetch_playlist(playlist_id).await?;
        Ok(playlist_detail(playlist_id, &pl, |c| self.cover_art_url(c)))
    }

    /// Internal: fetch a playlist with its ordered entries.
    async fn fetch_playlist(
        &self,
        playlist_id: &str,
    ) -> Result<SubsonicPlaylistWithSongs, AppError> {
        let resp = self.get("getPlaylist", &[("id", playlist_id)]).await?;
        Self::extract(&resp, "playlist")
    }

    /// Create an empty playlist (`createPlaylist?name=`).
    ///
    /// OpenSubsonic (≥1.14) echoes the created playlist (with its new id) under
    /// the `playlist` key. Older servers return an empty body, so we fall back
    /// to re-listing and matching by name (newest by `changed`).
    pub async fn create_playlist(&self, name: &str) -> Result<JellyfinPlaylist, AppError> {
        let resp = self.get("createPlaylist", &[("name", name)]).await?;
        if let Ok(pl) = Self::extract::<SubsonicPlaylistWithSongs>(&resp, "playlist") {
            if !pl.id.is_empty() {
                return Ok(JellyfinPlaylist {
                    jellyfin_id: pl.id,
                    name: if pl.name.is_empty() {
                        name.to_string()
                    } else {
                        pl.name
                    },
                    item_count: pl.song_count.unwrap_or(0),
                });
            }
        }
        // Fallback for servers that don't echo the created playlist.
        self.find_playlist_by_name(name).await
    }

    /// Resolve a just-created playlist by name, preferring the newest `changed`.
    ///
    /// Racy for duplicate names (an inherent limit of the no-id fallback), but
    /// the only option when the server doesn't return the new playlist's id.
    async fn find_playlist_by_name(&self, name: &str) -> Result<JellyfinPlaylist, AppError> {
        let resp = self.get("getPlaylists", &[]).await?;
        let list: Playlists = Self::extract(&resp, "playlists")?;
        pick_newest_by_name(&list.playlist, name)
            .map(playlist_to_summary)
            .ok_or_else(|| {
                AppError::MediaServer(format!("created playlist '{name}' not found after create"))
            })
    }

    /// Rename a playlist (`updatePlaylist?playlistId=&name=`).
    pub async fn rename_playlist(&self, playlist_id: &str, name: &str) -> Result<(), AppError> {
        self.get(
            "updatePlaylist",
            &[("playlistId", playlist_id), ("name", name)],
        )
        .await
        .map(|_| ())
    }

    /// Delete a playlist (`deletePlaylist?id=`).
    pub async fn delete_playlist(&self, playlist_id: &str) -> Result<(), AppError> {
        self.get("deletePlaylist", &[("id", playlist_id)])
            .await
            .map(|_| ())
    }

    /// Append tracks to a playlist (`updatePlaylist?songIdToAdd=…`, repeated).
    pub async fn add_tracks_to_playlist(
        &self,
        playlist_id: &str,
        track_ids: &[String],
    ) -> Result<(), AppError> {
        if track_ids.is_empty() {
            return Ok(());
        }
        let mut params: Vec<(&str, &str)> = Vec::with_capacity(track_ids.len() + 1);
        params.push(("playlistId", playlist_id));
        for id in track_ids {
            params.push(("songIdToAdd", id));
        }
        self.get("updatePlaylist", &params).await.map(|_| ())
    }

    /// Remove entries by their 0-based indices (`updatePlaylist?songIndexToRemove=…`).
    ///
    /// The Subsonic analog of Jellyfin's remove-by-entry-id: `entry_ids` are the
    /// synthetic position strings this client hands out in [`playlist_detail`].
    /// Indices must match the server's CURRENT ordering — the command layer
    /// refetches after every mutation and serialises via the store's operating
    /// flag, so a caller never passes stale positions.
    pub async fn remove_by_indices(
        &self,
        playlist_id: &str,
        entry_ids: &[String],
    ) -> Result<(), AppError> {
        if entry_ids.is_empty() {
            return Ok(());
        }
        // Validate/normalise each index up front so a bad value fails cleanly
        // rather than silently removing the wrong entry.
        let indices: Vec<String> = entry_ids
            .iter()
            .map(|e| {
                e.parse::<u32>().map(|n| n.to_string()).map_err(|_| {
                    AppError::MediaServer(format!("invalid playlist entry index '{e}'"))
                })
            })
            .collect::<Result<_, _>>()?;
        let mut params: Vec<(&str, &str)> = Vec::with_capacity(indices.len() + 1);
        params.push(("playlistId", playlist_id));
        for i in &indices {
            params.push(("songIndexToRemove", i));
        }
        self.get("updatePlaylist", &params).await.map(|_| ())
    }

    /// Move an entry to a new position by rebuilding the playlist.
    ///
    /// Subsonic has no reorder endpoint. `item_id` is the entry's current 0-based
    /// index; we fetch the authoritative order immediately before the rebuild,
    /// compute the new song-id order, and overwrite the playlist via
    /// `createPlaylist?playlistId=&songId=…` (destructive replace of the entry
    /// list — hence the just-in-time refetch).
    pub async fn move_playlist_item(
        &self,
        playlist_id: &str,
        item_id: &str,
        new_index: u32,
    ) -> Result<(), AppError> {
        let from: usize = item_id.parse().map_err(|_| {
            AppError::MediaServer(format!("invalid playlist item index '{item_id}'"))
        })?;

        let pl = self.fetch_playlist(playlist_id).await?;
        let ids: Vec<String> = pl.entry.iter().map(|s| s.id.clone()).collect();
        if from >= ids.len() {
            return Err(AppError::MediaServer(format!(
                "playlist item index {from} out of range (length {})",
                ids.len()
            )));
        }

        let reordered = move_in_vec(ids, from, new_index as usize);

        let mut params: Vec<(&str, &str)> = Vec::with_capacity(reordered.len() + 2);
        params.push(("playlistId", playlist_id));
        // Preserve the name on overwrite (omit if empty so we never blank it).
        if !pl.name.is_empty() {
            params.push(("name", &pl.name));
        }
        for sid in &reordered {
            params.push(("songId", sid));
        }
        self.get("createPlaylist", &params).await.map(|_| ())
    }
}

/// Move the element at `from` to `to` (0-based), returning the new order.
///
/// Pure helper for the reorder-by-rebuild path: remove `from`, then insert at
/// `to` clamped to the post-removal length. Matches the frontend's optimistic
/// splice semantics (it already resolves `to` to the final resting index).
fn move_in_vec(mut ids: Vec<String>, from: usize, to: usize) -> Vec<String> {
    if from >= ids.len() {
        return ids;
    }
    let item = ids.remove(from);
    let to = to.min(ids.len());
    ids.insert(to, item);
    ids
}

/// Pick the playlist named `name` with the newest `changed` timestamp.
///
/// Backs the `create_playlist` fallback when the server doesn't echo the new
/// playlist's id: ISO-8601 timestamps sort lexicographically, so string `max`
/// yields the most recently changed match. `None` when nothing matches.
fn pick_newest_by_name<'a>(
    playlists: &'a [super::models::SubsonicPlaylist],
    name: &str,
) -> Option<&'a super::models::SubsonicPlaylist> {
    playlists.iter().filter(|p| p.name == name).max_by(|a, b| {
        a.changed
            .as_deref()
            .unwrap_or("")
            .cmp(b.changed.as_deref().unwrap_or(""))
    })
}

// ---------------------------------------------------------------------------
// Credential storage (system keyring, service "bragi-subsonic")
// ---------------------------------------------------------------------------

const KEYRING_SERVICE: &str = "bragi-subsonic";
const CREDENTIALS_KEY: &str = "credentials";

/// Subsonic credentials serialised as a single keyring entry.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredCredentials {
    server_url: String,
    username: String,
    password: String,
}

/// Store OpenSubsonic credentials in the system keyring.
pub fn store_credentials(c: &SubsonicCredentials) -> Result<(), AppError> {
    let stored = StoredCredentials {
        server_url: c.server_url.clone(),
        username: c.username.clone(),
        password: c.password.clone(),
    };
    let json = serde_json::to_string(&stored)
        .map_err(|e| AppError::Config(format!("Failed to serialise Subsonic credentials: {e}")))?;
    crate::credential_store::credential_set(KEYRING_SERVICE, CREDENTIALS_KEY, &json)
}

/// Load OpenSubsonic credentials from the keyring; `None` if unset.
pub fn load_credentials() -> Result<Option<SubsonicCredentials>, AppError> {
    match crate::credential_store::credential_get(KEYRING_SERVICE, CREDENTIALS_KEY)? {
        Some(json) => {
            let s: StoredCredentials = serde_json::from_str(&json).map_err(|e| {
                AppError::Config(format!("Failed to parse Subsonic credentials: {e}"))
            })?;
            Ok(Some(SubsonicCredentials {
                server_url: s.server_url,
                username: s.username,
                password: s.password,
            }))
        }
        None => Ok(None),
    }
}

/// Clear stored OpenSubsonic credentials (idempotent).
pub fn clear_credentials() -> Result<(), AppError> {
    crate::credential_store::credential_delete(KEYRING_SERVICE, CREDENTIALS_KEY)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> SubsonicClient {
        SubsonicClient::new("https://demo.navidrome.org", "demo", "sesame").unwrap()
    }

    #[test]
    fn cover_art_url_returns_credential_free_sentinel() {
        let client = test_client();
        let sentinel = client.cover_art_url(Some("al-9")).unwrap();

        assert_eq!(sentinel, "subsonic-cover:al-9");
        // No auth material and no fetchable URL may appear in the sentinel.
        for needle in ["t=", "s=", "u=", "token", "sesame", "http", "getCoverArt"] {
            assert!(
                !sentinel.contains(needle),
                "sentinel leaked '{needle}': {sentinel}"
            );
        }
    }

    #[test]
    fn cover_art_url_none_for_missing_or_empty_id() {
        let client = test_client();
        assert_eq!(client.cover_art_url(None), None);
        assert_eq!(client.cover_art_url(Some("")), None);
    }

    #[tokio::test]
    async fn get_track_play_counts_empty_ids_short_circuits_without_network() {
        // An empty id list must return an empty map without issuing any request
        // (the demo host is never contacted here).
        let client = test_client();
        let counts = client.get_track_play_counts(&[]).await.unwrap();
        assert!(counts.is_empty());
    }

    #[test]
    fn move_in_vec_reorders_forward_and_backward() {
        let ids: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        // Move index 0 -> 2 (matches the frontend's post-removal target index).
        assert_eq!(move_in_vec(ids.clone(), 0, 2), vec!["b", "c", "a", "d"]);
        // Move index 3 -> 0 (to the front).
        assert_eq!(move_in_vec(ids.clone(), 3, 0), vec!["d", "a", "b", "c"]);
        // Move index 1 -> 1 is a no-op.
        assert_eq!(move_in_vec(ids.clone(), 1, 1), vec!["a", "b", "c", "d"]);
        // Target past the end clamps to the tail.
        assert_eq!(move_in_vec(ids.clone(), 0, 99), vec!["b", "c", "d", "a"]);
        // Out-of-range source is a safe no-op.
        assert_eq!(move_in_vec(ids.clone(), 9, 0), ids);
    }

    #[test]
    fn create_playlist_happy_path_reads_returned_id() {
        // A ≥1.14 server echoes the created playlist (with its id) under
        // "playlist"; extraction must yield the new id and name.
        let resp = serde_json::json!({
            "playlist": {"id":"pl-42","name":"Roadtrip","songCount":0}
        });
        let pl: SubsonicPlaylistWithSongs = SubsonicClient::extract(&resp, "playlist").unwrap();
        assert_eq!(pl.id, "pl-42");
        assert_eq!(pl.name, "Roadtrip");
    }

    #[test]
    fn create_playlist_fallback_matches_newest_by_changed() {
        // No "playlist" key → extraction fails, forcing the match-by-name path.
        let resp = serde_json::json!({"status":"ok"});
        assert!(SubsonicClient::extract::<SubsonicPlaylistWithSongs>(&resp, "playlist").is_err());

        // getPlaylists fallback: two playlists share the name; the newest
        // `changed` must win, and a differently-named one is ignored.
        let list: Playlists = serde_json::from_value(serde_json::json!({
            "playlist": [
                {"id":"old","name":"Mix","changed":"2026-07-01T00:00:00Z"},
                {"id":"new","name":"Mix","changed":"2026-07-05T00:00:00Z"},
                {"id":"other","name":"Other","changed":"2026-07-09T00:00:00Z"}
            ]
        }))
        .unwrap();
        let picked = pick_newest_by_name(&list.playlist, "Mix").unwrap();
        assert_eq!(picked.id, "new");
        assert!(pick_newest_by_name(&list.playlist, "Missing").is_none());
    }

    #[test]
    fn authed_cover_url_carries_auth_but_not_password() {
        let client = test_client();
        let url = client.authed_cover_url("al-9", Some(300));

        assert!(url.contains("/rest/getCoverArt.view"));
        assert!(url.contains("id=al-9"));
        assert!(url.contains("size=300"));
        // Salted token auth present …
        assert!(url.contains("u=demo"));
        assert!(url.contains("t="));
        assert!(url.contains("s="));
        // … but the raw password is never in the URL.
        assert!(!url.contains("sesame"));
    }
}
