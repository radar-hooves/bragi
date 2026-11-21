//! Music service API client (core, auth, scrobble).
//!
//! Sync endpoints in `client_sync`.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::HeaderValue;
use reqwest_middleware::ClientWithMiddleware;
use url::Url;

use super::credentials::MusicServiceCredentials;
use super::models::{ScrobbleRequest, SkipRequest, SkipResponse, SyncResponse};
use crate::error::AppError;

/// Request timeout for API calls.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Render a request error with its whole source chain.
///
/// `Display` on a `reqwest::Error` stops at the outermost layer, so a
/// transport failure logs only "error sending request for url (…)" — which
/// names neither the DNS, TLS nor connect fault underneath it, and leaves a
/// failed scrobble sync undiagnosable from the log alone. Keep the chain.
/// Stays on stderr: the audit target is the only thing that leaves the device.
///
/// Takes `&dyn Error` because a request failure and a body-decode failure are
/// different types once the client carries middleware.
pub(super) fn transport_detail(error: &dyn std::error::Error) -> String {
    let mut detail = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        detail.push_str(": ");
        detail.push_str(&cause.to_string());
        source = cause.source();
    }
    detail
}

/// Aggregated outcome of a bulk scrobble sync via `POST /client/sync`.
///
/// `rejected_no_mbid` is a defensive-net signal only: the primary hold
/// mechanism lives in `commands::scrobble::sync_all_inner`, which partitions
/// mbid-less scrobbles out of the batch before it is ever built, so this
/// should read zero in normal operation. A nonzero value means the scrobble server
/// rejected something Bragi believed was safe to send.
#[derive(Debug, Default)]
pub struct ScrobbleSyncOutcome {
    /// Scrobbles the scrobble server stored (new + duplicate).
    pub synced: u32,
    /// Scrobbles the scrobble server refused because they carried no MusicBrainz
    /// recording ID (the scrobble server rejects rather than
    /// silently storing these).
    pub rejected_no_mbid: u32,
    /// Positions in the input slice the service accepted. The caller marks
    /// exactly these synced, whether or not a later chunk failed.
    pub drained: Vec<usize>,
    /// Positions the service refused with a 4xx on their own. Retrying costs
    /// a request but never blocks the plays beside them.
    pub rejected: Vec<usize>,
    /// What stopped the drain (auth, transport, a 5xx), if anything. Every
    /// position neither drained nor rejected is still owed.
    pub error: Option<AppError>,
}

impl ScrobbleSyncOutcome {
    /// Fold one chunk's result in. False when the drain must stop.
    fn record(&mut self, positions: &[usize], result: ChunkResult) -> bool {
        match result {
            ChunkResult::Accepted {
                synced,
                rejected_no_mbid,
            } => {
                self.synced += synced;
                self.rejected_no_mbid += rejected_no_mbid;
                self.drained.extend_from_slice(positions);
                true
            }
            ChunkResult::Rejected { status, .. } => {
                tracing::warn!(
                    "Scrobble service refused {} play(s) with {}; left queued, isolated",
                    positions.len(),
                    status
                );
                self.rejected.extend_from_slice(positions);
                true
            }
            ChunkResult::Failed(error) => {
                self.error = Some(error);
                false
            }
        }
    }
}

/// How one chunk fared.
enum ChunkResult {
    Accepted {
        synced: u32,
        rejected_no_mbid: u32,
    },
    /// A 4xx other than 401, 408 and 429: the service refused this payload.
    /// `via_lb` marks a refusal from the ListenBrainz route a native 404/405
    /// fell back to, so a per-play resend goes straight there.
    Rejected {
        status: reqwest::StatusCode,
        via_lb: bool,
    },
    Failed(AppError),
}

/// A non-success, non-401 reply: a refused payload only for a 4xx that
/// resending cannot fix. 408 and 429 are the service's state, not the play's,
/// so they stop the drain as a 5xx does.
fn refusal(status: reqwest::StatusCode, route: &str) -> ChunkResult {
    use reqwest::StatusCode;
    if status.is_client_error()
        && status != StatusCode::REQUEST_TIMEOUT
        && status != StatusCode::TOO_MANY_REQUESTS
    {
        ChunkResult::Rejected {
            status,
            via_lb: false,
        }
    } else {
        ChunkResult::Failed(AppError::Network(format!(
            "{route} failed with status {status}"
        )))
    }
}

/// Music service API client.
///
/// Handles authentication and HTTP communication with a self-hosted scrobble
/// server. The HTTP client is wrapped in Arc for cheap cloning.
///
/// Speaks the server's native client API (`/client/*`: sync, skips, stats)
/// for scrobbling, and the ListenBrainz wire format, kept on the model
/// structs, for any other ListenBrainz-compatible service.
pub struct MusicServiceClient {
    pub(super) base_url: Url,
    pub(super) http: Arc<ClientWithMiddleware>,
}

// Custom Debug implementation to avoid exposing sensitive data
impl fmt::Debug for MusicServiceClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MusicServiceClient")
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl MusicServiceClient {
    /// Create a new music service client.
    ///
    /// # Arguments
    /// * `service_url` - Base URL of the service (e.g., "https://music.example.com").
    ///   The URL is used as-is for health checks. Native client API endpoints use
    ///   `/client/*` prefix.
    /// * `api_token` - API token for authentication (sent as Bearer token)
    ///
    /// # Errors
    /// Returns `AppError::Config` if the URL is invalid.
    pub fn new(service_url: &str, api_token: &str) -> Result<Self, AppError> {
        let mut base_url = Url::parse(service_url).map_err(|e| {
            AppError::Config(format!(
                "Invalid music service URL '{}': {}",
                service_url, e
            ))
        })?;

        // Strip trailing slash for consistent URL joining
        let path = base_url.path().to_string();
        if path.len() > 1 && path.ends_with('/') {
            base_url.set_path(path.trim_end_matches('/'));
        }

        // Use Bearer auth for the native client API
        let mut headers = crate::http::default_headers();
        headers.insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", api_token))
                .map_err(|_| AppError::Config("Invalid API token format".to_string()))?,
        );
        headers.insert("Content-Type", HeaderValue::from_static("application/json"));

        Ok(Self {
            base_url,
            http: Arc::new(crate::http::client(headers, REQUEST_TIMEOUT)),
        })
    }

    /// Create a client from credentials struct.
    pub fn from_credentials(credentials: &MusicServiceCredentials) -> Result<Self, AppError> {
        Self::new(&credentials.service_url, &credentials.api_token)
    }

    /// Get the base URL.
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// Build a URL for an API endpoint using a base URL reference.
    ///
    /// Static version for use without holding the client lock.
    pub(super) fn endpoint_from_base(base_url: &Url, path: &str) -> Result<Url, AppError> {
        // Ensure base URL ends with / for correct join behaviour
        let base = if base_url.as_str().ends_with('/') {
            base_url.clone()
        } else {
            Url::parse(&format!("{}/", base_url))
                .map_err(|e| AppError::Config(format!("Invalid base URL: {}", e)))?
        };

        // Remove leading slash from path for join to work correctly
        let path = path.strip_prefix('/').unwrap_or(path);

        base.join(path)
            .map_err(|e| AppError::Config(format!("Failed to build endpoint URL: {}", e)))
    }

    /// Clone the HTTP client and base URL for making requests without holding the lock.
    ///
    /// Returns a tuple of (http_client, base_url) that can be used with the
    /// static `*_with_client` methods.
    pub fn clone_for_request(&self) -> (Arc<ClientWithMiddleware>, Url) {
        (Arc::clone(&self.http), self.base_url.clone())
    }

    /// Test the connection to the music service.
    ///
    /// Makes a simple API call to verify credentials are valid.
    pub async fn test_connection(&self) -> Result<(), AppError> {
        Self::test_connection_with_client(&self.http, &self.base_url).await
    }

    /// Test connection using a cloned client (static version).
    ///
    /// Tries the scrobble server's health endpoint first (`GET /health`).
    /// Falls back to ListenBrainz validate-token (`/1/validate-token`)
    /// for other ListenBrainz-compatible services.
    pub async fn test_connection_with_client(
        http: &ClientWithMiddleware,
        base_url: &Url,
    ) -> Result<(), AppError> {
        // The scrobble server and ListenBrainz both implement /1/validate-token.
        // This checks reachability AND token validity in one request.
        let validate_url = Self::endpoint_from_base(base_url, "1/validate-token")?;

        let response = http.get(validate_url).send().await.map_err(|e| {
            AppError::Network(format!("Connection failed: {}", transport_detail(&e)))
        })?;

        if response.status().is_success() {
            if let Ok(body) = response.json::<serde_json::Value>().await {
                if body["valid"].as_bool() == Some(true) {
                    return Ok(());
                }
                return Err(AppError::Auth("Invalid API token".to_string()));
            }
            Ok(())
        } else if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            Err(AppError::Auth("Invalid API token".to_string()))
        } else {
            Err(AppError::Network(format!(
                "Music service returned status {}",
                response.status()
            )))
        }
    }

    /// Bulk sync multiple scrobbles using a cloned client (static version).
    ///
    /// Use this when you need to release the RwLock before making the HTTP request.
    /// Identified plays go first, in chunks of 50 to the scrobble server's `POST /client/sync`;
    /// a play with no recording id follows by artist and title over the
    /// ListenBrainz route, where the scrobble server mints a stable id for the normalised
    /// triple and other LB services match by name (`/client/sync` refuses it).
    ///
    /// A chunk the service refuses with a 4xx is re-sent one play at a time so
    /// a single poisoned play is isolated; a transport failure, 401 or 5xx
    /// stops the drain. Positions already accepted are reported either way.
    pub async fn sync_scrobbles_with_client(
        http: &ClientWithMiddleware,
        base_url: &Url,
        scrobbles: &[ScrobbleRequest],
    ) -> ScrobbleSyncOutcome {
        // The scrobble server allows max 50 scrobbles per sync request
        const BATCH_SIZE: usize = 50;

        let mut outcome = ScrobbleSyncOutcome::default();
        let (identified, unidentified): (Vec<usize>, Vec<usize>) =
            (0..scrobbles.len()).partition(|&i| scrobbles[i].track_mbid.is_some());

        for (positions, native) in [(identified, true), (unidentified, false)] {
            for chunk in positions.chunks(BATCH_SIZE) {
                let send = |idx: &[usize], native: bool| {
                    let batch: Vec<ScrobbleRequest> =
                        idx.iter().map(|&i| scrobbles[i].clone()).collect();
                    async move {
                        if native {
                            Self::sync_chunk_native(http, base_url, &batch).await
                        } else {
                            Self::sync_chunk_via_lb_import(http, base_url, &batch).await
                        }
                    }
                };
                let result = send(chunk, native).await;
                if let (ChunkResult::Rejected { status, via_lb }, true) = (&result, chunk.len() > 1)
                {
                    // Native already fell back to LB for this chunk: resend
                    // there directly, one request a play rather than two.
                    let native = native && !via_lb;
                    tracing::warn!(
                        "Scrobble chunk of {} refused with {}; sending one at a time",
                        chunk.len(),
                        status
                    );
                    for &i in chunk {
                        if !outcome.record(&[i], send(&[i], native).await) {
                            return outcome;
                        }
                    }
                } else if !outcome.record(chunk, result) {
                    return outcome;
                }
            }
        }
        outcome
    }

    /// One chunk to the scrobble server's native `POST /client/sync`, falling back to the
    /// LB import format where the service is not a server with the native client API.
    async fn sync_chunk_native(
        http: &ClientWithMiddleware,
        base_url: &Url,
        chunk: &[ScrobbleRequest],
    ) -> ChunkResult {
        let url = match Self::endpoint_from_base(base_url, "client/sync") {
            Ok(url) => url,
            Err(e) => return ChunkResult::Failed(e),
        };
        let payload = serde_json::json!({
            "scrobbles": chunk,
            "skips": [],
        });

        let response = match http.post(url).json(&payload).send().await {
            Ok(response) => response,
            Err(e) => {
                return ChunkResult::Failed(AppError::Network(format!(
                    "Failed to sync scrobbles: {}",
                    transport_detail(&e)
                )))
            }
        };
        let status = response.status();

        if status.is_success() {
            match response.json::<SyncResponse>().await {
                Ok(result) => {
                    // The scrobble server's `synced` already counts both newly-recorded and
                    // duplicate scrobbles (a duplicate is drained from the queue's
                    // perspective), so it is the full count of entries this chunk
                    // handled. `duplicates` is a subset of `synced`, reported for
                    // information only — adding it would double-count.
                    if result.rejected_no_mbid > 0 {
                        // Defensive net only: sync_all_inner already holds
                        // mbid-less scrobbles back before this call, so this
                        // should never fire in normal operation.
                        tracing::warn!(
                            "The scrobble server rejected {} scrobble(s) in this batch for missing \
                             MusicBrainz recording ID; Bragi should have held these back \
                             before sending",
                            result.rejected_no_mbid
                        );
                    }
                    tracing::debug!(
                        "Synced batch: {} synced, {} duplicates, {} rejected (no mbid)",
                        result.synced,
                        result.duplicates,
                        result.rejected_no_mbid
                    );
                    ChunkResult::Accepted {
                        synced: result.synced,
                        rejected_no_mbid: result.rejected_no_mbid,
                    }
                }
                Err(e) => ChunkResult::Failed(AppError::Network(format!(
                    "Failed to parse sync response: {}",
                    transport_detail(&e)
                ))),
            }
        } else if status == reqwest::StatusCode::UNAUTHORIZED {
            ChunkResult::Failed(AppError::Auth("Invalid API token".to_string()))
        } else if status == reqwest::StatusCode::NOT_FOUND
            || status == reqwest::StatusCode::METHOD_NOT_ALLOWED
        {
            // Not a server with the native client API (plain ListenBrainz or Maloja):
            // fall back to the LB import format on /1/submit-listens.
            match Self::sync_chunk_via_lb_import(http, base_url, chunk).await {
                ChunkResult::Rejected { status, .. } => ChunkResult::Rejected {
                    status,
                    via_lb: true,
                },
                other => other,
            }
        } else {
            refusal(status, "Sync")
        }
    }

    /// Drain for plays with no recording id, and the fallback for
    /// LB-compatible services without the native `/client/sync`:
    /// submits the chunk as a ListenBrainz `import` to `POST /1/submit-listens`.
    async fn sync_chunk_via_lb_import(
        http: &ClientWithMiddleware,
        base_url: &Url,
        chunk: &[ScrobbleRequest],
    ) -> ChunkResult {
        let url = match Self::endpoint_from_base(base_url, "1/submit-listens") {
            Ok(url) => url,
            Err(e) => return ChunkResult::Failed(e),
        };
        let payload = ScrobbleRequest::to_lb_import(chunk);

        let response = match http.post(url).json(&payload).send().await {
            Ok(response) => response,
            Err(e) => {
                return ChunkResult::Failed(AppError::Network(format!(
                    "Failed to send scrobbles: {}",
                    transport_detail(&e)
                )))
            }
        };
        let status = response.status();

        if status.is_success() {
            tracing::debug!("Synced batch of {} via ListenBrainz import", chunk.len());
            ChunkResult::Accepted {
                synced: chunk.len() as u32,
                rejected_no_mbid: 0,
            }
        } else if status == reqwest::StatusCode::UNAUTHORIZED {
            ChunkResult::Failed(AppError::Auth("Invalid API token".to_string()))
        } else {
            refusal(status, "ListenBrainz import")
        }
    }

    /// Record a skip using a cloned client (static version).
    ///
    /// Uses the native API at `POST /client/skips`.
    /// Returns `Ok(false)` if the service does not support skip recording.
    pub async fn record_skip_with_client(
        http: &ClientWithMiddleware,
        base_url: &Url,
        request: &SkipRequest,
    ) -> Result<bool, AppError> {
        let url = Self::endpoint_from_base(base_url, "client/skips")?;

        let response = http.post(url).json(request).send().await.map_err(|e| {
            AppError::Network(format!("Failed to send skip: {}", transport_detail(&e)))
        })?;

        if response.status().is_success() {
            let result: SkipResponse = response.json().await.map_err(|e| {
                AppError::Network(format!(
                    "Failed to parse response: {}",
                    transport_detail(&e)
                ))
            })?;

            Ok(result.success)
        } else if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            Err(AppError::Auth("Invalid API token".to_string()))
        } else if response.status() == reqwest::StatusCode::NOT_FOUND
            || response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED
        {
            // Not a server with the native client API: the service has no skip endpoint.
            Ok(false)
        } else if response.status().is_client_error() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            Err(AppError::InvalidOperation(format!(
                "Skip rejected: {}",
                error_text
            )))
        } else {
            Err(AppError::Network(format!(
                "Skip failed with status {}",
                response.status()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A play with no recording id goes by name over the ListenBrainz route,
    /// after the identified ones; `/client/sync` would refuse it.
    #[tokio::test]
    async fn id_less_plays_go_by_name_over_the_listenbrainz_route() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut paths = Vec::new();
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 8192];
                let n = socket.read(&mut buf).await.unwrap();
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                paths.push(request.split_whitespace().nth(1).unwrap().to_string());
                let body = r#"{"synced":1,"duplicates":0,"skips_synced":0,"skips_duplicates":0}"#;
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                socket.write_all(reply.as_bytes()).await.unwrap();
            }
            paths
        });

        let request = |mbid: Option<&str>| ScrobbleRequest {
            track_mbid: mbid.map(str::to_string),
            artist_name: "Megadeth".into(),
            track_name: "Live".into(),
            album_name: None,
            listened_at: "2023-03-26T00:00:00+00:00".into(),
            duration_ms: 1000,
            device_id: "d".into(),
            jellyfin_id: None,
            artist_mbid: None,
            release_mbid: None,
        };
        let http = crate::http::client(crate::http::default_headers(), REQUEST_TIMEOUT);
        let base = Url::parse(&format!("http://{addr}/")).unwrap();
        let outcome = MusicServiceClient::sync_scrobbles_with_client(
            &http,
            &base,
            &[
                request(None),
                request(Some("e97f805a-ab48-4c52-855e-07049142113d")),
            ],
        )
        .await;

        assert!(outcome.error.is_none());
        assert_eq!(outcome.synced, 2);
        assert_eq!(
            server.await.unwrap(),
            vec!["/client/sync".to_string(), "/1/submit-listens".to_string()]
        );
    }

    /// Serve `script` in order, one connection each: the request's path and
    /// the status to answer with. Returns the paths hit.
    async fn scripted_server(
        script: Vec<(u16, &'static str)>,
    ) -> (Url, tokio::task::JoinHandle<Vec<String>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut paths = Vec::new();
            for (status, body) in script {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 65536];
                let n = socket.read(&mut buf).await.unwrap();
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                paths.push(request.split_whitespace().nth(1).unwrap().to_string());
                let reply = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                socket.write_all(reply.as_bytes()).await.unwrap();
            }
            paths
        });
        (Url::parse(&format!("http://{addr}/")).unwrap(), server)
    }

    fn play(mbid: Option<&str>) -> ScrobbleRequest {
        ScrobbleRequest {
            track_mbid: mbid.map(str::to_string),
            artist_name: "Megadeth".into(),
            track_name: "Live".into(),
            album_name: None,
            listened_at: "2023-03-26T00:00:00+00:00".into(),
            duration_ms: 1000,
            device_id: "d".into(),
            jellyfin_id: None,
            artist_mbid: None,
            release_mbid: None,
        }
    }

    const MBID: &str = "e97f805a-ab48-4c52-855e-07049142113d";
    const SYNC_ONE: &str = r#"{"synced":1,"duplicates":0,"skips_synced":0,"skips_duplicates":0}"#;

    /// One poisoned id-less play (the scrobble server 400s an out-of-range `listened_at`)
    /// neither blocks the identified play ahead of it nor the good id-less
    /// play beside it: the refused chunk is re-sent a play at a time.
    #[tokio::test]
    async fn a_poisoned_play_is_isolated_and_blocks_nothing() {
        let (base, server) = scripted_server(vec![
            (200, SYNC_ONE),             // identified, native
            (400, "{}"),                 // id-less chunk of two refused
            (400, "{}"),                 // poisoned alone
            (200, r#"{"status":"ok"}"#), // good id-less alone
        ])
        .await;
        let http = crate::http::client(crate::http::default_headers(), REQUEST_TIMEOUT);
        let outcome = MusicServiceClient::sync_scrobbles_with_client(
            &http,
            &base,
            &[play(None), play(Some(MBID)), play(None)],
        )
        .await;

        assert!(outcome.error.is_none());
        assert_eq!(outcome.drained, vec![1, 2]);
        assert_eq!(outcome.rejected, vec![0]);
        assert_eq!(server.await.unwrap().len(), 4);
    }

    /// A chunk that fails after an earlier one succeeded still reports the
    /// earlier plays drained, so the caller never re-sends them.
    #[tokio::test]
    async fn plays_accepted_before_a_failure_are_reported_drained() {
        let (base, server) = scripted_server(vec![
            (
                200,
                r#"{"synced":50,"duplicates":0,"skips_synced":0,"skips_duplicates":0}"#,
            ),
            (503, "{}"),
        ])
        .await;
        let http = crate::http::client(crate::http::default_headers(), REQUEST_TIMEOUT);
        let plays: Vec<_> = (0..51).map(|_| play(Some(MBID))).collect();
        let outcome = MusicServiceClient::sync_scrobbles_with_client(&http, &base, &plays).await;

        assert_eq!(outcome.drained, (0..50).collect::<Vec<_>>());
        assert!(outcome.rejected.is_empty());
        assert!(matches!(outcome.error, Some(AppError::Network(_))));
        server.await.unwrap();
    }

    /// A 429 is the service throttling, not a bad play: the drain stops after
    /// one request, nothing is isolated, and the plays stay owed.
    #[tokio::test]
    async fn a_429_stops_the_drain_without_resending_each_play() {
        let (base, server) = scripted_server(vec![(429, "{}")]).await;
        let http = crate::http::client(crate::http::default_headers(), REQUEST_TIMEOUT);
        let outcome = MusicServiceClient::sync_scrobbles_with_client(
            &http,
            &base,
            &[play(Some(MBID)), play(Some(MBID)), play(Some(MBID))],
        )
        .await;

        assert!(outcome.drained.is_empty() && outcome.rejected.is_empty());
        assert!(matches!(outcome.error, Some(AppError::Network(_))));
        assert_eq!(server.await.unwrap().len(), 1);
    }

    /// Against a plain ListenBrainz service (`/client/sync` 404s) a refused
    /// chunk is isolated over the LB route alone: one request a play.
    #[tokio::test]
    async fn a_404_fallback_refusal_is_resent_over_lb_only() {
        let (base, server) = scripted_server(vec![
            (404, "{}"), // native chunk
            (400, "{}"), // LB chunk refused
            (200, r#"{"status":"ok"}"#),
            (400, "{}"),
        ])
        .await;
        let http = crate::http::client(crate::http::default_headers(), REQUEST_TIMEOUT);
        let outcome = MusicServiceClient::sync_scrobbles_with_client(
            &http,
            &base,
            &[play(Some(MBID)), play(Some(MBID))],
        )
        .await;

        assert!(outcome.error.is_none());
        assert_eq!(outcome.drained, vec![0]);
        assert_eq!(outcome.rejected, vec![1]);
        assert_eq!(
            server.await.unwrap(),
            vec![
                "/client/sync",
                "/1/submit-listens",
                "/1/submit-listens",
                "/1/submit-listens"
            ]
        );
    }

    /// A 401 is the token's fault, not the play's: it stops the drain.
    #[tokio::test]
    async fn a_401_stops_the_drain_as_an_auth_error() {
        let (base, server) = scripted_server(vec![(401, "{}")]).await;
        let http = crate::http::client(crate::http::default_headers(), REQUEST_TIMEOUT);
        let outcome =
            MusicServiceClient::sync_scrobbles_with_client(&http, &base, &[play(Some(MBID))]).await;

        assert!(outcome.drained.is_empty() && outcome.rejected.is_empty());
        assert!(matches!(outcome.error, Some(AppError::Auth(_))));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn transport_detail_keeps_the_cause() {
        // A refused connection is the cheapest real transport error. Display
        // alone stops at reqwest's own wrapper, so what is asserted here is
        // that the cause underneath it survives into the rendered string.
        let error = crate::http::client(crate::http::default_headers(), REQUEST_TIMEOUT)
            .get("http://127.0.0.1:1/")
            .send()
            .await
            .expect_err("port 1 on loopback never answers");

        let rendered = transport_detail(&error);
        assert!(
            rendered.len() > error.to_string().len(),
            "expected a cause beyond {:?}, got {rendered:?}",
            error.to_string()
        );
    }

    #[test]
    fn test_base_url_preserved() {
        // Base URL should be used as-is — Url always adds trailing / for root path
        let client = MusicServiceClient::new("https://music.example.com", "test-token").unwrap();
        assert_eq!(client.base_url.as_str(), "https://music.example.com/");
    }

    #[test]
    fn test_trailing_slash_normalised() {
        // Both with and without trailing slash should produce the same result
        let a = MusicServiceClient::new("https://music.example.com", "test-token").unwrap();
        let b = MusicServiceClient::new("https://music.example.com/", "test-token").unwrap();
        assert_eq!(a.base_url.as_str(), b.base_url.as_str());
    }

    #[test]
    fn test_path_trailing_slash_stripped() {
        // Non-root paths should have trailing slash stripped
        let client =
            MusicServiceClient::new("https://music.example.com/scrobble/", "test-token").unwrap();
        assert_eq!(
            client.base_url.as_str(),
            "https://music.example.com/scrobble"
        );
    }

    #[test]
    fn test_endpoint_from_base() {
        let base_url = Url::parse("https://music.example.com").unwrap();

        let url = MusicServiceClient::endpoint_from_base(&base_url, "1/submit-listens").unwrap();
        assert_eq!(url.as_str(), "https://music.example.com/1/submit-listens");
    }

    #[test]
    fn test_invalid_url() {
        let result = MusicServiceClient::new("not a url", "token");
        assert!(result.is_err());
    }

    #[test]
    fn test_clone_for_request() {
        let client = MusicServiceClient::new("https://music.example.com", "test-token").unwrap();

        let (http, base_url) = client.clone_for_request();

        assert_eq!(base_url.as_str(), "https://music.example.com/");
        // HTTP client should be the same Arc reference
        assert!(Arc::ptr_eq(&http, &client.http));
    }

    #[test]
    fn test_scrobble_request_to_lb_import() {
        let scrobbles = vec![
            ScrobbleRequest {
                track_mbid: None,
                artist_name: "Artist 1".to_string(),
                track_name: "Track 1".to_string(),
                album_name: None,
                listened_at: "2025-01-15T10:30:00Z".to_string(),
                duration_ms: 180000,
                device_id: "dev-1".to_string(),
                jellyfin_id: None,
                artist_mbid: None,
                release_mbid: None,
            },
            ScrobbleRequest {
                track_mbid: Some("xyz-789".to_string()),
                artist_name: "Artist 2".to_string(),
                track_name: "Track 2".to_string(),
                album_name: Some("Album 2".to_string()),
                listened_at: "2025-01-15T10:35:00Z".to_string(),
                duration_ms: 200000,
                device_id: "dev-1".to_string(),
                jellyfin_id: None,
                artist_mbid: None,
                release_mbid: None,
            },
        ];

        let payload = ScrobbleRequest::to_lb_import(&scrobbles);

        assert_eq!(payload["listen_type"], "import");
        assert_eq!(payload["payload"].as_array().unwrap().len(), 2);
        assert_eq!(
            payload["payload"][0]["track_metadata"]["artist_name"],
            "Artist 1"
        );
        assert_eq!(
            payload["payload"][1]["track_metadata"]["artist_name"],
            "Artist 2"
        );
    }
}
