//! Application state management for Bragi.
//!
//! Holds shared state accessible across Tauri commands.

use std::sync::Arc;
use tokio::sync::RwLock;

use std::path::PathBuf;

use crate::audio::AudioEngineHandle;
use crate::db::Database;
use crate::downloads::DownloadManagerHandle;
use crate::jellyfin::JellyfinClient;
use crate::mediaserver::subsonic::SubsonicClient;
use crate::mediaserver::{MediaClient, ServerKind};
use crate::models::{Album, Artist, Track};
use crate::proxy::ProxyHandle;
use crate::services::MusicServiceClient;

/// Application state shared across all commands.
///
/// Uses `Arc<RwLock<Option<T>>>` pattern for optional, lazily-initialised
/// clients that may be configured after app startup.
pub struct AppState {
    /// Active media-server client — Jellyfin or OpenSubsonic, initialised
    /// when the user configures a server. Enum-dispatched via `MediaClient`.
    pub media: Arc<RwLock<Option<MediaClient>>>,
    /// Music service client for scrobbling and listening history.
    pub music_service: Arc<RwLock<Option<MusicServiceClient>>>,
    /// Audio engine handle for controlling playback.
    /// Optional because audio is initialised after frontend signals ready.
    pub audio: Arc<RwLock<Option<AudioEngineHandle>>>,
    /// Local SQLite database for caching and offline support.
    pub db: Arc<Database>,
    /// Download manager for offline audio caching.
    pub download_manager: Arc<RwLock<Option<DownloadManagerHandle>>>,
    /// Local proxy for Jellyfin credential injection.
    pub proxy: Arc<RwLock<Option<ProxyHandle>>>,
    /// Directory for locally cached artwork images.
    pub artwork_cache_dir: PathBuf,
    /// Shared one-shot latch deduplicating session-expiry (401) events.
    ///
    /// Cloned into the Jellyfin client and proxy; reconnect resets it here.
    pub auth_guard: crate::auth_notify::AuthExpiredGuard,
    /// Emitter for the `jellyfin-auth-expired` event, installed once the
    /// `AppHandle` exists (setup hook). Reads via `OnceLock` are lock-free.
    pub auth_notifier: std::sync::OnceLock<crate::auth_notify::AuthExpiryNotifier>,
    /// App handle for emitting frontend events from the sync path,
    /// installed once it exists (setup hook). `perform_sync_for_client` only
    /// receives `&AppState`, so the detached playlist-prefetch task reads this
    /// lock-free to signal the frontend that the playlist cache is warm.
    pub app_handle: std::sync::OnceLock<tauri::AppHandle>,
    /// Close-to-tray preference. Lock-free cache read by the window-event
    /// handler on every close; kept in sync by `update_settings`.
    pub close_to_tray: std::sync::atomic::AtomicBool,
    /// Minimise-to-tray preference. Lock-free cache read by the
    /// window-event handler on frequent `Resized` events.
    pub minimise_to_tray: std::sync::atomic::AtomicBool,
    /// Automatic network-reachability signal (sync/offline/downloads): distinct from the user's manual `offline_mode` setting, which
    /// promises no background network activity and is never overridden by
    /// this. Updated by the sync scheduler's periodic reachability probe
    /// (mirrors Spotify's "downloads play automatically when you lose
    /// internet" — an automatic fallback signal alongside, not instead of, the
    /// user's own offline toggle). Optimistic default (`true`) so a cold start
    /// never shows a false "offline" state before the first probe completes.
    pub network_reachable: std::sync::atomic::AtomicBool,
    /// Per-backend single-flight guards for library sync.
    ///
    /// A non-blocking `try_lock` at the top of `perform_sync_for_client`
    /// coalesces any concurrent trigger for the SAME backend (manual+manual,
    /// manual+scheduled, first-load+reconnect) into a benign no-op, instead of
    /// running a second full ~2000-request scan that doubles the load on that
    /// host. Keyed per `ServerKind`: Jellyfin and OpenSubsonic are
    /// different hosts, so each single-flights independently — a long background
    /// sync of the inactive backend must not coalesce the user's manual sync of
    /// the active one into a no-op.
    sync_lock_jellyfin: Arc<tokio::sync::Mutex<()>>,
    sync_lock_opensubsonic: Arc<tokio::sync::Mutex<()>>,
}

impl AppState {
    /// Create a new AppState instance without audio engine.
    ///
    /// Audio engine will be initialised later when the frontend signals ready.
    /// This avoids race conditions with macOS CoreAudio during app startup.
    pub fn new_without_audio(db: Database, artwork_cache_dir: PathBuf) -> Self {
        let startup_cid = crate::audit::correlation_id();

        // Load the active media-server client, gated on the persisted backend
        // type. Absent key defaults to Jellyfin so existing installs (which have
        // `bragi-jellyfin` creds but no server_type) keep their session.
        let server_type = db
            .get_setting(crate::db::settings::keys::SERVER_TYPE)
            .ok()
            .flatten()
            .and_then(|s| ServerKind::parse(&s))
            .unwrap_or(ServerKind::Jellyfin);

        let media: Option<MediaClient> = match server_type {
            ServerKind::OpenSubsonic => match crate::mediaserver::subsonic::load_credentials() {
                Ok(Some(credentials)) => match SubsonicClient::from_credentials(&credentials) {
                    Ok(client) => {
                        crate::audit::log_credential_event("subsonic", "load", true, &startup_cid);
                        Some(MediaClient::Subsonic(client))
                    }
                    Err(e) => {
                        crate::audit::log_credential_event("subsonic", "load", false, &startup_cid);
                        tracing::warn!("Failed to create Subsonic client: {}", e);
                        None
                    }
                },
                Ok(None) => {
                    tracing::info!("No Subsonic credentials stored");
                    None
                }
                Err(e) => {
                    crate::audit::log_credential_event("subsonic", "load", false, &startup_cid);
                    tracing::warn!("Failed to load Subsonic credentials: {}", e);
                    None
                }
            },
            ServerKind::Jellyfin => match crate::jellyfin::load_credentials() {
                Ok(Some(credentials)) => match JellyfinClient::from_credentials(&credentials) {
                    Ok(client) => {
                        crate::audit::log_credential_event("jellyfin", "load", true, &startup_cid);
                        Some(MediaClient::Jellyfin(client))
                    }
                    Err(e) => {
                        crate::audit::log_credential_event("jellyfin", "load", false, &startup_cid);
                        tracing::warn!(
                            "Failed to create Jellyfin client from stored credentials: {}",
                            e
                        );
                        None
                    }
                },
                Ok(None) => {
                    tracing::info!("No Jellyfin credentials stored");
                    None
                }
                Err(e) => {
                    crate::audit::log_credential_event("jellyfin", "load", false, &startup_cid);
                    tracing::warn!("Failed to load Jellyfin credentials: {}", e);
                    None
                }
            },
        };

        // Load music service client
        let music_service = match crate::services::load_music_service_credentials() {
            Ok(Some(credentials)) => match MusicServiceClient::from_credentials(&credentials) {
                Ok(client) => {
                    crate::audit::log_credential_event("music_service", "load", true, &startup_cid);
                    Some(client)
                }
                Err(e) => {
                    crate::audit::log_credential_event(
                        "music_service",
                        "load",
                        false,
                        &startup_cid,
                    );
                    tracing::warn!(
                        "Failed to create music service client from stored credentials: {}",
                        e
                    );
                    None
                }
            },
            Ok(None) => {
                tracing::info!("No music service credentials stored");
                None
            }
            Err(e) => {
                crate::audit::log_credential_event("music_service", "load", false, &startup_cid);
                tracing::warn!("Failed to load music service credentials: {}", e);
                None
            }
        };

        // Seed the tray-behaviour caches from the DB before `db` is moved into
        // the Arc. close_to_tray defaults to true (preserves the historical
        // always-hide-on-close behaviour); minimise_to_tray defaults to false.
        use crate::db::settings::keys;
        let close_to_tray = db
            .get_parsed(keys::CLOSE_TO_TRAY)
            .ok()
            .flatten()
            .unwrap_or(true);
        let minimise_to_tray = db
            .get_parsed(keys::MINIMISE_TO_TRAY)
            .ok()
            .flatten()
            .unwrap_or(false);

        Self {
            media: Arc::new(RwLock::new(media)),
            music_service: Arc::new(RwLock::new(music_service)),
            audio: Arc::new(RwLock::new(None)),
            db: Arc::new(db),
            download_manager: Arc::new(RwLock::new(None)),
            proxy: Arc::new(RwLock::new(None)),
            artwork_cache_dir,
            auth_guard: crate::auth_notify::AuthExpiredGuard::new(),
            auth_notifier: std::sync::OnceLock::new(),
            app_handle: std::sync::OnceLock::new(),
            close_to_tray: std::sync::atomic::AtomicBool::new(close_to_tray),
            minimise_to_tray: std::sync::atomic::AtomicBool::new(minimise_to_tray),
            network_reachable: std::sync::atomic::AtomicBool::new(true),
            sync_lock_jellyfin: Arc::new(tokio::sync::Mutex::new(())),
            sync_lock_opensubsonic: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// The single-flight sync guard for `kind`.
    ///
    /// Each backend has its own guard so the two hosts single-flight
    /// independently: a background sync of the inactive backend never coalesces
    /// the user's manual sync of the active backend into a no-op.
    pub fn sync_lock_for(&self, kind: ServerKind) -> Arc<tokio::sync::Mutex<()>> {
        match kind {
            ServerKind::Jellyfin => self.sync_lock_jellyfin.clone(),
            ServerKind::OpenSubsonic => self.sync_lock_opensubsonic.clone(),
        }
    }

    /// Install the session-expiry notifier once the `AppHandle` is available.
    pub fn init_auth_notifier(&self, notifier: crate::auth_notify::AuthExpiryNotifier) {
        let _ = self.auth_notifier.set(notifier);
    }

    /// Install the app handle once it is available (setup hook), so background
    /// tasks holding only `&AppState` can emit frontend events.
    pub fn init_app_handle(&self, handle: tauri::AppHandle) {
        let _ = self.app_handle.set(handle);
    }

    /// The app handle for emitting frontend events, if installed. Lock-free.
    pub fn app_handle(&self) -> Option<&tauri::AppHandle> {
        self.app_handle.get()
    }

    /// Re-arm the auth-expiry latch so a future 401 emits a fresh event.
    ///
    /// Called after a successful (re)connect and on disconnect, so a new
    /// session resumes surfacing expiry prompts.
    pub fn reset_auth_expired(&self) {
        self.auth_guard.reset();
    }

    /// Set the download manager handle.
    pub async fn set_download_manager(&self, manager: DownloadManagerHandle) {
        let mut dm = self.download_manager.write().await;
        *dm = Some(manager);
    }

    /// Set the local proxy handle.
    pub async fn set_proxy(&self, proxy: ProxyHandle) {
        let mut p = self.proxy.write().await;
        *p = Some(proxy);
    }

    /// Directory for cached waveform peak files.
    ///
    /// A sibling of the artwork cache under the app cache root, so all
    /// per-track precomputed blobs live together.
    pub fn waveform_cache_dir(&self) -> PathBuf {
        self.artwork_cache_dir
            .parent()
            .map(|p| p.join("waveforms"))
            .unwrap_or_else(|| PathBuf::from("waveforms"))
    }

    /// The connected media-server client, cloned out so the lock is released
    /// before any await on the network (a slow server must never hold up a
    /// reconnect or backend switch that needs the write lock).
    pub async fn get_media(&self) -> Result<MediaClient, String> {
        (*self.media.read().await)
            .clone()
            .ok_or_else(|| "Not connected to Jellyfin server".to_string())
    }

    /// The download manager handle, returning an error if not initialised.
    pub async fn get_download_manager(&self) -> Result<DownloadManagerHandle, String> {
        (*self.download_manager.read().await)
            .clone()
            .ok_or_else(|| "Download manager not initialised".to_string())
    }

    /// Rewrite artwork references to live proxy URLs (the OpenSubsonic cover
    /// sentinel becomes a `/cover` proxy URL; a Jellyfin URL passes through).
    /// A no-op while the proxy is not yet started.
    pub async fn resolve_albums_artwork(&self, albums: &mut [Album]) {
        if let Ok(port) = self.proxy_port().await {
            crate::mediaserver::client::resolve_albums_artwork(albums, port);
        }
    }

    /// See [`Self::resolve_albums_artwork`].
    pub async fn resolve_artists_artwork(&self, artists: &mut [Artist]) {
        if let Ok(port) = self.proxy_port().await {
            crate::mediaserver::client::resolve_artists_artwork(artists, port);
        }
    }

    /// See [`Self::resolve_albums_artwork`].
    pub async fn resolve_tracks_artwork(&self, tracks: &mut [Track]) {
        if let Ok(port) = self.proxy_port().await {
            crate::mediaserver::client::resolve_tracks_artwork(tracks, port);
        }
    }

    /// Get the proxy port, returning an error if not started.
    pub async fn proxy_port(&self) -> Result<u16, String> {
        self.proxy
            .read()
            .await
            .as_ref()
            .map(|p| p.port())
            .ok_or_else(|| "Local proxy not yet started".to_string())
    }

    /// Read the proxy port and per-session auth token in one lock acquisition.
    ///
    /// Centralises the token plumbing so stream/download callers don't each
    /// fetch the port and token separately. The token is required on every
    /// `/stream` and `/download` request; it lives only in the `ProxyHandle`.
    pub async fn proxy_endpoint(&self) -> Result<(u16, String), String> {
        self.proxy
            .read()
            .await
            .as_ref()
            .map(|p| (p.port(), p.token().to_string()))
            .ok_or_else(|| "Local proxy not yet started".to_string())
    }

    /// Get the audio engine handle, returning an error if not initialised.
    pub async fn get_audio(&self) -> Result<AudioEngineHandle, String> {
        self.audio.read().await.clone().ok_or_else(|| {
            "Audio engine not yet initialised. Please wait for the app to fully load.".to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> AppState {
        let db = Database::open_in_memory().unwrap();
        AppState::new_without_audio(db, std::env::temp_dir())
    }

    /// The single-flight invariant is now per backend: the two
    /// backends' guards are independent (both can be held at once), while a
    /// second `try_lock` of the SAME backend's guard coalesces (fails).
    #[tokio::test]
    async fn sync_locks_are_independent_per_backend() {
        let state = test_state();

        let jf = state.sync_lock_for(ServerKind::Jellyfin);
        let os = state.sync_lock_for(ServerKind::OpenSubsonic);

        // Hold the Jellyfin guard.
        let _jf_held = jf.try_lock().expect("Jellyfin lock should be free");

        // The OpenSubsonic guard is independent and still acquirable.
        assert!(
            os.try_lock().is_ok(),
            "the two backends must single-flight independently"
        );

        // A second acquisition of the SAME backend's guard must coalesce.
        let jf2 = state.sync_lock_for(ServerKind::Jellyfin);
        assert!(
            jf2.try_lock().is_err(),
            "the single-flight guard must hold within a backend"
        );
    }
}
