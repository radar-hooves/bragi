//! Jellyfin API client.
//!
//! Handles authentication and HTTP communication with the Jellyfin server.
//! Supports username/password login and Quick Connect flows.

use std::fmt;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;
use url::Url;

use crate::error::AppError;

/// Request timeout for API calls.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Normalise a server URL entered by the user.
///
/// - Prepends `https://` when no scheme is present (bare hostnames like `jellyfin.example.com`).
/// - Strips trailing slashes so path construction is consistent.
/// - Returns a parsed `Url` or an `AppError::Config` on failure.
fn normalise_server_url(raw: &str) -> Result<Url, AppError> {
    let trimmed = raw.trim().trim_end_matches('/');

    // If the input has no scheme, assume HTTPS.
    let with_scheme = if !trimmed.contains("://") {
        format!("https://{trimmed}")
    } else {
        trimmed.to_string()
    };

    Url::parse(&with_scheme)
        .map_err(|e| AppError::Config(format!("Invalid Jellyfin server URL '{raw}': {e}")))
}
/// Connection timeout for establishing connections.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Idle/read timeout for downloads: no total budget (a big transcode can stream
/// for a long time), but abort if the body makes zero progress this long so a
/// stalled connection can't hang a download forever.
const DOWNLOAD_READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Build the `Authorization: MediaBrowser ...` header value.
///
/// If `token` is `Some`, includes the `Token` parameter for authenticated requests.
/// If `token` is `None`, omits it (used for unauthenticated auth requests).
fn build_auth_header(token: Option<&str>) -> Result<String, AppError> {
    let device_id = get_or_create_device_id()?;
    let version = env!("CARGO_PKG_VERSION");

    let mut parts = Vec::with_capacity(5);
    if let Some(t) = token {
        parts.push(format!("Token=\"{}\"", t));
    }
    parts.push("Client=\"Bragi\"".to_string());
    parts.push("Device=\"Desktop\"".to_string());
    parts.push(format!("DeviceId=\"{}\"", device_id));
    parts.push(format!("Version=\"{}\"", version));

    Ok(format!("MediaBrowser {}", parts.join(", ")))
}

/// Build the HTTP client used for large/slow downloads.
///
/// Shares the authenticated default headers, user-agent, connect timeout and
/// minimum TLS version with the main client, but deliberately omits the total
/// `REQUEST_TIMEOUT` so a large file or a slow server-side transcode isn't
/// aborted mid-body.
fn build_download_client(headers: HeaderMap) -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .default_headers(headers)
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(DOWNLOAD_READ_TIMEOUT)
        .build()
        .map_err(|e| AppError::Network(format!("Failed to create download HTTP client: {}", e)))
}

/// Jellyfin API client.
///
/// Cheaply cloneable — the underlying `reqwest::Client` is `Arc`-backed.
#[derive(Clone)]
pub struct JellyfinClient {
    base_url: Url,
    api_key: String,
    user_id: String,
    http: ClientWithMiddleware,
    /// HTTP client for large/slow downloads (offline caching, transcodes).
    ///
    /// Identical to `http` except it has NO total request timeout, so a big or
    /// server-side-transcoded file body isn't aborted after 30s. `connect_timeout`
    /// still bounds connection establishment.
    download_http: reqwest::Client,
    /// Fired once per session when a session call gets a 401.
    ///
    /// Shared through every `.clone()` (the sync scheduler clones the client),
    /// so all clones signal the same deduplicating notifier. `None` until the
    /// setup hook injects it (an `AppHandle` is required to emit).
    auth_expired_cb: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

// Custom Debug implementation to redact API key
impl fmt::Debug for JellyfinClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JellyfinClient")
            .field("base_url", &self.base_url)
            .field("api_key", &"[REDACTED]")
            .field("user_id", &self.user_id)
            .finish()
    }
}

/// Credentials required to connect to Jellyfin.
#[derive(Clone)]
pub struct JellyfinCredentials {
    pub server_url: String,
    pub api_key: String,
    pub user_id: String,
}

// Custom Debug implementation to redact API key
impl fmt::Debug for JellyfinCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JellyfinCredentials")
            .field("server_url", &self.server_url)
            .field("api_key", &"[REDACTED]")
            .field("user_id", &self.user_id)
            .finish()
    }
}

impl JellyfinClient {
    /// Create a new Jellyfin client.
    ///
    /// # Arguments
    /// * `server_url` - Base URL of the Jellyfin server (e.g., "https://jellyfin.example.com")
    /// * `api_key` - API key for authentication
    /// * `user_id` - User ID for user-specific endpoints
    ///
    /// # Errors
    /// Returns `AppError::Config` if the URL is invalid.
    pub fn new(server_url: &str, api_key: &str, user_id: &str) -> Result<Self, AppError> {
        let base_url = normalise_server_url(server_url)?;

        // Build default headers with Authorization: MediaBrowser scheme
        let auth_value = build_auth_header(Some(api_key))?;
        let mut headers = crate::http::default_headers();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_value)
                .map_err(|_| AppError::Config("Invalid access token format".to_string()))?,
        );

        let http = crate::http::client(headers.clone(), REQUEST_TIMEOUT);
        let download_http = build_download_client(headers)?;

        Ok(Self {
            base_url,
            api_key: api_key.to_string(),
            user_id: user_id.to_string(),
            http,
            download_http,
            auth_expired_cb: None,
        })
    }

    /// Create a new Jellyfin client with an explicit device ID (test seam only).
    ///
    /// Bypasses `get_or_create_device_id()` so unit tests never touch the
    /// credential store. Production code must use `new()`.
    #[cfg(test)]
    pub fn new_with_device_id(
        server_url: &str,
        api_key: &str,
        user_id: &str,
        device_id: &str,
    ) -> Result<Self, AppError> {
        let base_url = normalise_server_url(server_url)?;

        let version = env!("CARGO_PKG_VERSION");
        let auth_value = format!(
            "MediaBrowser Token=\"{}\", Client=\"Bragi\", Device=\"Desktop\", DeviceId=\"{}\", Version=\"{}\"",
            api_key, device_id, version
        );
        let mut headers = crate::http::default_headers();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_value)
                .map_err(|_| AppError::Config("Invalid access token format".to_string()))?,
        );

        let http = crate::http::client(headers.clone(), REQUEST_TIMEOUT);
        let download_http = build_download_client(headers)?;

        Ok(Self {
            base_url,
            api_key: api_key.to_string(),
            user_id: user_id.to_string(),
            http,
            download_http,
            auth_expired_cb: None,
        })
    }

    /// Create a client from credentials struct.
    pub fn from_credentials(credentials: &JellyfinCredentials) -> Result<Self, AppError> {
        Self::new(
            &credentials.server_url,
            &credentials.api_key,
            &credentials.user_id,
        )
    }

    /// Get the base URL.
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// Get the user ID.
    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    /// Get the API key (for re-authentication scenarios).
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Install the session-expiry callback.
    ///
    /// Called once during setup / on (re)connect. The callback is `Arc`-shared,
    /// so every subsequent `.clone()` of this client fires the same notifier.
    pub fn set_auth_expired_callback(&mut self, cb: std::sync::Arc<dyn Fn() + Send + Sync>) {
        self.auth_expired_cb = Some(cb);
    }

    /// Fire the session-expiry callback if one is installed.
    ///
    /// Invoked from the 401 handling of session (already-authenticated) calls.
    /// The callback itself deduplicates, so calling it from every concurrent
    /// 401 is safe and produces at most one event per session.
    pub(crate) fn fire_auth_expired(&self) {
        if let Some(cb) = &self.auth_expired_cb {
            cb();
        }
    }

    /// Get the traced HTTP client for making API requests.
    pub fn http(&self) -> &ClientWithMiddleware {
        &self.http
    }

    /// Get the HTTP client for large/slow downloads (no total request timeout).
    pub fn download_http(&self) -> &reqwest::Client {
        &self.download_http
    }

    /// Build a URL for an API endpoint.
    ///
    /// # Arguments
    /// * `path` - API path (e.g., "/Users/{userId}/Items")
    ///
    /// # Returns
    /// The full URL for the endpoint.
    pub fn endpoint(&self, path: &str) -> Url {
        let mut url = self.base_url.clone();
        // Ensure path starts with /
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{}", path)
        };
        url.set_path(&path);
        url
    }

    /// Build a URL for a user-specific endpoint.
    ///
    /// Replaces `{userId}` in the path with the actual user ID.
    pub fn user_endpoint(&self, path: &str) -> Url {
        let path = path.replace("{userId}", &self.user_id);
        self.endpoint(&path)
    }

    /// Test the connection to the Jellyfin server.
    ///
    /// Makes a simple API call to verify credentials are valid.
    pub async fn test_connection(&self) -> Result<(), AppError> {
        let url = self.user_endpoint("/Users/{userId}");

        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| AppError::Network(format!("Connection failed: {}", e)))?;

        if response.status().is_success() {
            Ok(())
        } else if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            Err(AppError::Auth(
                "Invalid access token or user ID".to_string(),
            ))
        } else {
            Err(AppError::Jellyfin(format!(
                "Jellyfin returned status {}",
                response.status()
            )))
        }
    }

    /// End this client's session on the server (`POST /Sessions/Logout`).
    ///
    /// Used for short-lived connections to a server this instance doesn't
    /// otherwise manage (e.g. a one-shot fetch from a friend's Jellyfin) so
    /// the access token stops showing up as an active session once we're
    /// done with it, rather than sitting authenticated until it expires on
    /// its own.
    pub async fn logout(&self) -> Result<(), AppError> {
        let url = self.endpoint("/Sessions/Logout");
        let response = self
            .http
            .post(url)
            .send()
            .await
            .map_err(|e| AppError::Network(format!("Logout failed: {}", e)))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(AppError::Jellyfin(format!(
                "Jellyfin returned status {} on logout",
                response.status()
            )))
        }
    }
}

/// Keyring service name for credential storage.
const KEYRING_SERVICE: &str = "bragi-jellyfin";

/// Keyring account key for the consolidated credentials entry.
const CREDENTIALS_KEY: &str = "credentials";

/// Keyring account key for the persistent device identity.
const DEVICE_KEY: &str = "device";

/// All Jellyfin session credentials serialised as a single keyring entry.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredCredentials {
    server_url: String,
    api_key: String,
    user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth_method: Option<String>,
}

/// Store Jellyfin credentials securely in the system keyring.
///
/// On macOS this uses Keychain; on Windows, Credential Manager;
/// on Linux, Secret Service (e.g. GNOME Keyring).
///
/// All fields are stored as a single JSON entry to minimise
/// macOS Keychain access prompts (one prompt per entry).
pub fn store_credentials(credentials: &JellyfinCredentials) -> Result<(), AppError> {
    let stored = StoredCredentials {
        server_url: credentials.server_url.clone(),
        api_key: credentials.api_key.clone(),
        user_id: credentials.user_id.clone(),
        auth_method: None,
    };
    let json = serde_json::to_string(&stored)
        .map_err(|e| AppError::Config(format!("Failed to serialise credentials: {e}")))?;

    keyring_set(CREDENTIALS_KEY, &json)?;
    tracing::info!("Stored Jellyfin credentials in system keyring");
    Ok(())
}

/// Load Jellyfin credentials from the system keyring.
///
/// Returns `None` if credentials haven't been stored yet.
/// Automatically migrates legacy per-key entries to the new consolidated format.
pub fn load_credentials() -> Result<Option<JellyfinCredentials>, AppError> {
    // Try consolidated entry first.
    if let Some(json) = keyring_get(CREDENTIALS_KEY)? {
        let stored: StoredCredentials = serde_json::from_str(&json)
            .map_err(|e| AppError::Config(format!("Failed to parse stored credentials: {e}")))?;
        return Ok(Some(JellyfinCredentials {
            server_url: stored.server_url,
            api_key: stored.api_key,
            user_id: stored.user_id,
        }));
    }

    // Fall back to legacy per-key entries and migrate if found.
    migrate_legacy_credentials()
}

/// Clear stored Jellyfin credentials.
///
/// The device ID is intentionally NOT cleared; it is a device identity,
/// not a session credential, and persists across logouts.
pub fn clear_credentials() -> Result<(), AppError> {
    keyring_delete(CREDENTIALS_KEY)?;
    // Also clean up any leftover legacy entries.
    for key in ["server_url", "api_key", "user_id", "auth_method"] {
        keyring_delete(key)?;
    }
    tracing::info!("Cleared Jellyfin credentials from system keyring");
    Ok(())
}

/// Store the authentication method used for the current session.
pub fn store_auth_method(method: &str) -> Result<(), AppError> {
    // Load the existing blob, update auth_method, re-store.
    let json = keyring_get(CREDENTIALS_KEY)?.ok_or_else(|| {
        AppError::Config("No credentials stored to attach auth method".to_string())
    })?;
    let mut stored: StoredCredentials = serde_json::from_str(&json)
        .map_err(|e| AppError::Config(format!("Failed to parse stored credentials: {e}")))?;
    stored.auth_method = Some(method.to_string());
    let updated = serde_json::to_string(&stored)
        .map_err(|e| AppError::Config(format!("Failed to serialise credentials: {e}")))?;
    keyring_set(CREDENTIALS_KEY, &updated)
}

/// Load or generate a persistent device ID for Jellyfin session tracking.
///
/// The device ID is a UUID v4 stored in the system keyring. It persists
/// across logouts and reconnections, giving Jellyfin a stable device identity.
pub fn get_or_create_device_id() -> Result<String, AppError> {
    if let Some(existing) = keyring_get(DEVICE_KEY)? {
        return Ok(existing);
    }
    // Also check the legacy key before generating a new one.
    if let Some(existing) = keyring_get("device_id")? {
        keyring_set(DEVICE_KEY, &existing)?;
        keyring_delete("device_id")?;
        return Ok(existing);
    }
    let new_id = uuid::Uuid::new_v4().to_string();
    keyring_set(DEVICE_KEY, &new_id)?;
    tracing::info!("Generated new device ID for Jellyfin session tracking");
    Ok(new_id)
}

/// Migrate legacy per-key credential entries to the consolidated format.
///
/// If all three required keys exist, stores them as a single JSON blob
/// and deletes the old entries. Returns `None` if no legacy entries found.
fn migrate_legacy_credentials() -> Result<Option<JellyfinCredentials>, AppError> {
    let server_url = match keyring_get("server_url")? {
        Some(v) => v,
        None => return Ok(None),
    };
    let api_key = match keyring_get("api_key")? {
        Some(v) => v,
        None => return Ok(None),
    };
    let user_id = match keyring_get("user_id")? {
        Some(v) => v,
        None => return Ok(None),
    };
    let auth_method = keyring_get("auth_method")?;

    let stored = StoredCredentials {
        server_url: server_url.clone(),
        api_key: api_key.clone(),
        user_id: user_id.clone(),
        auth_method,
    };
    let json = serde_json::to_string(&stored)
        .map_err(|e| AppError::Config(format!("Failed to serialise credentials: {e}")))?;
    keyring_set(CREDENTIALS_KEY, &json)?;

    // Clean up legacy entries.
    for key in ["server_url", "api_key", "user_id", "auth_method"] {
        keyring_delete(key)?;
    }

    tracing::info!("Migrated legacy Jellyfin credentials to consolidated format");
    Ok(Some(JellyfinCredentials {
        server_url,
        api_key,
        user_id,
    }))
}

// ---------------------------------------------------------------------------
// Authentication flows
// ---------------------------------------------------------------------------

/// Response from Jellyfin `AuthenticateByName` and `AuthenticateWithQuickConnect`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AuthenticationResult {
    access_token: String,
    user: AuthenticatedUser,
}

/// Authenticated user info nested in `AuthenticationResult`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AuthenticatedUser {
    id: String,
}

/// Quick Connect initiation/poll result from Jellyfin.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct QuickConnectResult {
    /// Server-side secret used to poll and complete authentication.
    pub secret: String,
    /// Short numeric code the user enters in another Jellyfin client.
    pub code: String,
    /// Whether the code has been authorised by the user.
    pub authenticated: bool,
}

/// Build a one-off HTTP client for unauthenticated Jellyfin API calls.
///
/// Used for login and Quick Connect flows that run before a `JellyfinClient` exists.
/// Does not enforce HTTPS — homelab servers commonly run on plain HTTP.
fn unauthenticated_client() -> ClientWithMiddleware {
    crate::http::client(crate::http::default_headers(), REQUEST_TIMEOUT)
}

/// Build the `Authorization` header value for unauthenticated requests.
fn unauthed_header_value() -> Result<HeaderValue, AppError> {
    let value = build_auth_header(None)?;
    HeaderValue::from_str(&value)
        .map_err(|_| AppError::Config("Failed to build authorization header".to_string()))
}

/// Authenticate with a Jellyfin server using username and password.
///
/// Calls `POST /Users/AuthenticateByName` and returns credentials containing
/// the server URL, access token, and user ID on success.
pub async fn authenticate_by_name(
    server_url: &str,
    username: &str,
    password: &str,
) -> Result<JellyfinCredentials, AppError> {
    let base_url = normalise_server_url(server_url)?;
    let normalised = base_url.to_string();

    let http = unauthenticated_client();
    let auth_header = unauthed_header_value()?;

    let mut url = base_url;
    url.set_path("/Users/AuthenticateByName");

    let body = serde_json::json!({
        "Username": username,
        "Pw": password,
    });

    let response = http
        .post(url)
        .header(AUTHORIZATION, auth_header)
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            let mut msg = format!("Connection failed: {e}");
            let mut source = std::error::Error::source(&e);
            while let Some(cause) = source {
                msg.push_str(&format!(" -> {cause}"));
                source = std::error::Error::source(cause);
            }
            AppError::Network(msg)
        })?;

    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(AppError::Auth("Invalid username or password".to_string()));
    }

    if !response.status().is_success() {
        return Err(AppError::Jellyfin(format!(
            "Authentication failed with status {}",
            response.status()
        )));
    }

    let result: AuthenticationResult = response
        .json()
        .await
        .map_err(|e| AppError::Jellyfin(format!("Failed to parse auth response: {}", e)))?;

    Ok(JellyfinCredentials {
        server_url: normalised,
        api_key: result.access_token,
        user_id: result.user.id,
    })
}

// ---------------------------------------------------------------------------
// Quick Connect
// ---------------------------------------------------------------------------

/// Check whether Quick Connect is enabled on a Jellyfin server.
pub async fn quick_connect_enabled(server_url: &str) -> Result<bool, AppError> {
    let base_url = normalise_server_url(server_url)?;

    let http = unauthenticated_client();
    let auth_header = unauthed_header_value()?;

    let mut url = base_url;
    url.set_path("/QuickConnect/Enabled");

    let response = http
        .get(url)
        .header(AUTHORIZATION, auth_header)
        .send()
        .await
        .map_err(|e| AppError::Network(format!("Connection failed: {}", e)))?;

    if !response.status().is_success() {
        return Ok(false);
    }

    let enabled: bool = response.json().await.unwrap_or(false);

    Ok(enabled)
}

/// Initiate a Quick Connect session on a Jellyfin server.
///
/// Returns a `QuickConnectResult` containing the `code` (displayed to the user)
/// and the `secret` (used to poll and complete authentication).
pub async fn quick_connect_initiate(server_url: &str) -> Result<QuickConnectResult, AppError> {
    let base_url = normalise_server_url(server_url)?;

    let http = unauthenticated_client();
    let auth_header = unauthed_header_value()?;

    let mut url = base_url;
    url.set_path("/QuickConnect/Initiate");

    let response = http
        .post(url)
        .header(AUTHORIZATION, auth_header)
        .send()
        .await
        .map_err(|e| AppError::Network(format!("Failed to initiate Quick Connect: {}", e)))?;

    if !response.status().is_success() {
        return Err(AppError::Jellyfin(format!(
            "Quick Connect initiation failed with status {}",
            response.status()
        )));
    }

    response
        .json()
        .await
        .map_err(|e| AppError::Jellyfin(format!("Failed to parse Quick Connect response: {}", e)))
}

/// Poll a Quick Connect session to check if it has been authorised.
///
/// Returns `true` if the user has approved the code in another Jellyfin client.
pub async fn quick_connect_poll(server_url: &str, secret: &str) -> Result<bool, AppError> {
    let base_url = normalise_server_url(server_url)?;

    let http = unauthenticated_client();
    let auth_header = unauthed_header_value()?;

    let mut url = base_url;
    url.set_path("/QuickConnect/Connect");
    url.query_pairs_mut().append_pair("secret", secret);

    let response = http
        .get(url)
        .header(AUTHORIZATION, auth_header)
        .send()
        .await
        .map_err(|e| AppError::Network(format!("Failed to poll Quick Connect: {}", e)))?;

    if !response.status().is_success() {
        return Ok(false);
    }

    let result: QuickConnectResult = response
        .json()
        .await
        .map_err(|e| AppError::Jellyfin(format!("Failed to parse Quick Connect poll: {}", e)))?;

    Ok(result.authenticated)
}

/// Complete Quick Connect authentication by exchanging the secret for an access token.
///
/// Calls `POST /Users/AuthenticateWithQuickConnect` and returns credentials.
pub async fn quick_connect_authenticate(
    server_url: &str,
    secret: &str,
) -> Result<JellyfinCredentials, AppError> {
    let base_url = normalise_server_url(server_url)?;
    let normalised = base_url.to_string();

    let http = unauthenticated_client();
    let auth_header = unauthed_header_value()?;

    let mut url = base_url;
    url.set_path("/Users/AuthenticateWithQuickConnect");

    let body = serde_json::json!({
        "Secret": secret,
    });

    let response = http
        .post(url)
        .header(AUTHORIZATION, auth_header)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Network(format!("Quick Connect authentication failed: {}", e)))?;

    if !response.status().is_success() {
        return Err(AppError::Jellyfin(format!(
            "Quick Connect authentication failed with status {}",
            response.status()
        )));
    }

    let result: AuthenticationResult = response
        .json()
        .await
        .map_err(|e| AppError::Jellyfin(format!("Failed to parse auth response: {}", e)))?;

    Ok(JellyfinCredentials {
        server_url: normalised,
        api_key: result.access_token,
        user_id: result.user.id,
    })
}

// ---------------------------------------------------------------------------
// Keyring helpers (delegate to shared credential_store backend)
// ---------------------------------------------------------------------------

fn keyring_set(key: &str, value: &str) -> Result<(), AppError> {
    crate::credential_store::credential_set(KEYRING_SERVICE, key, value)
}

fn keyring_get(key: &str) -> Result<Option<String>, AppError> {
    crate::credential_store::credential_get(KEYRING_SERVICE, key)
}

fn keyring_delete(key: &str) -> Result<(), AppError> {
    crate::credential_store::credential_delete(KEYRING_SERVICE, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_building() {
        let client = JellyfinClient::new_with_device_id(
            "https://jellyfin.example.com",
            "test-api-key",
            "test-user-id",
            "test-device-id",
        )
        .unwrap();

        let url = client.endpoint("/Items");
        assert_eq!(url.as_str(), "https://jellyfin.example.com/Items");

        let url = client.user_endpoint("/Users/{userId}/Items");
        assert_eq!(
            url.as_str(),
            "https://jellyfin.example.com/Users/test-user-id/Items"
        );
    }

    #[test]
    fn test_invalid_url() {
        // URL parse fails before the credential store is touched — safe to use new_with_device_id
        // or new(); using new_with_device_id keeps the test hermetic regardless.
        let result =
            JellyfinClient::new_with_device_id("not a url", "key", "user", "test-device-id");
        assert!(result.is_err());
    }
}
