//! Jellyfin audio streaming and artwork URL generation.
//!
//! Generates URLs for streaming audio and fetching artwork from Jellyfin.
//!
//! # Security
//!
//! Audio stream URLs can be accessed in two ways:
//! 1. Via the local proxy (injects `X-Emby-Token` header) - used by frontend
//! 2. Direct URLs with auth header for mpv - avoids proxy complexity
//!
//! API keys never appear in URLs. Artwork URLs don't require authentication
//! on most Jellyfin configurations.

use super::client::JellyfinClient;

impl JellyfinClient {
    /// Get the streaming URL for an audio item via the local proxy.
    ///
    /// Returns a localhost URL that the proxy will forward to Jellyfin
    /// with authentication headers injected.
    ///
    /// # Arguments
    /// * `item_id` - The Jellyfin item ID of the audio track
    /// * `proxy_port` - Port of the local proxy server
    /// * `proxy_token` - Per-session proxy auth token, carried as `?tk=` so the
    ///   loopback proxy accepts the request and injects credentials.
    pub fn get_stream_url(&self, item_id: &str, proxy_port: u16, proxy_token: &str) -> String {
        format!(
            "http://127.0.0.1:{}/stream/{}?tk={}",
            proxy_port, item_id, proxy_token
        )
    }

    /// Get a direct Jellyfin streaming URL with server-side transcoding.
    ///
    /// Used by the offline download path to fetch a transcoded copy. The URL is
    /// hit with the authenticated HTTP client (auth is a baked-in default
    /// header), so no api_key/token appears in the URL.
    ///
    /// # Arguments
    /// * `item_id` - The Jellyfin item ID of the audio track
    /// * `container` - Target container format (e.g., "mp3", "m4a", "opus", "flac")
    /// * `audio_codec` - Audio codec (e.g., "mp3", "aac", "opus", "flac")
    /// * `bitrate` - Target bitrate in kbps for lossy formats, or `None` for
    ///   lossless (FLAC) where the bitrate parameter is omitted entirely.
    pub fn get_transcoded_stream_url(
        &self,
        item_id: &str,
        container: &str,
        audio_codec: &str,
        bitrate: Option<u32>,
    ) -> String {
        let mut url = self.endpoint(&format!("/Audio/{}/stream.{}", item_id, container));

        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("audioCodec", audio_codec);
            if let Some(kbps) = bitrate {
                pairs.append_pair("audioBitRate", &(kbps * 1000).to_string());
            }
        }

        // Transcoded streams use the authenticated HTTP client directly
        // (not user-facing URLs), so header auth is used via the client.
        url.to_string()
    }

    /// Get the artwork URL for an item.
    ///
    /// Returns a URL for the primary image of an item (album cover, artist photo).
    /// Artwork URLs do not require authentication on most Jellyfin setups.
    ///
    /// # Arguments
    /// * `item_id` - The Jellyfin item ID
    /// * `size` - Maximum width/height in pixels (e.g., 300 for thumbnails, 1000 for full)
    pub fn get_artwork_url(&self, item_id: &str, size: u32) -> String {
        let mut url = self.endpoint(&format!("/Items/{}/Images/Primary", item_id));

        url.query_pairs_mut()
            .append_pair("maxWidth", &size.to_string())
            .append_pair("maxHeight", &size.to_string())
            .append_pair("quality", "90");

        url.to_string()
    }

    /// Get the artwork URL with a specific image tag.
    ///
    /// Using an image tag allows Jellyfin to return a cached version if the
    /// image hasn't changed.
    ///
    /// # Arguments
    /// * `item_id` - The Jellyfin item ID
    /// * `tag` - The image tag from the item's ImageTags
    /// * `size` - Maximum width/height in pixels
    pub fn get_artwork_url_with_tag(&self, item_id: &str, tag: &str, size: u32) -> String {
        let mut url = self.endpoint(&format!("/Items/{}/Images/Primary", item_id));

        url.query_pairs_mut()
            .append_pair("maxWidth", &size.to_string())
            .append_pair("maxHeight", &size.to_string())
            .append_pair("quality", "90")
            .append_pair("tag", tag);

        url.to_string()
    }

    /// Get the backdrop image URL for an item.
    ///
    /// Backdrops are larger images suitable for backgrounds.
    ///
    /// # Arguments
    /// * `item_id` - The Jellyfin item ID
    /// * `index` - Backdrop index (0 for first backdrop)
    /// * `size` - Maximum width in pixels
    pub fn get_backdrop_url(&self, item_id: &str, index: u32, size: u32) -> String {
        let mut url = self.endpoint(&format!("/Items/{}/Images/Backdrop/{}", item_id, index));

        url.query_pairs_mut()
            .append_pair("maxWidth", &size.to_string())
            .append_pair("quality", "80");

        url.to_string()
    }

    /// Get a URL for downloading an audio file via the local proxy.
    ///
    /// # Arguments
    /// * `item_id` - The Jellyfin item ID of the audio track
    /// * `proxy_port` - Port of the local proxy server
    /// * `proxy_token` - Per-session proxy auth token, carried as `?tk=`.
    pub fn get_download_url(&self, item_id: &str, proxy_port: u16, proxy_token: &str) -> String {
        format!(
            "http://127.0.0.1:{}/download/{}?tk={}",
            proxy_port, item_id, proxy_token
        )
    }

    /// Download raw bytes from a URL using the authenticated HTTP client.
    ///
    /// Used to fetch artwork images for local caching.
    pub async fn download_bytes(&self, url: &str) -> Result<bytes::Bytes, crate::error::AppError> {
        use crate::error::AppError;

        let response = self
            .http()
            .get(url)
            .send()
            .await
            .map_err(|e| AppError::Network(e.to_string()))?;

        if !response.status().is_success() {
            // A 401 on this session call is an expiry, not a transient network
            // fault: fire the deduplicated notifier and surface it distinctly
            // so the user is prompted to reconnect.
            if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                self.fire_auth_expired();
                return Err(AppError::AuthExpired(
                    "Jellyfin session expired".to_string(),
                ));
            }
            return Err(AppError::Network(format!(
                "HTTP {} for {}",
                response.status(),
                url
            )));
        }

        response
            .bytes()
            .await
            .map_err(|e| AppError::Network(e.to_string()))
    }

    /// Download raw bytes using the untimed download client, capped at
    /// `max_bytes`.
    ///
    /// Uses the download HTTP client (no total request timeout, so a slow
    /// server-side transcode isn't aborted mid-body) but streams the body with
    /// a hard size cap so a huge or slow-drip response can't exhaust memory —
    /// the plain `download_bytes` has no such bound. Used to fetch the small
    /// transcoded MP3 for waveform precomputation.
    ///
    /// Returns `Ok(None)` when the body exceeds `max_bytes` (caller treats the
    /// track as having no waveform), and `Err` only for genuine network/auth
    /// failures (which the caller surfaces as transient).
    pub async fn download_bytes_capped(
        &self,
        url: &str,
        max_bytes: usize,
    ) -> Result<Option<bytes::Bytes>, crate::error::AppError> {
        use crate::error::AppError;

        let response = self
            .download_http()
            .get(url)
            .send()
            .await
            .map_err(|e| AppError::Network(e.to_string()))?;

        if !response.status().is_success() {
            if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                self.fire_auth_expired();
                return Err(AppError::AuthExpired(
                    "Jellyfin session expired".to_string(),
                ));
            }
            return Err(AppError::Network(format!(
                "HTTP {} for {}",
                response.status(),
                url
            )));
        }

        crate::http::read_capped(response, max_bytes).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> JellyfinClient {
        JellyfinClient::new_with_device_id(
            "https://jellyfin.example.com",
            "test-api-key",
            "test-user-id",
            "test-device-id",
        )
        .unwrap()
    }

    #[test]
    fn test_stream_url_uses_proxy() {
        let client = test_client();
        let url = client.get_stream_url("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4", 18374, "tok123");

        assert_eq!(
            url,
            "http://127.0.0.1:18374/stream/a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4?tk=tok123"
        );
        // The per-session proxy token is embedded as `tk`.
        assert!(url.contains("?tk=tok123"));
        assert!(url.contains("127.0.0.1:18374"));
        // API key must NOT appear in the URL
        assert!(!url.contains("api_key"));
        assert!(!url.contains("test-api-key"));
    }

    #[test]
    fn test_download_url_uses_proxy() {
        let client = test_client();
        let url = client.get_download_url("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4", 18374, "tok123");

        assert_eq!(
            url,
            "http://127.0.0.1:18374/download/a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4?tk=tok123"
        );
        assert!(url.contains("?tk=tok123"));
        assert!(!url.contains("api_key"));
        assert!(!url.contains("test-api-key"));
    }

    #[test]
    fn test_transcoded_stream_url_mp3() {
        let client = test_client();
        let url = client.get_transcoded_stream_url(
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4",
            "mp3",
            "mp3",
            Some(320),
        );

        assert!(url.contains("/Audio/a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4/stream.mp3"));
        assert!(url.contains("audioCodec=mp3"));
        // kbps is converted to bps.
        assert!(url.contains("audioBitRate=320000"));
        // Auth must never leak into the URL.
        assert!(!url.contains("api_key"));
        assert!(!url.contains("test-api-key"));
    }

    #[test]
    fn test_transcoded_stream_url_aac_uses_m4a_container() {
        let client = test_client();
        let url = client.get_transcoded_stream_url(
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4",
            "m4a",
            "aac",
            Some(256),
        );

        assert!(url.contains("/stream.m4a"));
        assert!(url.contains("audioCodec=aac"));
        assert!(url.contains("audioBitRate=256000"));
        assert!(!url.contains("api_key"));
    }

    #[test]
    fn test_transcoded_stream_url_opus() {
        let client = test_client();
        let url = client.get_transcoded_stream_url(
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4",
            "opus",
            "opus",
            Some(128),
        );

        assert!(url.contains("/stream.opus"));
        assert!(url.contains("audioCodec=opus"));
        assert!(url.contains("audioBitRate=128000"));
        assert!(!url.contains("api_key"));
    }

    #[test]
    fn test_transcoded_stream_url_flac_omits_bitrate() {
        let client = test_client();
        let url = client.get_transcoded_stream_url(
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4",
            "flac",
            "flac",
            None,
        );

        assert!(url.contains("/stream.flac"));
        assert!(url.contains("audioCodec=flac"));
        // Lossless: no bitrate parameter at all.
        assert!(!url.contains("audioBitRate"));
        assert!(!url.contains("api_key"));
    }

    #[test]
    fn test_artwork_url_no_api_key() {
        let client = test_client();
        let url = client.get_artwork_url("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4", 300);

        assert!(url.contains("/Items/a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4/Images/Primary"));
        assert!(url.contains("maxWidth=300"));
        assert!(url.contains("maxHeight=300"));
        assert!(url.contains("quality=90"));
        assert!(!url.contains("api_key"));
    }
}
