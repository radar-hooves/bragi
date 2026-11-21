//! Media-server client dispatch.
//!
//! `MediaClient` is a thin enum-dispatch wrapper over the two concrete backend
//! clients — `JellyfinClient` and `SubsonicClient` — exposing exactly the
//! capability surface the app calls. It mirrors the enum-dispatch style of
//! `llm/`: no trait objects, just a `match` per method that forwards to the
//! active backend.
//!
//! Both inner clients are cheap to `Clone` (their `reqwest::Client`s are
//! `Arc`-backed), so `MediaClient` is `Clone` too and the app keeps cloning it
//! out of `Arc<RwLock<Option<MediaClient>>>` before awaiting, preserving the
//! release-before-await discipline.
//!
//! # Backend parity
//!
//! Browse / stream / artwork / lyrics / download / server play counts /
//! playlists are implemented on both backends today (Jellyfin reads
//! `UserData.PlayCount`; OpenSubsonic reads the inline `playCount` on songs —
//! both read-only, Bragi never writes play state back). Playlist writes go to
//! whichever backend is active: OpenSubsonic maps entry identity to 0-based
//! position (no per-entry id) and implements reorder as a `createPlaylist`
//! overwrite (no move endpoint) — see `subsonic::client`.

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use url::Url;

use crate::error::AppError;
use crate::jellyfin::JellyfinClient;
use crate::models::{
    Album, Artist, JellyfinPlaylist, JellyfinPlaylistDetail, SearchResults, Track, TrackLyrics,
};

use super::kind::ServerKind;
use super::subsonic::SubsonicClient;

/// A connected media-server client: either Jellyfin or an OpenSubsonic server.
#[derive(Debug, Clone)]
pub enum MediaClient {
    Jellyfin(JellyfinClient),
    Subsonic(SubsonicClient),
}

/// Sentinel prefix stored in `cached_*.artwork_url` (and returned by the
/// OpenSubsonic mappers) in place of a real cover URL.
///
/// A cover reference is persisted as `subsonic-cover:{coverArt_id}` — port-free
/// and credential-free — and rewritten to a live `/cover/{id}` proxy URL at the
/// read boundary by [`MediaClient::resolve_artwork_url`]. Jellyfin artwork URLs
/// are already unauthenticated and pass through unchanged.
pub const SUBSONIC_COVER_SENTINEL: &str = "subsonic-cover:";

/// Rewrite a stored/mapped cover reference to a live proxy URL, if needed.
///
/// A `subsonic-cover:{id}` sentinel becomes
/// `http://127.0.0.1:{proxy_port}/cover/{id}`; any other value (a Jellyfin
/// `/Items/…` URL, an `artwork://` reference, or `None`) passes through
/// unchanged. Backend-agnostic and instance-free so offline cache reads (which
/// may have no live `MediaClient`) can call it too.
pub fn resolve_artwork_url(url: Option<String>, proxy_port: u16) -> Option<String> {
    match url {
        Some(u) => match u.strip_prefix(SUBSONIC_COVER_SENTINEL) {
            Some(id) => Some(format!("http://127.0.0.1:{proxy_port}/cover/{id}")),
            None => Some(u),
        },
        None => None,
    }
}

/// Rewrite the cover sentinel in a batch of albums to live proxy URLs.
pub fn resolve_albums_artwork(albums: &mut [Album], proxy_port: u16) {
    for a in albums.iter_mut() {
        a.artwork_url = resolve_artwork_url(a.artwork_url.take(), proxy_port);
    }
}

/// Rewrite the cover sentinel in a batch of artists to live proxy URLs.
pub fn resolve_artists_artwork(artists: &mut [Artist], proxy_port: u16) {
    for a in artists.iter_mut() {
        a.artwork_url = resolve_artwork_url(a.artwork_url.take(), proxy_port);
    }
}

/// Rewrite the cover sentinel in a batch of tracks to live proxy URLs.
pub fn resolve_tracks_artwork(tracks: &mut [Track], proxy_port: u16) {
    for t in tracks.iter_mut() {
        t.artwork_url = resolve_artwork_url(t.artwork_url.take(), proxy_port);
    }
}

impl MediaClient {
    // ---- identity / session ------------------------------------------------

    /// Which backend this client speaks to.
    pub fn kind(&self) -> ServerKind {
        match self {
            MediaClient::Jellyfin(_) => ServerKind::Jellyfin,
            MediaClient::Subsonic(_) => ServerKind::OpenSubsonic,
        }
    }

    /// The normalised server base URL.
    pub fn base_url(&self) -> &Url {
        match self {
            MediaClient::Jellyfin(c) => c.base_url(),
            MediaClient::Subsonic(c) => c.base_url(),
        }
    }

    /// The untimed HTTP client for large/slow downloads.
    pub fn download_http(&self) -> &reqwest::Client {
        match self {
            MediaClient::Jellyfin(c) => c.download_http(),
            MediaClient::Subsonic(c) => c.download_http(),
        }
    }

    /// Install the session-expiry callback, shared across clones.
    pub fn set_auth_expired_callback(&mut self, cb: Arc<dyn Fn() + Send + Sync>) {
        match self {
            MediaClient::Jellyfin(c) => c.set_auth_expired_callback(cb),
            MediaClient::Subsonic(c) => c.set_auth_expired_callback(cb),
        }
    }

    // ---- connection --------------------------------------------------------

    /// Verify the server is reachable and the credentials are valid.
    pub async fn test_connection(&self) -> Result<(), AppError> {
        match self {
            MediaClient::Jellyfin(c) => c.test_connection().await,
            MediaClient::Subsonic(c) => c.test_connection().await,
        }
    }

    // ---- library browsing (implemented on both backends) -------------------

    pub async fn get_albums(&self) -> Result<Vec<Album>, AppError> {
        match self {
            MediaClient::Jellyfin(c) => c.get_albums().await,
            MediaClient::Subsonic(c) => c.get_albums().await,
        }
    }

    pub async fn get_album_tracks(&self, album_id: &str) -> Result<Vec<Track>, AppError> {
        match self {
            MediaClient::Jellyfin(c) => c.get_album_tracks(album_id).await,
            MediaClient::Subsonic(c) => c.get_album_tracks(album_id).await,
        }
    }

    /// An album's tracks plus a `server_id → server play count` map, captured in
    /// the same browse request where the backend reports it.
    ///
    /// OpenSubsonic returns `playCount` inline on `getAlbum` songs — free to
    /// capture during sync. Jellyfin's album-tracks endpoint doesn't carry per-
    /// user play counts (they come from a separate `UserData` fetch), so the map
    /// is empty for Jellyfin, matching its existing play-count flow.
    pub async fn get_album_tracks_with_play_counts(
        &self,
        album_id: &str,
    ) -> Result<(Vec<Track>, HashMap<String, u32>), AppError> {
        match self {
            MediaClient::Jellyfin(c) => Ok((c.get_album_tracks(album_id).await?, HashMap::new())),
            MediaClient::Subsonic(c) => c.get_album_tracks_with_play_counts(album_id).await,
        }
    }

    pub async fn get_artists(&self) -> Result<Vec<Artist>, AppError> {
        match self {
            MediaClient::Jellyfin(c) => c.get_artists().await,
            MediaClient::Subsonic(c) => c.get_artists().await,
        }
    }

    pub async fn get_artist_albums(&self, artist_id: &str) -> Result<Vec<Album>, AppError> {
        match self {
            MediaClient::Jellyfin(c) => c.get_artist_albums(artist_id).await,
            MediaClient::Subsonic(c) => c.get_artist_albums(artist_id).await,
        }
    }

    pub async fn search(&self, query: &str) -> Result<SearchResults, AppError> {
        match self {
            MediaClient::Jellyfin(c) => c.search(query).await,
            MediaClient::Subsonic(c) => c.search(query).await,
        }
    }

    /// Built-in lyrics for a track (`Ok(None)` = no lyrics on either backend).
    pub async fn get_lyrics(&self, item_id: &str) -> Result<Option<TrackLyrics>, AppError> {
        match self {
            MediaClient::Jellyfin(c) => c.get_lyrics(item_id).await,
            MediaClient::Subsonic(c) => c.get_lyrics(item_id).await,
        }
    }

    // ---- play counts (both backends; read-only) ----------------------------

    /// Server-side play counts for the given track ids, keyed by server id.
    ///
    /// Read-only on both backends — Bragi never writes play state back. Jellyfin
    /// batches `UserData.PlayCount`; OpenSubsonic fans out `getSong` (no bulk
    /// endpoint), bounded and best-effort.
    pub async fn get_track_play_counts(
        &self,
        ids: &[String],
    ) -> Result<HashMap<String, u32>, AppError> {
        match self {
            MediaClient::Jellyfin(c) => c.get_track_play_counts(ids).await,
            MediaClient::Subsonic(c) => c.get_track_play_counts(ids).await,
        }
    }

    // ---- streaming / artwork URLs (both backends, synchronous) -------------

    pub fn get_stream_url(&self, item_id: &str, proxy_port: u16, proxy_token: &str) -> String {
        match self {
            MediaClient::Jellyfin(c) => c.get_stream_url(item_id, proxy_port, proxy_token),
            MediaClient::Subsonic(c) => c.get_stream_url(item_id, proxy_port, proxy_token),
        }
    }

    pub fn get_download_url(&self, item_id: &str, proxy_port: u16, proxy_token: &str) -> String {
        match self {
            MediaClient::Jellyfin(c) => c.get_download_url(item_id, proxy_port, proxy_token),
            MediaClient::Subsonic(c) => c.get_download_url(item_id, proxy_port, proxy_token),
        }
    }

    pub fn get_transcoded_stream_url(
        &self,
        item_id: &str,
        container: &str,
        audio_codec: &str,
        bitrate: Option<u32>,
    ) -> String {
        match self {
            MediaClient::Jellyfin(c) => {
                c.get_transcoded_stream_url(item_id, container, audio_codec, bitrate)
            }
            MediaClient::Subsonic(c) => {
                c.get_transcoded_stream_url(item_id, container, audio_codec, bitrate)
            }
        }
    }

    /// Build a ready-to-render cover URL for the `get_artwork_url` command.
    ///
    /// Jellyfin returns its unauthenticated `/Items/{id}/Images/Primary` URL
    /// directly; OpenSubsonic returns a credential-free `/cover/{id}?size=` proxy
    /// URL (the proxy injects the salted token server-side).
    pub fn cover_proxy_url(&self, item_id: &str, size: u32, proxy_port: u16) -> String {
        match self {
            MediaClient::Jellyfin(c) => c.get_artwork_url(item_id, size),
            MediaClient::Subsonic(_) => {
                format!("http://127.0.0.1:{proxy_port}/cover/{item_id}?size={size}")
            }
        }
    }

    /// Fetch cover-art bytes server-side for the disk-cache warm.
    ///
    /// Jellyfin builds its Primary-image URL and downloads it; OpenSubsonic signs
    /// a `getCoverArt` request internally. Never exposes a credential-bearing URL
    /// to the caller.
    pub async fn fetch_artwork_bytes(&self, item_id: &str, size: u32) -> Result<Bytes, AppError> {
        match self {
            MediaClient::Jellyfin(c) => {
                let url = c.get_artwork_url(item_id, size);
                c.download_bytes(&url).await
            }
            MediaClient::Subsonic(c) => c.fetch_artwork_bytes(item_id, size).await,
        }
    }

    // ---- raw downloads (both backends) -------------------------------------

    pub async fn download_bytes_capped(
        &self,
        url: &str,
        max_bytes: usize,
    ) -> Result<Option<Bytes>, AppError> {
        match self {
            MediaClient::Jellyfin(c) => c.download_bytes_capped(url, max_bytes).await,
            MediaClient::Subsonic(c) => c.download_bytes_capped(url, max_bytes).await,
        }
    }

    // ---- playlists (both backends) -----------------------------------
    //
    // OpenSubsonic has no per-entry id and no reorder endpoint, so the Subsonic
    // client maps entry identity to 0-based position and rebuilds the playlist
    // on reorder. Signatures are identical to Jellyfin's so the Tauri commands
    // dispatch transparently across backends.

    pub async fn get_playlists(&self) -> Result<Vec<JellyfinPlaylist>, AppError> {
        match self {
            MediaClient::Jellyfin(c) => c.get_playlists().await,
            MediaClient::Subsonic(c) => c.get_playlists().await,
        }
    }

    pub async fn get_playlist_tracks(
        &self,
        playlist_id: &str,
    ) -> Result<JellyfinPlaylistDetail, AppError> {
        match self {
            MediaClient::Jellyfin(c) => c.get_playlist_tracks(playlist_id).await,
            MediaClient::Subsonic(c) => c.get_playlist_tracks(playlist_id).await,
        }
    }

    pub async fn create_playlist(&self, name: &str) -> Result<JellyfinPlaylist, AppError> {
        match self {
            MediaClient::Jellyfin(c) => c.create_playlist(name).await,
            MediaClient::Subsonic(c) => c.create_playlist(name).await,
        }
    }

    pub async fn delete_playlist(&self, playlist_id: &str) -> Result<(), AppError> {
        match self {
            MediaClient::Jellyfin(c) => c.delete_playlist(playlist_id).await,
            MediaClient::Subsonic(c) => c.delete_playlist(playlist_id).await,
        }
    }

    pub async fn rename_playlist(&self, playlist_id: &str, name: &str) -> Result<(), AppError> {
        match self {
            MediaClient::Jellyfin(c) => c.rename_playlist(playlist_id, name).await,
            MediaClient::Subsonic(c) => c.rename_playlist(playlist_id, name).await,
        }
    }

    pub async fn add_tracks_to_playlist(
        &self,
        playlist_id: &str,
        track_ids: &[String],
    ) -> Result<(), AppError> {
        match self {
            MediaClient::Jellyfin(c) => c.add_tracks_to_playlist(playlist_id, track_ids).await,
            MediaClient::Subsonic(c) => c.add_tracks_to_playlist(playlist_id, track_ids).await,
        }
    }

    /// Remove playlist entries.
    ///
    /// `entry_ids` are Jellyfin `PlaylistItemId`s for Jellyfin, and 0-based
    /// position strings for OpenSubsonic (its entries have no id) — the latter
    /// routes to `remove_by_indices`.
    pub async fn remove_tracks_from_playlist(
        &self,
        playlist_id: &str,
        entry_ids: &[String],
    ) -> Result<(), AppError> {
        match self {
            MediaClient::Jellyfin(c) => c.remove_tracks_from_playlist(playlist_id, entry_ids).await,
            MediaClient::Subsonic(c) => c.remove_by_indices(playlist_id, entry_ids).await,
        }
    }

    pub async fn move_playlist_item(
        &self,
        playlist_id: &str,
        item_id: &str,
        new_index: u32,
    ) -> Result<(), AppError> {
        match self {
            MediaClient::Jellyfin(c) => c.move_playlist_item(playlist_id, item_id, new_index).await,
            MediaClient::Subsonic(c) => c.move_playlist_item(playlist_id, item_id, new_index).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mediaserver::subsonic::SubsonicClient;

    /// Regression guard: the Subsonic play-count arm must no longer
    /// return the `subsonic_unsupported` sentinel. An empty id list exercises the
    /// dispatch without any network I/O and must yield an empty map, not an error.
    #[tokio::test]
    async fn subsonic_play_counts_no_longer_unsupported() {
        let client = MediaClient::Subsonic(
            SubsonicClient::new("https://demo.navidrome.org", "demo", "sesame").unwrap(),
        );
        let counts = client.get_track_play_counts(&[]).await.unwrap();
        assert!(counts.is_empty());
    }

    #[test]
    fn resolve_artwork_url_rewrites_subsonic_sentinel() {
        assert_eq!(
            resolve_artwork_url(Some("subsonic-cover:al-9".to_string()), 45321),
            Some("http://127.0.0.1:45321/cover/al-9".to_string())
        );
    }

    #[test]
    fn resolve_artwork_url_passes_through_jellyfin_and_none() {
        // A Jellyfin Primary-image URL is already credential-free — untouched.
        let jf = "https://jf.example.com/Items/abc/Images/Primary?maxWidth=300".to_string();
        assert_eq!(
            resolve_artwork_url(Some(jf.clone()), 45321),
            Some(jf.clone())
        );
        // `None` stays `None`.
        assert_eq!(resolve_artwork_url(None, 45321), None);
        // An already-cached artwork:// reference is not a sentinel — untouched.
        let art = "artwork://localhost/abc.jpg".to_string();
        assert_eq!(resolve_artwork_url(Some(art.clone()), 45321), Some(art));
    }

    #[test]
    fn resolve_albums_artwork_rewrites_in_place() {
        let mut albums = vec![
            Album {
                jellyfin_id: "al-9".to_string(),
                release_mbid: None,
                name: "A".to_string(),
                artist_name: String::new(),
                artist_jellyfin_id: String::new(),
                year: None,
                genre: None,
                artwork_url: Some("subsonic-cover:al-9".to_string()),
                track_count: 0,
                duration_ms: 0,
                downloaded_track_count: 0,
                date_created: None,
            },
            Album {
                jellyfin_id: "al-10".to_string(),
                release_mbid: None,
                name: "B".to_string(),
                artist_name: String::new(),
                artist_jellyfin_id: String::new(),
                year: None,
                genre: None,
                artwork_url: None,
                track_count: 0,
                duration_ms: 0,
                downloaded_track_count: 0,
                date_created: None,
            },
        ];
        resolve_albums_artwork(&mut albums, 6000);
        assert_eq!(
            albums[0].artwork_url.as_deref(),
            Some("http://127.0.0.1:6000/cover/al-9")
        );
        assert_eq!(albums[1].artwork_url, None);
    }
}
