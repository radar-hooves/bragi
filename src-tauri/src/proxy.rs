//! Local HTTP proxy for secure Jellyfin credential injection.
//!
//! Runs on `127.0.0.1` with an OS-assigned port. Accepts requests for
//! audio streams and downloads, injects the `X-Emby-Token` header, and
//! forwards the response from Jellyfin. This eliminates API keys from
//! URLs entirely (ISM-1504 compliance).
//!
//! # Endpoints
//!
//! - `GET /stream/{item_id}` — Proxies `/Audio/{item_id}/stream?static=true`
//! - `GET /download/{item_id}` — Proxies `/Audio/{item_id}/stream?static=true`
//!   (uses the same endpoint as streaming; the `/Items/{id}/Download` endpoint
//!   returns 502 through many Jellyfin reverse proxy configurations)
//! - `GET /cover/{item_id}?size=` — Proxies cover art (`/rest/getCoverArt.view`
//!   for OpenSubsonic, `/Items/{id}/Images/Primary` for Jellyfin), so the salted
//!   OpenSubsonic auth token never lands in a `<img src>` or the SQLite cache.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use reqwest::header::CONTENT_TYPE;
use tokio::net::TcpListener;
use url::Url;

use crate::downloads::validation::is_valid_item_id;
use crate::mediaserver::subsonic::auth::append_auth;

/// Which backend the proxy forwards to, plus the secrets it injects per request.
///
/// Public because the connection commands and the startup bootstrap construct
/// it. Jellyfin injects an `Authorization: MediaBrowser` header; OpenSubsonic
/// signs the query string with a salted token.
#[derive(Clone)]
pub enum ProxyUpstream {
    /// Jellyfin: inject an `Authorization: MediaBrowser` header.
    Jellyfin { api_key: String, device_id: String },
    /// OpenSubsonic: sign each request's query string via `append_auth`.
    Subsonic { username: String, password: String },
}

/// Shared state for the proxy server.
#[derive(Clone)]
struct ProxyState {
    /// Upstream server base URL (Jellyfin or OpenSubsonic).
    upstream_url: Url,
    /// Backend identity + the credentials injected on every forwarded request.
    upstream: ProxyUpstream,
    /// HTTP client for forwarding requests upstream.
    http: reqwest::Client,
    /// Per-session proxy auth token required on `/stream` and `/download` as the
    /// `tk` query param. Minted per proxy start from a CSPRNG (uuid v4) so a
    /// local process can't blindly hit the loopback listener and have the app
    /// inject its upstream credentials. Never logged.
    token: String,
    /// Fired (deduplicated) when the upstream returns 401 on an audio stream,
    /// so an expired session surfaces as a reconnect prompt.
    auth_expired_cb: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// Handle for managing the proxy server lifetime.
pub struct ProxyHandle {
    /// The port the proxy is listening on.
    port: u16,
    /// Per-session proxy auth token (see [`ProxyState::token`]). Held in memory
    /// only; regenerated on every reconnect/restart, never persisted or logged.
    token: String,
    /// Shutdown signal sender.
    _shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl ProxyHandle {
    /// Get the port the proxy is listening on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get the per-session proxy auth token (the `tk` query param value).
    ///
    /// Must never appear in a log line or any frontend-facing string beyond the
    /// tokenised stream/download URLs handed to mpv/reqwest.
    pub fn token(&self) -> &str {
        &self.token
    }
}

/// Start the local proxy server.
///
/// Binds to `127.0.0.1:0` (OS-assigned port) and returns a handle that
/// provides the assigned port and proxy URL builders. The server runs
/// until the handle is dropped.
///
/// # Arguments
/// * `upstream_url` — Base URL of the upstream media server
/// * `upstream` — Backend identity + credentials to inject on each request
/// * `auth_expired_cb` — Deduplicated notifier fired when an audio stream 401s
pub async fn start_proxy(
    upstream_url: &str,
    upstream: ProxyUpstream,
    auth_expired_cb: Option<Arc<dyn Fn() + Send + Sync>>,
) -> Result<ProxyHandle, String> {
    let base_url =
        Url::parse(upstream_url).map_err(|e| format!("Invalid upstream URL for proxy: {}", e))?;

    // No total-request timeout: a long lossless file on a slow link can
    // legitimately stream for longer than any fixed budget. Connection
    // setup stays bounded.
    let http = reqwest::Client::builder()
        .user_agent(format!("Bragi/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create proxy HTTP client: {}", e))?;

    // Mint a per-session proxy auth token from a CSPRNG (uuid v4 → 32 lowercase
    // hex chars, 122 bits of entropy). Required on every /stream and /download
    // request so a local process can't turn the loopback listener into a
    // credential-injection oracle. Deliberately NOT `fastrand`.
    let token = uuid::Uuid::new_v4().simple().to_string();

    let state = Arc::new(ProxyState {
        upstream_url: base_url,
        upstream,
        http,
        token: token.clone(),
        auth_expired_cb,
    });

    let app = Router::new()
        .route("/stream/{item_id}", get(handle_stream))
        .route("/download/{item_id}", get(handle_download))
        .route("/cover/{item_id}", get(handle_cover))
        .with_state(state);

    // Bind to localhost with OS-assigned port
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed to bind proxy listener: {}", e))?;

    let addr = listener
        .local_addr()
        .map_err(|e| format!("Failed to get proxy address: {}", e))?;

    let port = addr.port();
    tracing::info!("Local proxy listening on 127.0.0.1:{}", port);

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap_or_else(|e| {
                tracing::error!("Proxy server error: {}", e);
            });
        tracing::info!("Local proxy shut down");
    });

    Ok(ProxyHandle {
        port,
        token,
        _shutdown_tx: shutdown_tx,
    })
}

/// Redact a stream/download URL for logging.
///
/// Returns only the portion before the query string, so the per-session proxy
/// token (`?tk=<secret>`) never lands in a tracing line or the persisted audit
/// log. Safe for `file://` URLs (no query) and non-proxy URLs alike.
pub(crate) fn redact_stream_url(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

/// The `tk` query parameter carrying the per-session proxy auth token.
#[derive(serde::Deserialize)]
struct ProxyAuth {
    tk: Option<String>,
}

/// Gate a proxy request on the per-session auth token.
///
/// Returns `403 FORBIDDEN` when the `tk` query param is missing or does not
/// match the token minted at proxy start. The tokens are fixed-length, 122-bit
/// CSPRNG secrets, so a plain `==` comparison is acceptable (there is no
/// low-entropy prefix for a timing side-channel to walk). The token value is
/// never logged by the caller.
fn check_proxy_token(provided: Option<&str>, expected: &str) -> Result<(), StatusCode> {
    match provided {
        Some(tk) if tk == expected => Ok(()),
        _ => Err(StatusCode::FORBIDDEN),
    }
}

/// Validate an opaque media item ID (see [`is_valid_item_id`]).
fn validate_item_id(id: &str) -> Result<(), StatusCode> {
    if is_valid_item_id(id) {
        Ok(())
    } else {
        Err(StatusCode::BAD_REQUEST)
    }
}

/// Which upstream endpoint a request maps to (Subsonic distinguishes them;
/// Jellyfin uses the same static-stream endpoint for both).
#[derive(Clone, Copy)]
enum UpstreamMode {
    Stream,
    Download,
}

/// Build the authenticated upstream request for `item_id`, per backend.
///
/// The Jellyfin arm is byte-identical to the original (the `/Audio/{id}/stream`
/// endpoint with a `static=true` query and the `Authorization: MediaBrowser`
/// header, for both stream and download — Jellyfin ignores `mode`, and the
/// `/Items/{id}/Download` 502 workaround is preserved). The Subsonic arm hits
/// `/rest/{stream|download}.view` with salted-token query auth.
fn build_upstream_request(
    state: &ProxyState,
    item_id: &str,
    mode: UpstreamMode,
) -> Result<reqwest::RequestBuilder, StatusCode> {
    match &state.upstream {
        ProxyUpstream::Jellyfin { api_key, device_id } => {
            let mut url = state.upstream_url.clone();
            url.set_path(&format!("/Audio/{}/stream", item_id));
            let version = env!("CARGO_PKG_VERSION");
            let auth_value = format!(
                "MediaBrowser Token=\"{}\", Client=\"Bragi\", Device=\"Desktop\", DeviceId=\"{}\", Version=\"{}\"",
                api_key, device_id, version,
            );
            Ok(state
                .http
                .get(url)
                .query(&[("static", "true")])
                .header("Authorization", &auth_value))
        }
        ProxyUpstream::Subsonic { username, password } => {
            let view = match mode {
                UpstreamMode::Stream => "stream",
                UpstreamMode::Download => "download",
            };
            let mut url = state.upstream_url.clone();
            url.set_path(&format!("/rest/{}.view", view));
            {
                let mut q = url.query_pairs_mut();
                q.append_pair("id", item_id);
                // Force the original file for playback (Range/seeking) rather than
                // the server's default transcode — analogous to Jellyfin's
                // static=true. download.view already returns the original.
                if matches!(mode, UpstreamMode::Stream) {
                    q.append_pair("format", "raw");
                }
            }
            append_auth(&mut url, username, password);
            Ok(state.http.get(url))
        }
    }
}

/// Cover-art query parameters (`?size=`).
///
/// `size` is kept as a raw string so a non-numeric value is *ignored* (falls
/// back to the default) rather than 400-ing the whole request — see
/// [`parse_cover_size`].
#[derive(serde::Deserialize)]
struct CoverParams {
    size: Option<String>,
}

/// Smallest cover dimension the proxy will request upstream (px).
const COVER_SIZE_MIN: u32 = 1;
/// Largest cover dimension the proxy will request upstream (px). Guards against
/// a caller coaxing the server into rendering an enormous image.
const COVER_SIZE_MAX: u32 = 2000;
/// Default cover dimension when `size` is absent or unparseable (px).
const COVER_SIZE_DEFAULT: u32 = 300;

/// Parse and clamp the `size` query param: non-numeric → default, out-of-range
/// → clamped to `[COVER_SIZE_MIN, COVER_SIZE_MAX]`.
fn parse_cover_size(raw: Option<&str>) -> u32 {
    match raw.and_then(|s| s.parse::<u32>().ok()) {
        Some(n) => n.clamp(COVER_SIZE_MIN, COVER_SIZE_MAX),
        None => COVER_SIZE_DEFAULT,
    }
}

/// Build the upstream cover-art URL for `item_id`, per backend.
///
/// Subsonic → `/rest/getCoverArt.view?id=&size=` signed with `append_auth`
/// (auth rides in the query). Jellyfin → `/Items/{id}/Images/Primary?maxWidth=&
/// maxHeight=&quality=90` (auth rides in the `Authorization` header added by
/// [`build_cover_request`], not the URL).
fn build_cover_url(upstream: &ProxyUpstream, base: &Url, item_id: &str, size: u32) -> Url {
    match upstream {
        ProxyUpstream::Jellyfin { .. } => {
            let mut url = base.clone();
            url.set_path(&format!("/Items/{}/Images/Primary", item_id));
            url.query_pairs_mut()
                .append_pair("maxWidth", &size.to_string())
                .append_pair("maxHeight", &size.to_string())
                .append_pair("quality", "90");
            url
        }
        ProxyUpstream::Subsonic { username, password } => {
            let mut url = base.clone();
            url.set_path("/rest/getCoverArt.view");
            url.query_pairs_mut()
                .append_pair("id", item_id)
                .append_pair("size", &size.to_string());
            append_auth(&mut url, username, password);
            url
        }
    }
}

/// Build the authenticated cover-art request for `item_id`, per backend.
fn build_cover_request(state: &ProxyState, item_id: &str, size: u32) -> reqwest::RequestBuilder {
    let url = build_cover_url(&state.upstream, &state.upstream_url, item_id, size);
    match &state.upstream {
        ProxyUpstream::Jellyfin { api_key, device_id } => {
            let version = env!("CARGO_PKG_VERSION");
            let auth_value = format!(
                "MediaBrowser Token=\"{}\", Client=\"Bragi\", Device=\"Desktop\", DeviceId=\"{}\", Version=\"{}\"",
                api_key, device_id, version,
            );
            state.http.get(url).header("Authorization", &auth_value)
        }
        // Subsonic auth is already baked into the query string.
        ProxyUpstream::Subsonic { .. } => state.http.get(url),
    }
}

/// Forward upstream response headers that are safe to pass through.
fn forward_response_headers(upstream: &reqwest::Response) -> HeaderMap {
    let mut headers = HeaderMap::new();

    if let Some(ct) = upstream.headers().get(CONTENT_TYPE) {
        if let Ok(v) = HeaderValue::from_bytes(ct.as_bytes()) {
            headers.insert(CONTENT_TYPE, v);
        }
    }

    if let Some(cl) = upstream.headers().get("content-length") {
        if let Ok(v) = HeaderValue::from_bytes(cl.as_bytes()) {
            headers.insert("content-length", v);
        }
    }

    // Pass range support through exactly as the upstream reports it.
    // Advertising it unconditionally makes mpv issue ranged tail-prefetches
    // the upstream then answers with 200/full-body, wedging the near-end
    // seek transition (reproduced by the soak spec).
    if let Some(ar) = upstream.headers().get("accept-ranges") {
        if let Ok(v) = HeaderValue::from_bytes(ar.as_bytes()) {
            headers.insert("accept-ranges", v);
        }
    }

    // Forward Content-Range for partial content responses (206)
    if let Some(cr) = upstream.headers().get("content-range") {
        if let Ok(v) = HeaderValue::from_bytes(cr.as_bytes()) {
            headers.insert("content-range", v);
        }
    }

    headers
}

/// Handle a stream request by proxying to Jellyfin.
async fn handle_stream(
    State(state): State<Arc<ProxyState>>,
    Path(item_id): Path<String>,
    Query(auth): Query<ProxyAuth>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    let cid = crate::audit::correlation_id();

    // Reject before any upstream request is built so no credential is injected
    // for an unauthenticated caller. Never log the token value itself.
    if let Err(status) = check_proxy_token(auth.tk.as_deref(), &state.token) {
        crate::audit::log_validation_failure(
            "proxy",
            "token",
            "missing or invalid proxy token for stream request",
            &cid,
        );
        return Err(status);
    }

    if let Err(status) = validate_item_id(&item_id) {
        crate::audit::log_validation_failure(
            "proxy",
            "item_id",
            "invalid item ID for stream request",
            &cid,
        );
        return Err(status);
    }

    let mut request = build_upstream_request(&state, &item_id, UpstreamMode::Stream)?;

    // Forward Range header for seeking support (required by OGG/Opus demuxers)
    if let Some(range) = headers.get(header::RANGE) {
        if let Ok(range_str) = range.to_str() {
            request = request.header("Range", range_str);
            tracing::debug!("[{}] Forwarding Range header: {}", cid, range_str);
        }
    }

    tracing::debug!("[{}] Proxy stream request for item {}", cid, item_id);

    let upstream = request.send().await.map_err(|e| {
        // Strip the URL: a Subsonic upstream URL carries the salted auth token,
        // which must never land in a log line (review).
        let e = e.without_url();
        // Provide detailed error categorisation for debugging.
        let error_detail = if e.is_connect() {
            format!("connection failed: {}", e)
        } else if e.is_timeout() {
            format!("request timeout: {}", e)
        } else if e.is_request() {
            format!("request error: {}", e)
        } else {
            format!("unknown error: {}", e)
        };
        tracing::error!(
            "[{}] Proxy upstream error for stream {}: {}",
            cid,
            item_id,
            error_detail
        );
        StatusCode::BAD_GATEWAY
    })?;

    // Accept both 200 OK and 206 Partial Content as success
    let upstream_status = upstream.status();
    if !upstream_status.is_success() {
        tracing::warn!(
            "[{}] Upstream returned {} for stream {} (server: {})",
            cid,
            upstream_status,
            item_id,
            state.upstream_url
        );
        // A 401 while streaming means the session expired — surface it as a
        // deduplicated reconnect prompt rather than a silent
        // playback failure. The status is still forwarded to mpv unchanged.
        if upstream_status == StatusCode::UNAUTHORIZED {
            if let Some(cb) = &state.auth_expired_cb {
                cb();
            }
        }
        return Err(
            StatusCode::from_u16(upstream_status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY)
        );
    }

    // Log content details for debugging.
    let content_length = upstream
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());
    let content_type = upstream
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    tracing::debug!(
        "[{}] Streaming {} (type={}, length={:?}, status={})",
        cid,
        item_id,
        content_type,
        content_length,
        upstream_status
    );

    let response_headers = forward_response_headers(&upstream);
    let body = axum::body::Body::from_stream(upstream.bytes_stream());

    // Return the same status code as upstream (200 or 206)
    let status = StatusCode::from_u16(upstream_status.as_u16()).unwrap_or(StatusCode::OK);
    Ok((status, response_headers, body))
}

/// Handle a download request by proxying to Jellyfin.
///
/// Uses `/Audio/{id}/stream?static=true` rather than `/Items/{id}/Download`
/// because the Download endpoint returns 502 through many Jellyfin reverse
/// proxy setups. The static stream endpoint returns the original file
/// without transcoding — identical content, more reliable delivery.
async fn handle_download(
    State(state): State<Arc<ProxyState>>,
    Path(item_id): Path<String>,
    Query(auth): Query<ProxyAuth>,
) -> Result<impl IntoResponse, StatusCode> {
    let cid = crate::audit::correlation_id();

    // Same per-session token gate as the stream path: reject before any
    // upstream credential injection. Token value is never logged.
    if let Err(status) = check_proxy_token(auth.tk.as_deref(), &state.token) {
        crate::audit::log_validation_failure(
            "proxy",
            "token",
            "missing or invalid proxy token for download request",
            &cid,
        );
        return Err(status);
    }

    if let Err(status) = validate_item_id(&item_id) {
        crate::audit::log_validation_failure(
            "proxy",
            "item_id",
            "invalid item ID for download request",
            &cid,
        );
        return Err(status);
    }

    // Jellyfin uses the Audio stream endpoint with static=true to get the
    // original file (more reliable than /Items/{id}/Download, which 502s through
    // many reverse proxies); Subsonic uses /rest/download.view.
    let request = build_upstream_request(&state, &item_id, UpstreamMode::Download)?;

    let upstream = request.send().await.map_err(|e| {
        // Strip the URL (carries the Subsonic token) before logging (review).
        tracing::error!(
            "Proxy upstream error for download {}: {}",
            item_id,
            e.without_url()
        );
        StatusCode::BAD_GATEWAY
    })?;

    if !upstream.status().is_success() {
        tracing::warn!(
            "Jellyfin returned {} for download {}",
            upstream.status(),
            item_id
        );
        // Same session-expiry surfacing as handle_stream.
        if upstream.status() == StatusCode::UNAUTHORIZED {
            if let Some(cb) = &state.auth_expired_cb {
                cb();
            }
        }
        return Err(
            StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY)
        );
    }

    let response_headers = forward_response_headers(&upstream);
    let body = axum::body::Body::from_stream(upstream.bytes_stream());

    Ok((response_headers, body))
}

/// Handle a cover-art request by proxying to the active backend.
///
/// Injects auth server-side (Subsonic query token / Jellyfin header) so no
/// credential ever appears in the `<img src>` or the cached DB row. The upstream
/// Content-Type is forwarded verbatim (getCoverArt may return jpeg/png/webp) and
/// the response is marked immutable + CORS-open for the WebView.
async fn handle_cover(
    State(state): State<Arc<ProxyState>>,
    Path(item_id): Path<String>,
    Query(params): Query<CoverParams>,
) -> Result<impl IntoResponse, StatusCode> {
    let cid = crate::audit::correlation_id();

    if let Err(status) = validate_item_id(&item_id) {
        crate::audit::log_validation_failure(
            "proxy",
            "item_id",
            "invalid item ID for cover request",
            &cid,
        );
        return Err(status);
    }

    let size = parse_cover_size(params.size.as_deref());
    let request = build_cover_request(&state, &item_id, size);

    let upstream = request.send().await.map_err(|e| {
        // Strip the URL: a Subsonic cover URL carries the salted auth token,
        // which must never land in a log line (review).
        tracing::error!(
            "[{}] Proxy upstream error for cover {}: {}",
            cid,
            item_id,
            e.without_url()
        );
        StatusCode::BAD_GATEWAY
    })?;

    let upstream_status = upstream.status();
    if !upstream_status.is_success() {
        tracing::warn!(
            "[{}] Upstream returned {} for cover {}",
            cid,
            upstream_status,
            item_id
        );
        // A 401 means the session expired — surface it as a deduplicated
        // reconnect prompt, exactly like the stream path.
        if upstream_status == StatusCode::UNAUTHORIZED {
            if let Some(cb) = &state.auth_expired_cb {
                cb();
            }
        }
        return Err(
            StatusCode::from_u16(upstream_status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY)
        );
    }

    // Forward the upstream Content-Type verbatim (jpeg/png/webp all valid).
    let mut headers = HeaderMap::new();
    if let Some(ct) = upstream.headers().get(CONTENT_TYPE) {
        if let Ok(v) = HeaderValue::from_bytes(ct.as_bytes()) {
            headers.insert(CONTENT_TYPE, v);
        }
    }
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("max-age=31536000, immutable"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );

    let body = axum::body::Body::from_stream(upstream.bytes_stream());
    Ok((headers, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_item_id_valid_jellyfin_hex() {
        assert!(validate_item_id("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4").is_ok());
    }

    #[test]
    fn test_validate_item_id_accepts_subsonic_ids() {
        // Navidrome base62 id + coverArt id (contains '-' and '_').
        assert!(validate_item_id("2cahnhu6UPen2wbYDm4CHK").is_ok());
        assert!(validate_item_id("al-2cahnhu6UPen2wbYDm4CHK_640a93a8").is_ok());
    }

    #[test]
    fn test_validate_item_id_rejects_traversal_and_overlong() {
        // Path-injection guard: '.', '/', '\' are rejected.
        assert!(validate_item_id("a/b").is_err());
        assert!(validate_item_id("a\\b").is_err());
        assert!(validate_item_id("a.b").is_err());
        assert!(validate_item_id("..").is_err());
        // Length cap.
        assert!(validate_item_id(&"a".repeat(65)).is_err());
    }

    #[test]
    fn test_validate_item_id_empty() {
        assert!(validate_item_id("").is_err());
    }

    #[test]
    fn test_check_proxy_token() {
        // Correct token passes the gate.
        assert!(check_proxy_token(Some("secret-token"), "secret-token").is_ok());
        // A wrong token is forbidden.
        assert_eq!(
            check_proxy_token(Some("wrong"), "secret-token"),
            Err(StatusCode::FORBIDDEN)
        );
        // A missing token is forbidden.
        assert_eq!(
            check_proxy_token(None, "secret-token"),
            Err(StatusCode::FORBIDDEN)
        );
        // An empty provided token never matches a non-empty expected token.
        assert_eq!(
            check_proxy_token(Some(""), "secret-token"),
            Err(StatusCode::FORBIDDEN)
        );
    }

    #[tokio::test]
    async fn test_start_proxy_mints_unique_hex_tokens() {
        let mk = || {
            start_proxy(
                "https://music.example.com",
                ProxyUpstream::Subsonic {
                    username: "demo".to_string(),
                    password: "sesame".to_string(),
                },
                None,
            )
        };
        let a = mk().await.expect("proxy a should start");
        let b = mk().await.expect("proxy b should start");

        // 32 lowercase hex chars, non-empty.
        assert_eq!(a.token().len(), 32, "uuid simple form is 32 hex chars");
        assert!(!a.token().is_empty());
        assert!(
            a.token()
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "token must be lowercase hex: {}",
            a.token()
        );

        // Two proxy starts yield different tokens (per-session CSPRNG).
        assert_ne!(a.token(), b.token(), "each proxy start mints a fresh token");

        // The tokenised stream URL carries the token and its id still validates.
        let url = format!(
            "http://127.0.0.1:{}/stream/a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4?tk={}",
            a.port(),
            a.token()
        );
        assert!(url.contains(&format!("?tk={}", a.token())));
        assert!(validate_item_id("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4").is_ok());
    }

    #[test]
    fn test_redact_stream_url_strips_token_query() {
        // A tokenised proxy URL: the `?tk=` secret must not survive redaction.
        let (tx, _rx) = tokio::sync::oneshot::channel();
        let handle = ProxyHandle {
            port: 12345,
            token: "deadbeefdeadbeefdeadbeefdeadbeef".to_string(),
            _shutdown_tx: tx,
        };
        let url = format!(
            "http://127.0.0.1:{}/stream/a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4?tk={}",
            handle.port(),
            handle.token()
        );
        let redacted = redact_stream_url(&url);
        assert!(
            !redacted.contains("tk="),
            "redacted URL still leaks the token: {redacted}"
        );
        assert!(
            !redacted.contains(handle.token()),
            "redacted URL still contains the token value: {redacted}"
        );
        assert_eq!(
            redacted,
            "http://127.0.0.1:12345/stream/a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4"
        );

        // A cached file:// URL has no query string and is returned unchanged.
        assert_eq!(
            redact_stream_url("file:///home/user/song.flac"),
            "file:///home/user/song.flac"
        );
    }

    #[test]
    fn test_parse_cover_size_clamps_and_defaults() {
        // Absent / non-numeric → default.
        assert_eq!(parse_cover_size(None), COVER_SIZE_DEFAULT);
        assert_eq!(parse_cover_size(Some("abc")), COVER_SIZE_DEFAULT);
        assert_eq!(parse_cover_size(Some("")), COVER_SIZE_DEFAULT);
        // In-range passes through.
        assert_eq!(parse_cover_size(Some("640")), 640);
        // Out-of-range clamps to the bounds.
        assert_eq!(parse_cover_size(Some("0")), COVER_SIZE_MIN);
        assert_eq!(parse_cover_size(Some("99999")), COVER_SIZE_MAX);
    }

    #[test]
    fn test_build_cover_url_subsonic_signs_query_without_leaking_password() {
        let base = Url::parse("https://music.example.com").unwrap();
        let upstream = ProxyUpstream::Subsonic {
            username: "demo".to_string(),
            password: "sesame".to_string(),
        };
        let url = build_cover_url(&upstream, &base, "al-9", 640);

        assert_eq!(url.path(), "/rest/getCoverArt.view");
        let q = url.query().unwrap();
        assert!(q.contains("id=al-9"), "missing id in {q}");
        assert!(q.contains("size=640"), "missing size in {q}");
        // Auth params present …
        assert!(q.contains("u=demo"), "missing u in {q}");
        assert!(q.contains("t="), "missing token in {q}");
        assert!(q.contains("s="), "missing salt in {q}");
        // … but the raw password never appears anywhere in the URL.
        assert!(
            !url.as_str().contains("sesame"),
            "raw password leaked into {url}"
        );
    }

    #[test]
    fn test_build_cover_url_jellyfin_builds_primary_image() {
        let base = Url::parse("https://jellyfin.example.com").unwrap();
        let upstream = ProxyUpstream::Jellyfin {
            api_key: "secret-api-key".to_string(),
            device_id: "device-1".to_string(),
        };
        let url = build_cover_url(&upstream, &base, "abc123", 300);

        assert_eq!(url.path(), "/Items/abc123/Images/Primary");
        let q = url.query().unwrap();
        assert!(q.contains("maxWidth=300"), "missing maxWidth in {q}");
        assert!(q.contains("maxHeight=300"), "missing maxHeight in {q}");
        assert!(q.contains("quality=90"), "missing quality in {q}");
        // The Jellyfin api_key rides in the Authorization header, never the URL.
        assert!(
            !url.as_str().contains("secret-api-key"),
            "api_key leaked into {url}"
        );
    }
}
