//! The app's outbound HTTP clients.
//!
//! Every API client is `telemetry::http_client()` underneath: `reqwest` under
//! middleware that carries W3C `traceparent` on each request and opens a client
//! span recording the method, the host, the port, the path and the status,
//! because a request's path is the user's own listening history. The span never records the query string, so a Subsonic
//! auth token — signed into the query, never the path — cannot reach Tempo.
//! One inner client is built for the process, so every client derived here
//! shares its connection pool.
//!
//! The crate owns that inner `reqwest::ClientBuilder`, so the default headers
//! and the total timeout are applied per request by an initialiser instead.
//! The ten-second connect timeout comes from the crate: only the total timeout
//! has a `RequestBuilder` form, so without it a black-holed LAN address would
//! hang for the whole total timeout rather than failing at connect.
//!
//! `read_timeout` has no per-request form either, which is why the
//! body-streaming clients — the `download_http` pair and the local proxy —
//! build a plain `reqwest::Client`: they carry no total budget on purpose (a
//! server-side transcode legitimately streams for minutes) and are bounded by
//! connect and idle-read alone.
//!
//! No client sets `min_tls_version`: the floor is rustls's own, which speaks
//! TLS 1.2 and 1.3 and nothing else, so a builder call could only ever raise
//! it. Keep reqwest's `rustls` feature — the crate installs a crypto provider
//! only when it builds a client, so `rustls-no-provider` alone would panic any
//! client bragi builds before the first `http_client()` call.

use std::sync::OnceLock;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, RequestBuilder};

/// The process's one traced client; a clone shares the connection pool.
///
/// Built on first use from the main thread's first client construction, never
/// inside `telemetry::init` — the two are independent.
fn shared() -> &'static ClientWithMiddleware {
    static CLIENT: OnceLock<ClientWithMiddleware> = OnceLock::new();
    CLIENT.get_or_init(telemetry::http_client)
}

/// The `User-Agent` every bragi request carries, plus room for auth headers.
pub fn default_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(concat!("Bragi/", env!("CARGO_PKG_VERSION"))),
    );
    headers
}

/// A traced client applying `headers` and a total `timeout` to every request.
///
/// The initialiser runs when the builder is created, so a call site's own
/// `.timeout()` replaces this one. A same-named `.header()` does NOT replace:
/// `reqwest::RequestBuilder::header` appends, and the header would be sent
/// twice. Pass a header here or at the call site, never both.
pub fn client(headers: HeaderMap, timeout: Duration) -> ClientWithMiddleware {
    ClientBuilder::from_client(shared().clone())
        .with_init(move |request: RequestBuilder| request.headers(headers.clone()).timeout(timeout))
        .build()
}

/// Read a response body, giving up with `Ok(None)` once it exceeds
/// `max_bytes`: an advertised length is rejected before any download, and a
/// body without one (or a slow drip) still cannot grow unbounded in memory.
/// A transport error has its URL stripped (a Subsonic URL carries its token).
pub async fn read_capped(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Option<bytes::Bytes>, crate::error::AppError> {
    use futures_util::StreamExt;

    if response
        .content_length()
        .is_some_and(|len| len as usize > max_bytes)
    {
        return Ok(None);
    }
    let mut stream = response.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|e| crate::error::AppError::Network(e.without_url().to_string()))?;
        if buf.len() + chunk.len() > max_bytes {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(Some(bytes::Bytes::from(buf)))
}

#[cfg(test)]
mod tests {
    use reqwest::header::AUTHORIZATION;

    use super::*;

    /// What the crate's inner client cannot hold has to arrive per request.
    #[test]
    fn a_derived_client_applies_its_headers_and_timeout_to_every_request() {
        let mut headers = default_headers();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer token"));

        let request = client(headers, Duration::from_secs(30))
            .get("https://music.example.com/health")
            .build()
            .expect("a plain GET builds");

        assert_eq!(
            request.headers()[USER_AGENT],
            concat!("Bragi/", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(request.headers()[AUTHORIZATION], "Bearer token");
        assert_eq!(request.timeout(), Some(&Duration::from_secs(30)));
    }

    /// A call site's timeout replaces the initialiser's; a same-named header
    /// appends to it, which is why no call site sets one this module already
    /// carries.
    #[test]
    fn a_call_sites_timeout_replaces_and_its_header_appends() {
        let request = client(default_headers(), Duration::from_secs(30))
            .get("https://music.example.com/health")
            .timeout(Duration::from_secs(1))
            .header(USER_AGENT, "Bragi/other")
            .build()
            .expect("a plain GET builds");

        assert_eq!(request.timeout(), Some(&Duration::from_secs(1)));
        assert_eq!(
            request.headers().get_all(USER_AGENT).iter().count(),
            2,
            "reqwest appends a header rather than replacing it"
        );
    }
}
