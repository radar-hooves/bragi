//! OpenSubsonic streaming / download / artwork URLs.
//!
//! Playback and downloads go through Bragi's local proxy (so the auth token
//! isn't handed to mpv or `<img>`), exactly like the Jellyfin path — the proxy
//! rewrites `/stream/{id}` and `/download/{id}` to signed
//! `/rest/stream.view` / `/rest/download.view` calls. Direct authenticated URLs
//! (transcode, artwork bytes) are hit with the client's own HTTP client, which
//! carries the auth in the query string.

use super::auth;
use super::client::SubsonicClient;

impl SubsonicClient {
    /// Proxy stream URL for a track (the proxy signs the upstream call).
    ///
    /// `proxy_token` is the per-session proxy auth token, carried as `?tk=` so
    /// the loopback proxy accepts the request.
    pub fn get_stream_url(&self, item_id: &str, proxy_port: u16, proxy_token: &str) -> String {
        format!("http://127.0.0.1:{proxy_port}/stream/{item_id}?tk={proxy_token}")
    }

    /// Proxy download URL for a track (carries the `?tk=` proxy token).
    pub fn get_download_url(&self, item_id: &str, proxy_port: u16, proxy_token: &str) -> String {
        format!("http://127.0.0.1:{proxy_port}/download/{item_id}?tk={proxy_token}")
    }

    /// A direct, authenticated transcode URL (`stream.view?format=&maxBitRate=`).
    ///
    /// Hit with the authenticated client (auth is in the query), mirroring the
    /// Jellyfin transcode path used by offline downloads and waveform peaks.
    /// `container`/`audio_codec` map to Subsonic's `format`; `bitrate` (kbps) to
    /// `maxBitRate`.
    pub fn get_transcoded_stream_url(
        &self,
        item_id: &str,
        _container: &str,
        audio_codec: &str,
        bitrate: Option<u32>,
    ) -> String {
        let mut url = self.rest_endpoint("stream");
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("id", item_id);
            // Subsonic `format` is a CODEC token (mp3/opus/aac/flac), not a
            // container — so AAC must be `aac`, never `m4a` (review).
            q.append_pair("format", audio_codec);
            if let Some(kbps) = bitrate {
                q.append_pair("maxBitRate", &kbps.to_string());
            }
        }
        auth::append_auth(&mut url, self.username(), self.password());
        url.to_string()
    }

    /// Fetch cover-art bytes for the internal disk-cache warm.
    ///
    /// Builds the authenticated `getCoverArt` URL server-side and downloads the
    /// bytes with the client's own HTTP client, so the salted token never leaves
    /// the backend. Navidrome accepts an album/artist/song id as well as a bare
    /// coverArt id, so the caller may pass the item id directly.
    pub async fn fetch_artwork_bytes(
        &self,
        item_id: &str,
        size: u32,
    ) -> Result<bytes::Bytes, crate::error::AppError> {
        let url = self.authed_cover_url(item_id, Some(size));
        self.download_bytes(&url).await
    }

    /// Download raw bytes from an authenticated URL (auth already in the query).
    pub async fn download_bytes(&self, url: &str) -> Result<bytes::Bytes, crate::error::AppError> {
        use crate::error::AppError;
        let response = self
            .http()
            .get(url)
            .send()
            .await
            .map_err(|e| AppError::Network(e.without_url().to_string()))?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.fire_auth_expired();
            return Err(AppError::AuthExpired(
                "Subsonic session expired".to_string(),
            ));
        }
        if !response.status().is_success() {
            // Don't include the URL: it carries the Subsonic token (review).
            return Err(AppError::Network(format!("HTTP {}", response.status())));
        }
        response
            .bytes()
            .await
            .map_err(|e| AppError::Network(e.without_url().to_string()))
    }

    /// Download raw bytes with a hard size cap (mirrors the Jellyfin waveform
    /// fetch): `Ok(None)` when the body exceeds `max_bytes`, `Err` on transient
    /// network/auth failure.
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
            .map_err(|e| AppError::Network(e.without_url().to_string()))?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.fire_auth_expired();
            return Err(AppError::AuthExpired(
                "Subsonic session expired".to_string(),
            ));
        }
        if !response.status().is_success() {
            // Don't include the URL: it carries the Subsonic token (review).
            return Err(AppError::Network(format!("HTTP {}", response.status())));
        }
        crate::http::read_capped(response, max_bytes).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> SubsonicClient {
        SubsonicClient::new("https://music.example.com", "demo", "sesame").unwrap()
    }

    #[test]
    fn stream_url_embeds_proxy_token_on_loopback() {
        let client = test_client();
        let url = client.get_stream_url("2cahnhu6UPen2wbYDm4CHK", 45321, "tok123");
        assert_eq!(
            url,
            "http://127.0.0.1:45321/stream/2cahnhu6UPen2wbYDm4CHK?tk=tok123"
        );
        // Loopback host/port unchanged, token present, no credential leaked.
        assert!(url.starts_with("http://127.0.0.1:45321/"));
        assert!(url.ends_with("?tk=tok123"));
        assert!(!url.contains("sesame"));
    }

    #[test]
    fn download_url_embeds_proxy_token_on_loopback() {
        let client = test_client();
        let url = client.get_download_url("2cahnhu6UPen2wbYDm4CHK", 45321, "tok123");
        assert_eq!(
            url,
            "http://127.0.0.1:45321/download/2cahnhu6UPen2wbYDm4CHK?tk=tok123"
        );
        assert!(url.contains("?tk=tok123"));
        assert!(!url.contains("sesame"));
    }
}
