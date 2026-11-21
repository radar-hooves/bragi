//! OpenSubsonic connection command.
//!
//! Unified with the Jellyfin path via `MediaClient` / `AppState.media`: a
//! successful login writes `MediaClient::Subsonic` into the shared media slot,
//! starts the Subsonic-signing proxy, and records the active backend — so
//! library browsing, sync and playback all flow through the same code paths as
//! Jellyfin. Status/disconnect are handled by the shared media commands
//! (`get_jellyfin_status` reports the active backend; `disconnect_jellyfin`
//! clears whichever backend is active).

use crate::audit;
use crate::error::CommandResult;
use crate::mediaserver::subsonic::{store_credentials, SubsonicClient, SubsonicCredentials};
use crate::mediaserver::MediaClient;
use crate::proxy;
use crate::state::AppState;

/// Log in to an OpenSubsonic server (Navidrome, Gonic, Airsonic).
///
/// Verifies the credentials with a `ping` before storing anything, persists them
/// in the `bragi-subsonic` keyring, records `server_type = opensubsonic`, starts
/// the token-signing proxy, and publishes the client to `state.media`.
#[tauri::command]
pub async fn login_subsonic(
    state: tauri::State<'_, AppState>,
    server_url: String,
    username: String,
    password: String,
) -> CommandResult<()> {
    let cid = audit::correlation_id();
    audit::log_connection_event("subsonic", "login_attempt", &server_url, &cid);

    // Build the client (borrows the credentials); verify by pinging first.
    let mut client = SubsonicClient::new(&server_url, &username, &password).map_err(|e| {
        audit::log_auth_event("subsonic", "login", false, audit::error_kind(&e), &cid);
        e.to_string()
    })?;

    // Session-expiry callback, identical to the Jellyfin login path.
    let auth_cb = state.auth_notifier.get().map(|n| n.callback());
    if let Some(cb) = &auth_cb {
        client.set_auth_expired_callback(cb.clone());
    }

    if let Err(e) = client.test_connection().await {
        audit::log_auth_event(
            "subsonic",
            "connection_test",
            false,
            audit::error_kind(&e),
            &cid,
        );
        return Err(e.to_string().into());
    }

    // Cleartext (http://) LAN parity is allowed (mirrors Jellyfin), but a
    // non-loopback http server means the non-expiring salted token and all audio
    // travel unencrypted — record it.
    if crate::mediaserver::is_cleartext_url(client.base_url()) {
        audit::log_connection_event(
            "subsonic",
            "cleartext_connection",
            client.base_url().as_str(),
            &cid,
        );
    }

    // Persist credentials (store the normalised base URL, not the raw input).
    let base = client.base_url().to_string();
    let creds = SubsonicCredentials {
        server_url: base.clone(),
        username: username.clone(),
        password: password.clone(),
    };
    match store_credentials(&creds) {
        Ok(()) => audit::log_credential_event("subsonic", "store", true, &cid),
        Err(e) => {
            audit::log_credential_event("subsonic", "store", false, &cid);
            return Err(e.to_string().into());
        }
    }

    // Record the active backend so startup/state-load restores OpenSubsonic.
    // Keep the persisted SERVER_TYPE and the in-memory read filter in lock-step.
    let _ = state
        .db
        .set_setting(crate::db::settings::keys::SERVER_TYPE, "opensubsonic");
    state
        .db
        .set_active_source(crate::mediaserver::ServerKind::OpenSubsonic);

    // Start the Subsonic-signing proxy before moving the client into shared
    // state (the proxy owns its own copy of the credentials for query signing).
    let proxy_handle = proxy::start_proxy(
        &base,
        proxy::ProxyUpstream::Subsonic { username, password },
        auth_cb,
    )
    .await
    .map_err(|e| format!("Failed to start local proxy: {}", e))?;

    {
        let mut media_guard = state.media.write().await;
        *media_guard = Some(MediaClient::Subsonic(client));
    }
    state.reset_auth_expired();
    state.set_proxy(proxy_handle).await;

    audit::log_auth_event("subsonic", "connect", true, "connected", &cid);
    Ok(())
}
