//! Jellyfin connection management commands.
//!
//! Handles connecting to and disconnecting from a Jellyfin server.
//! Supports username/password login, Quick Connect, and (deprecated) API key auth.

use crate::audit;
use crate::error::{CommandError, CommandResult};
use crate::jellyfin::{
    authenticate_by_name, clear_credentials, get_or_create_device_id, quick_connect_authenticate,
    quick_connect_enabled, quick_connect_initiate, quick_connect_poll, store_auth_method,
    store_credentials, JellyfinClient, JellyfinCredentials,
};
use crate::mediaserver::subsonic::SubsonicClient;
use crate::mediaserver::{MediaClient, ServerKind};
use crate::proxy;
use crate::state::AppState;
use serde::Serialize;

/// Media-server connection status.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    pub connected: bool,
    pub server_url: Option<String>,
    /// Active backend when connected: `jellyfin` | `opensubsonic`.
    pub server_type: String,
    /// Active connection is cleartext `http://` to a non-loopback host, so the
    /// UI can render a persistent "unencrypted" warning while connected.
    pub insecure: bool,
}

/// Quick Connect initiation info returned to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickConnectInfo {
    pub code: String,
    pub secret: String,
}

// ---------------------------------------------------------------------------
// Shared helper: finalise connection after obtaining credentials
// ---------------------------------------------------------------------------

/// Complete the connection flow: create client, test, store credentials, start proxy.
async fn finalise_connection(
    state: &tauri::State<'_, AppState>,
    credentials: JellyfinCredentials,
    auth_method: &str,
    cid: &str,
) -> CommandResult<()> {
    // Create client and test connection
    let mut client = match JellyfinClient::from_credentials(&credentials) {
        Ok(c) => c,
        Err(e) => {
            audit::log_auth_event("jellyfin", "connect", false, audit::error_kind(&e), cid);
            return Err(e.to_string().into());
        }
    };

    // Install the session-expiry callback so future 401s on this session emit
    // the deduplicated reconnect event. Harmless here: login-time 401s
    // come from test_connection, which maps to AppError::Auth and never fires
    // this callback — only already-authenticated session calls do.
    let auth_cb = state.auth_notifier.get().map(|n| n.callback());
    if let Some(cb) = &auth_cb {
        client.set_auth_expired_callback(cb.clone());
    }

    if let Err(e) = client.test_connection().await {
        audit::log_auth_event(
            "jellyfin",
            "connection_test",
            false,
            audit::error_kind(&e),
            cid,
        );
        return Err(e.to_string().into());
    }

    // Cleartext (http://) LAN parity is allowed, but a non-loopback http server
    // exposes credentials + audio over the network — record it.
    if crate::mediaserver::is_cleartext_url(client.base_url()) {
        audit::log_connection_event(
            "jellyfin",
            "cleartext_connection",
            client.base_url().as_str(),
            cid,
        );
    }

    // Store credentials securely
    match store_credentials(&credentials) {
        Ok(()) => audit::log_credential_event("jellyfin", "store", true, cid),
        Err(e) => {
            audit::log_credential_event("jellyfin", "store", false, cid);
            return Err(e.to_string().into());
        }
    }

    // Store auth method
    if let Err(e) = store_auth_method(auth_method) {
        tracing::warn!("Failed to store auth method: {}", e);
    }

    // Get device ID for proxy
    let device_id = get_or_create_device_id()?;

    // Start local proxy for secure credential injection
    let proxy_handle = proxy::start_proxy(
        &credentials.server_url,
        proxy::ProxyUpstream::Jellyfin {
            api_key: credentials.api_key.clone(),
            device_id,
        },
        auth_cb,
    )
    .await
    .map_err(|e| format!("Failed to start local proxy: {}", e))?;
    tracing::info!("Local proxy started on port {}", proxy_handle.port());

    // Update app state
    // Record the active backend so startup/state-load restores Jellyfin.
    // Keep the persisted SERVER_TYPE and the in-memory read filter in lock-step.
    let _ = state
        .db
        .set_setting(crate::db::settings::keys::SERVER_TYPE, "jellyfin");
    state.db.set_active_source(ServerKind::Jellyfin);

    let mut media_guard = state.media.write().await;
    *media_guard = Some(MediaClient::Jellyfin(client));
    drop(media_guard);

    // A fresh session re-arms the dedup latch so a later expiry emits again.
    state.reset_auth_expired();

    state.set_proxy(proxy_handle).await;

    audit::log_auth_event("jellyfin", "connect", true, "connected", cid);
    Ok(())
}

// ---------------------------------------------------------------------------
// Username/password login (primary)
// ---------------------------------------------------------------------------

/// Log in to a Jellyfin server using username and password.
///
/// Authenticates via `AuthenticateByName`, stores the resulting access token
/// in the system keyring, and starts the local proxy.
#[tauri::command]
pub async fn login_jellyfin(
    state: tauri::State<'_, AppState>,
    server_url: String,
    username: String,
    password: String,
) -> CommandResult<()> {
    let cid = audit::correlation_id();
    audit::log_connection_event("jellyfin", "login_attempt", &server_url, &cid);

    let credentials = authenticate_by_name(&server_url, &username, &password)
        .await
        .map_err(|e| {
            audit::log_auth_event("jellyfin", "login", false, audit::error_kind(&e), &cid);
            e.to_string()
        })?;

    finalise_connection(&state, credentials, "password", &cid).await
}

// ---------------------------------------------------------------------------
// Quick Connect
// ---------------------------------------------------------------------------

/// Check whether Quick Connect is enabled on a Jellyfin server.
#[tauri::command]
pub async fn quick_connect_check(server_url: String) -> CommandResult<bool> {
    quick_connect_enabled(&server_url)
        .await
        .map_err(CommandError::msg)
}

/// Initiate a Quick Connect session on a Jellyfin server.
///
/// Returns the code (to display to the user) and secret (to poll with).
#[tauri::command]
pub async fn quick_connect_start(server_url: String) -> CommandResult<QuickConnectInfo> {
    let result = quick_connect_initiate(&server_url).await?;

    Ok(QuickConnectInfo {
        code: result.code,
        secret: result.secret,
    })
}

/// Poll a Quick Connect session to check if the user has authorised it.
///
/// Returns `true` when the code has been approved in another Jellyfin client.
#[tauri::command]
pub async fn quick_connect_status(server_url: String, secret: String) -> CommandResult<bool> {
    quick_connect_poll(&server_url, &secret)
        .await
        .map_err(CommandError::msg)
}

/// Complete Quick Connect authentication.
///
/// Exchanges the secret for an access token, then connects like a normal login.
#[tauri::command]
pub async fn quick_connect_complete(
    state: tauri::State<'_, AppState>,
    server_url: String,
    secret: String,
) -> CommandResult<()> {
    let cid = audit::correlation_id();
    audit::log_connection_event("jellyfin", "quick_connect_attempt", &server_url, &cid);

    let credentials = quick_connect_authenticate(&server_url, &secret)
        .await
        .map_err(|e| {
            audit::log_auth_event(
                "jellyfin",
                "quick_connect",
                false,
                audit::error_kind(&e),
                &cid,
            );
            e.to_string()
        })?;

    finalise_connection(&state, credentials, "quick_connect", &cid).await
}

// ---------------------------------------------------------------------------
// API key connection (deprecated)
// ---------------------------------------------------------------------------

/// Connect to a Jellyfin server using an API key.
///
/// **Deprecated**: Use `login_jellyfin` (username/password) or Quick Connect instead.
/// This command is retained for backward compatibility but is no longer exposed in the GUI.
#[tauri::command]
pub async fn connect_jellyfin(
    state: tauri::State<'_, AppState>,
    server_url: String,
    api_key: String,
    user_id: String,
) -> CommandResult<()> {
    let cid = audit::correlation_id();
    audit::log_connection_event("jellyfin", "connect_attempt", &server_url, &cid);

    let credentials = JellyfinCredentials {
        server_url,
        api_key,
        user_id,
    };

    finalise_connection(&state, credentials, "api_key", &cid).await
}

// ---------------------------------------------------------------------------
// Disconnect and status
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Shared dual-backend helpers: build / install / disconnect a backend
// ---------------------------------------------------------------------------

/// Build a client + proxy for `kind` from its stored credentials.
///
/// Returns `Ok(None)` when that backend has no stored credentials. Deliberately
/// does NOT call `test_connection`, so toggling works offline (both libraries
/// are cached). All awaits (the proxy bind) happen while holding NO state locks,
/// preserving the release-before-await discipline.
async fn build_backend(
    state: &tauri::State<'_, AppState>,
    kind: ServerKind,
) -> CommandResult<Option<(MediaClient, proxy::ProxyHandle)>> {
    let auth_cb = state.auth_notifier.get().map(|n| n.callback());
    match kind {
        ServerKind::Jellyfin => {
            let Some(c) = crate::jellyfin::load_credentials()? else {
                return Ok(None);
            };
            let mut client = JellyfinClient::from_credentials(&c)?;
            if let Some(cb) = &auth_cb {
                client.set_auth_expired_callback(cb.clone());
            }
            let device_id = get_or_create_device_id()?;
            let proxy_handle = proxy::start_proxy(
                &c.server_url,
                proxy::ProxyUpstream::Jellyfin {
                    api_key: c.api_key.clone(),
                    device_id,
                },
                auth_cb,
            )
            .await
            .map_err(|e| format!("Failed to start local proxy: {}", e))?;
            Ok(Some((MediaClient::Jellyfin(client), proxy_handle)))
        }
        ServerKind::OpenSubsonic => {
            let Some(c) = crate::mediaserver::subsonic::load_credentials()? else {
                return Ok(None);
            };
            let mut client = SubsonicClient::from_credentials(&c)?;
            if let Some(cb) = &auth_cb {
                client.set_auth_expired_callback(cb.clone());
            }
            let proxy_handle = proxy::start_proxy(
                &c.server_url,
                proxy::ProxyUpstream::Subsonic {
                    username: c.username.clone(),
                    password: c.password.clone(),
                },
                auth_cb,
            )
            .await
            .map_err(|e| format!("Failed to start local proxy: {}", e))?;
            Ok(Some((MediaClient::Subsonic(client), proxy_handle)))
        }
    }
}

/// Atomically swap the live backend to `client`/`proxy` and persist the choice.
///
/// Contains NO HTTP await (only lock-acquire awaits). Overwriting `state.proxy`
/// drops the previous `ProxyHandle`, shutting down the old listener AFTER the
/// new one is already bound — so there is no proxy-less window.
async fn install_backend(
    state: &tauri::State<'_, AppState>,
    client: MediaClient,
    proxy_handle: proxy::ProxyHandle,
    kind: ServerKind,
) {
    {
        let mut m = state.media.write().await;
        *m = Some(client);
    }
    state.set_proxy(proxy_handle).await;
    let _ = state
        .db
        .set_setting(crate::db::settings::keys::SERVER_TYPE, kind.as_str());
    state.db.set_active_source(kind);
    // A fresh live session re-arms the dedup latch so a later expiry emits.
    state.reset_auth_expired();
}

/// Switch which connected backend is live, without re-syncing.
///
/// Loads the target backend's stored credentials, restarts the proxy against it,
/// swaps `state.media`, and flips the DB's active source so library reads
/// re-filter to that backend's cached rows. Errors if the target has no creds.
#[tauri::command]
pub async fn set_active_backend(
    state: tauri::State<'_, AppState>,
    server_type: String,
) -> CommandResult<()> {
    let cid = audit::correlation_id();
    let target = ServerKind::parse(&server_type)
        .ok_or_else(|| format!("Unknown server type: {}", server_type))?;

    let (client, proxy_handle) = build_backend(&state, target)
        .await?
        .ok_or_else(|| format!("{} is not connected", target.as_str()))?;
    install_backend(&state, client, proxy_handle, target).await;

    audit::log_connection_event(target.as_str(), "set_active", "", &cid);
    Ok(())
}

/// Disconnect one backend, promoting the other if the active one was removed.
///
/// Clears ONLY `target`'s credentials (never the other backend). If `target`
/// wasn't the active backend, the live session is untouched. If it was, the
/// other backend is promoted when it still has credentials; otherwise the media
/// slot + proxy are torn down.
///
/// The cached library is deliberately LEFT INTACT on disconnect (as it always
/// has been): wiping it would destroy the offline-available library and
/// orphan already-downloaded audio files on disk (their rows would be gone, so
/// eviction/disk-accounting could never see them). A later reconnect shows the
/// cached library instantly, then a sync refreshes it.
async fn disconnect_backend_inner(
    state: &tauri::State<'_, AppState>,
    target: ServerKind,
    cid: &str,
) -> CommandResult<()> {
    // 1. Clear ONLY this backend's credentials.
    let cleared = match target {
        ServerKind::Jellyfin => clear_credentials(),
        ServerKind::OpenSubsonic => crate::mediaserver::subsonic::clear_credentials(),
    };
    match cleared {
        Ok(()) => audit::log_credential_event(target.as_str(), "clear", true, cid),
        Err(e) => {
            audit::log_credential_event(target.as_str(), "clear", false, cid);
            return Err(e.to_string().into());
        }
    }

    // 2. Non-active backend disconnected → leave the live session untouched.
    let active = state.media.read().await.as_ref().map(|c| c.kind());
    if active != Some(target) {
        audit::log_auth_event(target.as_str(), "disconnect", true, "disconnected", cid);
        return Ok(());
    }

    // 3. Active backend disconnected → promote the other if it still has creds.
    let other = match target {
        ServerKind::Jellyfin => ServerKind::OpenSubsonic,
        ServerKind::OpenSubsonic => ServerKind::Jellyfin,
    };
    match build_backend(state, other).await? {
        Some((client, proxy_handle)) => install_backend(state, client, proxy_handle, other).await,
        None => {
            // Nothing left: tear down. Drop proxy first (shuts down the
            // listener) then the media slot, with no await between.
            {
                let mut p = state.proxy.write().await;
                *p = None;
            }
            {
                let mut m = state.media.write().await;
                *m = None;
            }
            state.reset_auth_expired();
        }
    }

    audit::log_auth_event(target.as_str(), "disconnect", true, "disconnected", cid);
    Ok(())
}

/// Disconnect from the Jellyfin server (clears only Jellyfin credentials).
#[tauri::command]
pub async fn disconnect_jellyfin(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    let cid = audit::correlation_id();
    audit::log_connection_event("jellyfin", "disconnect", "", &cid);
    disconnect_backend_inner(&state, ServerKind::Jellyfin, &cid).await
}

/// Disconnect from the OpenSubsonic server (clears only Subsonic credentials).
#[tauri::command]
pub async fn disconnect_subsonic(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    let cid = audit::correlation_id();
    audit::log_connection_event("subsonic", "disconnect", "", &cid);
    disconnect_backend_inner(&state, ServerKind::OpenSubsonic, &cid).await
}

/// Per-backend connection status: does this backend have stored credentials?
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendConnection {
    /// Independently connectable: credentials are stored for this backend.
    pub connected: bool,
    pub server_url: Option<String>,
}

/// Both backends' connection status plus which one is currently live.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaBackendsStatus {
    /// The live media slot: `jellyfin` | `opensubsonic` | null when none active.
    pub active: Option<String>,
    pub jellyfin: BackendConnection,
    pub opensubsonic: BackendConnection,
}

/// Report both backends' credential presence and the active backend.
///
/// Drives the Connections page (two independent cards + a live active-backend
/// toggle). Credential presence — not a live probe — defines "connected" so the
/// toggle works offline.
#[tauri::command]
pub async fn get_media_backends(
    state: tauri::State<'_, AppState>,
) -> CommandResult<MediaBackendsStatus> {
    let jf = crate::jellyfin::load_credentials().ok().flatten();
    let ss = crate::mediaserver::subsonic::load_credentials()
        .ok()
        .flatten();
    let active = state
        .media
        .read()
        .await
        .as_ref()
        .map(|c| c.kind().as_str().to_string());

    Ok(MediaBackendsStatus {
        active,
        jellyfin: BackendConnection {
            connected: jf.is_some(),
            server_url: jf.map(|c| c.server_url),
        },
        opensubsonic: BackendConnection {
            connected: ss.is_some(),
            server_url: ss.map(|c| c.server_url),
        },
    })
}

/// Get the current Jellyfin connection status.
#[tauri::command]
pub async fn get_jellyfin_status(
    state: tauri::State<'_, AppState>,
) -> CommandResult<ConnectionStatus> {
    let jellyfin_guard = state.media.read().await;

    let status = match jellyfin_guard.as_ref() {
        Some(client) => ConnectionStatus {
            connected: true,
            server_url: Some(client.base_url().to_string()),
            server_type: client.kind().as_str().to_string(),
            insecure: crate::mediaserver::is_cleartext_url(client.base_url()),
        },
        None => ConnectionStatus {
            connected: false,
            server_url: None,
            server_type: crate::mediaserver::ServerKind::Jellyfin
                .as_str()
                .to_string(),
            insecure: false,
        },
    };

    Ok(status)
}

/// Test the connection to the current Jellyfin server.
///
/// Returns an error if not connected or if the connection test fails.
#[tauri::command]
pub async fn test_jellyfin_connection(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    let cid = audit::correlation_id();
    // Clone the client and drop the lock before the network await
    // (release-before-await): a hung connection test must never hold up a
    // disconnect or backend switch that needs the write lock.
    let client = state.get_media().await?;

    match client.test_connection().await {
        Ok(()) => {
            audit::log_auth_event("jellyfin", "connection_test", true, "reachable", &cid);
            Ok(())
        }
        Err(e) => {
            audit::log_auth_event(
                "jellyfin",
                "connection_test",
                false,
                audit::error_kind(&e),
                &cid,
            );
            Err(CommandError::msg(e))
        }
    }
}
