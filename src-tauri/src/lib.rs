//! Bragi - Desktop music player application.
//!
//! This is the main entry point for the Tauri application.

mod app_id;
mod artwork;
mod audio;
mod audit;
mod auth_notify;
mod commands;
mod credential_store;
mod db;
mod downloads;
mod error;
mod genres;
mod http;
mod jellyfin;
mod llm;
#[cfg(target_os = "macos")]
mod media_keys;
mod mediaserver;
mod models;
mod notifications;
mod ordering;
mod proxy;
mod services;
mod state;
mod telemetry_store;
mod tray;
mod waveform;
mod window;

#[cfg(target_os = "macos")]
use audio::DeviceWatcher;
use audio::{start_audio_engine, AudioCommand, AudioEvent};
use db::{get_database_path, Database};
use downloads::{start_download_manager, DownloadEvent};
use jellyfin::get_or_create_device_id;
use state::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Listener, Manager};
use tauri_plugin_telemetry::TelemetryStore;

/// This app's OWN extra `tracing` targets, handed to
/// `tauri_plugin_telemetry::init`'s `allow` argument: every target beyond what
/// the `telemetry` crate already force-allows for itself (`telemetry::panic`,
/// `telemetry::client`, `telemetry::process` — a crash, a caught error, an
/// HTTP client span and the process sampler) and beyond the plugin's own
/// command-span target, which `init` adds automatically. A curated few
/// commands carry an argument that explains the work — a library query, a
/// track played, a playlist touched — because the user's own listening history
/// may reach the user's own stack; see
/// `tauri_plugin_telemetry`'s own `span` module doc for which ones and why a
/// credential-bearing command never does.
const ALLOWED_TARGETS: &[&str] = &[audit::TARGET, commands::SCROBBLE_TARGET, audio::TARGET];

/// Holds the device watcher handle for graceful shutdown.
#[cfg(target_os = "macos")]
struct DeviceWatcherHandle(Mutex<Option<DeviceWatcher>>);

/// Whether a `GDK_BACKEND`-shaped value requests Wayland.
///
/// `GDK_BACKEND` is a comma-separated priority list (GDK tries each token in
/// order until one succeeds), so `"wayland,x11"` still opens a Wayland display
/// first and hits the same broken-rendering path as a bare `"wayland"`.
/// A token match — not exact-string equality — is therefore required. Tokens
/// are trimmed for stray whitespace; the comparison is case-sensitive because
/// GDK's own backend names are lowercase by convention.
///
/// Pulled out of the `#[cfg(target_os = "linux")]`-gated [`ensure_x11_backend`]
/// so this pure string logic is unit-tested on every platform, not just Linux.
#[cfg(any(test, target_os = "linux"))]
fn gdk_backend_requests_wayland(value: &str) -> bool {
    value.split(',').any(|token| token.trim() == "wayland")
}

/// Linux/WebKitGTK rendering workaround. GTK reads `GDK_BACKEND` when its display
/// is opened, which happens before any of our Rust runs (a plain `set_var` — even
/// from a `#[ctor]` — is too late: GDK has already bound the backend). The only
/// reliable way to get the var into the process *from birth* is to re-exec
/// ourselves once with it set. Baked into the binary so EVERY path gets it:
/// release .deb/AppImage from `tauri build`, the Nix package consumed as a flake
/// input (system-wide / home-manager), and `pnpm tauri dev`.
///
/// Crucially we must escape a Wayland request in `GDK_BACKEND` too, not just an
/// unset value: Wayland desktop sessions (Hyprland, etc.) routinely export it
/// globally, and native Wayland is exactly the broken path. A Wayland request is
/// not only the exact value `"wayland"` — GDK accepts a comma-separated priority
/// list, and `"wayland,x11"` still tries Wayland first — so we force x11 when the
/// var is unset OR [`gdk_backend_requests_wayland`] finds a `wayland` token in it,
/// and only respect an explicit, entirely non-Wayland value (e.g. a user who
/// already forced x11, or the Nix wrapper's `--set`). `BRAGI_ALLOW_WAYLAND` is the
/// escape hatch for anyone who's fixed WebKitGTK and wants Wayland back.
///
/// Why x11: WebKitGTK 2.4x/GTK3 renders BROKEN under native Wayland *fractional*
/// scaling on NVIDIA — the webview viewport resolves to an infinite size (JS
/// layout metrics come back as 2^25/garbage), so the album grid explodes and the
/// app shell never paints. Independent of the WebKitGTK version (2.48.3 / 2.50.5
/// / 2.52.4 all break) and the app code (unchanged since April 2026) — a host
/// GTK/Mesa/compositor update triggered it. XWayland sidesteps fractional scaling
/// and still renders on the GPU (WebKit uses GL either way; only the windowing
/// protocol changes). WEBKIT_DISABLE_DMABUF_RENDERER avoids the NVIDIA/Wayland
/// "Error 71" DMABUF crash.
///
/// Removal criteria: this whole workaround can be dropped once the upstream
/// WebKitGTK/GTK <-> compositor fractional-scaling bug is fixed (tracked
/// upstream as wry#1727); until then it is deliberate, permanent behaviour,
/// not deferred work.
#[cfg(target_os = "linux")]
fn ensure_x11_backend() {
    use std::os::unix::process::CommandExt;

    // Explicit opt-out for anyone who's fixed WebKitGTK and wants Wayland back.
    if std::env::var_os("BRAGI_ALLOW_WAYLAND").is_some() {
        return;
    }
    // Respect an explicit backend list with no Wayland token in it; but unset
    // OR any list containing a "wayland" token (e.g. "wayland,x11") falls
    // through to the re-exec (see doc comment).
    if let Ok(v) = std::env::var("GDK_BACKEND") {
        if !v.is_empty() && !gdk_backend_requests_wayland(&v) {
            return;
        }
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.args(std::env::args_os().skip(1));
    cmd.env("GDK_BACKEND", "x11");
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        cmd.env("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    // Replaces this process image; the child starts fresh with the vars already
    // in its environment (before GTK initialises), and won't re-exec again
    // because it now sees GDK_BACKEND=x11. Only returns on failure — in which
    // case we fall through and run anyway (best effort).
    let _ = cmd.exec();
}

/// Run the Tauri application.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    ensure_x11_backend();

    // Copies the directories the previous bundle identifier owned, before
    // anything below opens the new ones.
    app_id::migrate_legacy_dirs();

    // Resolve the artwork cache dir before building so it can be captured
    // by the custom protocol handler (which must be registered on the builder).
    let artwork_cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from(".cache"))
        .join(app_id::APP_ID)
        .join("artwork");

    // Filled in by the setup hook below once the database is open: this
    // plugin's own setup hook runs before the app's own (Tauri runs every
    // plugin's setup first), so `telemetry_store::BragiTelemetryStore` cannot
    // hold the database itself — see its module doc.
    let telemetry_db: telemetry_store::DbCell = Arc::new(std::sync::OnceLock::new());

    // Single instance first; a second launch raises the active window, and
    // window-state saves and restores `main` alone (the MiniPlayer is built
    // at its own size).
    let mut builder = desktop_shell::window::plugins_with(
        tauri::Builder::default(),
        window::show_active,
        |label| label == window::MAIN_LABEL,
    )
    .plugin(tauri_plugin_telemetry::init(
        "bragi",
        env!("CARGO_PKG_VERSION"),
        ALLOWED_TARGETS,
        telemetry_store::BragiTelemetryStore(telemetry_db.clone()),
    ))
    .plugin(tauri_plugin_opener::init())
    .plugin(tauri_plugin_global_shortcut::Builder::new().build())
    .plugin(tauri_plugin_notification::init())
    // Launch-on-login. LaunchAgent on macOS (no Automation prompt,
    // unlike the AppleScript launcher). Rust-side app.autolaunch() needs no
    // capability grant; the frontend stays invoke()-only via a command.
    .plugin(tauri_plugin_autostart::init(
        tauri_plugin_autostart::MacosLauncher::LaunchAgent,
        None,
    ));

    builder = artwork::register_artwork_protocol(builder, artwork_cache_dir.clone());

    // Bragi updates itself from its own signed releases: the key is
    // `plugins.updater` in tauri.conf.json.
    #[cfg(desktop)]
    {
        builder = builder
            .plugin(tauri_plugin_updater::Builder::new().build())
            .plugin(tauri_plugin_process::init());
    }

    #[cfg(target_os = "macos")]
    {
        builder = builder.manage(DeviceWatcherHandle(Mutex::new(None)));
    }

    builder
        .setup(move |app| {
            // `tauri_plugin_telemetry::init` (registered on the builder chain
            // above) already installed the whole subscriber, in its own setup
            // hook, which Tauri runs before this one: the process sampler, the
            // Guard drop on `RunEvent::Exit`, and the allow-listed command
            // spans are all its responsibility now. Nothing here configures
            // tracing beyond `ALLOWED_TARGETS`, handed to that `init` call.
            tracing::info!("Starting Bragi...");

            #[cfg(debug_assertions)]
            tracing::info!(
                "Using file-based credential store (<data dir>/com.poodle64.bragi/credentials.json)"
            );
            #[cfg(not(debug_assertions))]
            tracing::info!("Using native system keyring for credential storage");

            // Initialise the database
            let db_path = get_database_path(app.handle())
                .map_err(|e| format!("Failed to get database path: {}", e))?;
            let database = Database::open(db_path)
                .map_err(|e| format!("Failed to open database: {}", e))?;
            tracing::info!("Database initialised");

            // A setup still on the retired Claudette preset moves onto the
            // OpenAI-compatible provider; on failure it is tried again next launch.
            if let Err(e) = llm::migrate::migrate_claudette(&database) {
                telemetry::report_error_with_cause("Claudette preset migration failed", &e);
            }

            // Create app state with database only (no audio yet)
            let state = AppState::new_without_audio(database, artwork_cache_dir);
            app.manage(state);
            tracing::info!("App state initialised");

            // Hand the telemetry store its database handle now it exists —
            // see `telemetry_store`'s module doc for why the store carries an
            // initially-empty cell rather than the database directly.
            let _ = telemetry_db.set(app.state::<AppState>().db.clone());

            // Wire up the deduplicated Jellyfin session-expiry notifier.
            // Built after manage() so it clones the managed AppState's shared
            // guard; the Jellyfin client and proxy fire this notifier on a 401.
            {
                let s = app.state::<AppState>();
                let notifier = auth_notify::AuthExpiryNotifier::new(
                    app.handle().clone(),
                    s.auth_guard.clone(),
                );
                s.init_auth_notifier(notifier);
                // Give background tasks (the playlist-cache prefetch) a
                // handle to emit frontend events from paths that only hold
                // `&AppState`.
                s.init_app_handle(app.handle().clone());
            }

            // Point the exporter at this machine's saved collector, now the
            // database that holds it is open. A Dock-launched app inherits no
            // fleet environment, so the Settings pane is the only door on a
            // machine the fleet does not configure; where the environment did
            // set OTEL_EXPORTER_OTLP_ENDPOINT, this is a no-op and it wins.
            tauri_plugin_telemetry::apply_saved(
                app.handle(),
                telemetry_store::BragiTelemetryStore(telemetry_db.clone()).load(),
            );

            // Initialise notification state for track change notifications.
            app.manage(notifications::NotificationState::new());

            // Register global media key shortcuts.
            // These let the user control playback via keyboard media keys
            // even when the app is not focused.
            //
            // On macOS, media keys (especially next/previous) require the app
            // to have Accessibility permission (System Settings > Privacy &
            // Security > Accessibility). Registration is non-fatal so the app
            // still starts without it — only media keys will be unavailable.
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

                let audio_for_shortcuts = app.state::<AppState>().audio.clone();
                let gs = app.global_shortcut();

                let media_keys: &[(&str, AudioCommand)] = &[
                    ("MediaPlayPause", AudioCommand::TogglePlayPause),
                    ("MediaTrackNext", AudioCommand::SkipNext),
                    ("MediaTrackPrev", AudioCommand::SkipPrevious),
                ];

                let mut registered = 0u32;
                for (key_name, command) in media_keys {
                    let audio = audio_for_shortcuts.clone();
                    let cmd = command.clone();
                    let name = *key_name;
                    match gs.on_shortcut(*key_name, move |_app, _shortcut, event| {
                        tracing::debug!("Media key event: {} state={:?}", name, event.state);
                        if event.state == ShortcutState::Pressed {
                            if let Ok(guard) = audio.try_read() {
                                if let Some(handle) = guard.as_ref() {
                                    let _ = handle.send(cmd.clone());
                                }
                            }
                        }
                    }) {
                        Ok(()) => {
                            tracing::info!("Registered media key: {}", key_name);
                            registered += 1;
                        }
                        Err(e) => tracing::warn!(
                            "Failed to register {} media key (Accessibility permission may be required): {}",
                            key_name,
                            e,
                        ),
                    }
                }

                if registered == media_keys.len() as u32 {
                    tracing::info!("All media key shortcuts registered");
                } else {
                    tracing::warn!(
                        "Only {}/{} media key shortcuts registered. \
                         On macOS, grant Accessibility permission to Bragi in \
                         System Settings > Privacy & Security > Accessibility",
                        registered,
                        media_keys.len(),
                    );
                }
            }

            // Register with macOS MPRemoteCommandCenter for next/previous
            // track keys. These are delivered via the Now Playing system, not
            // via CGEventTap, so the global-shortcut plugin cannot intercept them.
            #[cfg(target_os = "macos")]
            {
                let audio_for_media = app.state::<AppState>().audio.clone();
                match media_keys::register_media_commands(audio_for_media) {
                    Ok(handle) => {
                        // Store handle in Tauri managed state to keep it alive
                        app.manage(handle);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to register MPRemoteCommandCenter: {}", e);
                    }
                }
            }

            // Start local proxy if Jellyfin credentials are available.
            // The proxy injects auth headers so API keys never appear in URLs.
            let proxy_app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = proxy_app_handle.state::<AppState>();
                let auth_cb = state.auth_notifier.get().map(|n| n.callback());

                // Install the session-expiry callback on the startup-loaded
                // client (it was built before the AppHandle existed, so its
                // callback is None) and copy out the URL/key. Take the write
                // guard, mutate, clone owned strings, then DROP the guard before
                // any await — preserving the release-before-await discipline.
                let (base_url, upstream) = {
                    let mut guard = state.media.write().await;
                    match guard.as_mut() {
                        Some(client) => {
                            if let Some(cb) = &auth_cb {
                                client.set_auth_expired_callback(cb.clone());
                            }
                            let base = client.base_url().as_str().to_string();
                            let upstream = match &*client {
                                mediaserver::MediaClient::Jellyfin(c) => {
                                    let device_id = match get_or_create_device_id() {
                                        Ok(id) => id,
                                        Err(e) => {
                                            tracing::error!(
                                                "Failed to get device ID for proxy: {}",
                                                e
                                            );
                                            return;
                                        }
                                    };
                                    proxy::ProxyUpstream::Jellyfin {
                                        api_key: c.api_key().to_string(),
                                        device_id,
                                    }
                                }
                                mediaserver::MediaClient::Subsonic(c) => {
                                    proxy::ProxyUpstream::Subsonic {
                                        username: c.username().to_string(),
                                        password: c.password().to_string(),
                                    }
                                }
                            };
                            (base, upstream)
                        }
                        None => return,
                    }
                };

                match proxy::start_proxy(&base_url, upstream, auth_cb).await {
                    Ok(handle) => {
                        tracing::info!("Local proxy started on port {}", handle.port());
                        state.set_proxy(handle).await;
                    }
                    Err(e) => {
                        tracing::error!("Failed to start local proxy: {}", e);
                    }
                }
            });

            // Set up the system tray icon with playback controls. A missing
            // appindicator library on Linux panics rather than errors; the kit
            // catches it so the app starts without a tray.
            desktop_shell::guard_tray(|| tray::setup_tray(app.handle()));

            // macOS: AppKit centres the traffic lights in the top bar itself.
            #[cfg(target_os = "macos")]
            if let Some(window) = app.get_webview_window(window::MAIN_LABEL) {
                desktop_shell::traffic_lights::setup(&window);
            }

            // Reconcile launch-on-login: the OS is the source of truth.
            // If the user removed the .desktop/plist externally, mirror that back
            // into the DB so the Settings toggle reflects reality. On error we
            // leave the DB value untouched (do NOT clobber to false).
            {
                use tauri_plugin_autostart::ManagerExt;
                match app.autolaunch().is_enabled() {
                    Ok(enabled) => {
                        let _ = app.state::<AppState>().db.set_setting(
                            db::settings::keys::STARTUP_LAUNCH_ON_LOGIN,
                            if enabled { "true" } else { "false" },
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Failed to query autostart state: {}", e);
                    }
                }
            }

            // Start minimised to tray: if the setting is on, hide the
            // window that config marks visible:true. Any read/parse failure must
            // fall through to SHOWING the window (unwrap_or(false)) — a hidden
            // window with no obvious tray on some DEs looks like a dead app.
            let start_minimised = app
                .state::<AppState>()
                .db
                .get_setting(db::settings::keys::START_MINIMISED)
                .ok()
                .flatten()
                .and_then(|v| v.parse::<bool>().ok())
                .unwrap_or(false);
            // Fits the restored frame to the displays, hides on close when
            // close-to-tray is on (macOS always, so ⌘W never destroys `main`
            // and the Dock and tray can bring it back), hides for a minimised
            // start.
            desktop_shell::window::setup_with(
                app.handle(),
                window::MAIN_LABEL,
                start_minimised,
                window::MAIN_MIN_SIZE,
                |window| {
                    cfg!(target_os = "macos")
                        || window
                            .try_state::<AppState>()
                            .map(|st| st.close_to_tray.load(Ordering::Relaxed))
                            .unwrap_or(true)
                },
            );

            // Flag to ensure audio is only initialised once
            let audio_initialized = Arc::new(AtomicBool::new(false));

            // Listen for frontend-ready event to initialise audio
            // This defers audio engine startup until after macOS app delegate has finished
            let app_handle = app.handle().clone();
            let audio_init_flag = audio_initialized.clone();
            app.listen("frontend-ready", move |_event| {
                // Only initialise once
                if audio_init_flag.swap(true, Ordering::SeqCst) {
                    tracing::debug!("Audio already initialised, ignoring duplicate frontend-ready");
                    return;
                }

                tracing::info!("Frontend ready - initialising audio engine");

                let app_for_audio = app_handle.clone();
                let audio_handle = match start_audio_engine(move |event| {
                    emit_audio_event(&app_for_audio, event);
                }) {
                    Ok(result) => result,
                    Err(e) => {
                        tracing::error!("Failed to start audio engine: {}", e);
                        return;
                    }
                };

                // Store audio handle in app state via Tauri's async runtime.
                // Also configure the audio engine with Jellyfin auth header if available.
                let state = app_handle.state::<AppState>();
                let audio_lock = state.audio.clone();
                let db_for_eq = state.db.clone();
                tauri::async_runtime::spawn(async move {
                    // Replay the user's saved EQ into the fresh engine before the
                    // handle is published, so playback honours a persisted EQ
                    // immediately instead of only after Settings > Playback is opened.
                    commands::apply_persisted_eq(&db_for_eq, &audio_handle);
                    // Replay the persisted fade-on-pause/stop settings too.
                    commands::apply_persisted_fade(&db_for_eq, &audio_handle);

                    // Store the audio handle
                    let mut lock = audio_lock.write().await;
                    *lock = Some(audio_handle);
                });

                tracing::info!("Audio engine initialised successfully");

                // Start device watcher to detect audio device changes (macOS only)
                #[cfg(target_os = "macos")]
                {
                    let watcher_app_handle = app_handle.clone();
                    let device_watcher = DeviceWatcher::start(move |devices| {
                        emit_audio_event(
                            &watcher_app_handle,
                            AudioEvent::DevicesChanged { devices },
                        );
                    });

                    if let Some(handle) = app_handle.try_state::<DeviceWatcherHandle>() {
                        if let Ok(mut guard) = handle.0.lock() {
                            *guard = Some(device_watcher);
                        }
                    }
                    tracing::info!("Audio device watcher started");
                }

                // Initialise download manager.
                // Uses tauri::async_runtime::spawn so the download manager's
                // background task lives on Tauri's persistent tokio runtime.
                // A temporary std::thread + Runtime would drop after block_on
                // completes, killing the spawned download loop.
                let dm_app_handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    let state = dm_app_handle.state::<AppState>();

                    // Resolve cache directory
                    let cache_dir = dm_app_handle
                        .path()
                        .app_cache_dir()
                        .unwrap_or_else(|_| std::path::PathBuf::from(".cache"))
                        .join("audio");

                    let db = state.db.clone();
                    let jellyfin = state.media.clone();
                    let proxy = state.proxy.clone();
                    let audio = state.audio.clone();
                    let emit_handle = dm_app_handle.clone();

                    let (dm_handle, _task) = start_download_manager(
                        cache_dir,
                        db,
                        jellyfin,
                        proxy,
                        audio,
                        move |event| {
                            emit_download_event(&emit_handle, event);
                        },
                    );

                    state.set_download_manager(dm_handle).await;
                    tracing::info!("Download manager initialised");
                });

                // Start background sync scheduler
                services::start_sync_scheduler(app_handle.clone());
                commands::start_background_drain(app_handle.clone());
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            // MiniPlayer: the close-to-tray / minimise-to-tray behaviours
            // below apply ONLY to the main window. When the MiniPlayer (label
            // "mini") closes — via its command, native control, or the macOS red
            // traffic light — restore the main window so no close path ever
            // leaves zero visible windows. Then return (never hide-to-tray it).
            if window.label() != window::MAIN_LABEL {
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    if window.label() == commands::MINI_LABEL {
                        desktop_shell::window::show(window.app_handle(), window::MAIN_LABEL);
                    }
                }
                return;
            }
            // Minimise-to-tray: Tauri v2 has no MinimizeRequested event,
                // so detect a minimise via Resized + is_minimized(). The
                // is_visible() guard avoids the restore race — the tray restore
                // (unminimize+show) emits Resized events, and without it a
                // transient still-minimised reading would immediately re-hide.
                if let tauri::WindowEvent::Resized(_) = event {
                    if let Some(st) = window.try_state::<AppState>() {
                        if st.minimise_to_tray.load(Ordering::Relaxed)
                            && window.is_minimized().unwrap_or(false)
                            && window.is_visible().unwrap_or(false)
                        {
                            let _ = window.hide();
                        }
                    }
                }
        })
        .invoke_handler(tauri_plugin_telemetry::invoke_handler(tauri::generate_handler![
            // Jellyfin connection commands
            commands::login_jellyfin,
            commands::quick_connect_check,
            commands::quick_connect_start,
            commands::quick_connect_status,
            commands::quick_connect_complete,
            commands::connect_jellyfin, // deprecated: API key auth
            commands::disconnect_jellyfin,
            commands::get_jellyfin_status,
            commands::test_jellyfin_connection,
            // OpenSubsonic connection
            commands::login_subsonic,
            commands::disconnect_subsonic,
            // Dual-backend toggle + per-backend status
            commands::set_active_backend,
            commands::get_media_backends,
            // Library commands
            commands::get_library_albums,
            commands::get_library_artists,
            commands::get_album_tracks,
            commands::get_track_lyrics,
            commands::get_track_waveform,
            commands::get_artist_albums,
            commands::search_library,
            commands::sync_library,
            commands::get_stream_url,
            commands::get_artwork_url,
            // Cache commands
            commands::get_cached_albums,
            commands::get_recently_added_albums,
            commands::get_cached_artist_albums,
            commands::get_cached_tracks,
            commands::get_all_cached_tracks,
            commands::get_cached_artists,
            commands::search_cached_library,
            commands::get_cache_stats,
            commands::get_pending_sync_operations,
            commands::get_genre_stats,
            commands::query_library_view,
            commands::query_tracks,
            // Playback commands
            commands::play_track,
            commands::play_context,
            commands::pause,
            commands::resume,
            commands::seek,
            commands::set_volume,
            commands::get_playback_state,
            // Queue commands
            commands::get_queue,
            commands::add_to_queue,
            commands::clear_queue,
            commands::remove_from_queue,
            commands::reorder_queue,
            commands::jump_to_queue_index,
            commands::skip_next,
            commands::skip_previous,
            // Shuffle and repeat
            commands::set_shuffle,
            commands::set_repeat,
            // Device selection
            commands::get_output_devices,
            commands::set_output_device,
            // Equalizer commands
            commands::get_eq_presets,
            commands::get_eq_status,
            commands::set_eq_enabled,
            commands::set_eq_preset,
            commands::set_eq_bands,
            commands::save_eq_profile,
            commands::delete_eq_profile,
            commands::get_eq_preset_info,
            commands::reset_eq,
            // Fade on pause/stop
            commands::set_fade_settings,
            // MiniPlayer
            commands::toggle_mini_player,
            commands::open_mini_player,
            commands::close_mini_player,
            commands::set_mini_always_on_top,
            // Scrobbling commands
            commands::scrobble_track,
            commands::skip_track,
            commands::sync_scrobbles,
            commands::get_sync_status,
            commands::get_listening_history,
            commands::connect_music_service,
            commands::disconnect_music_service,
            commands::get_music_service_status,
            commands::test_music_service_connection,
            // Settings commands
            commands::get_settings,
            commands::update_settings,
            commands::get_app_info,
            // AI Playlists
            commands::set_llm_api_key,
            commands::clear_llm_api_key,
            commands::test_llm_connection,
            commands::generate_ai_playlist,
            commands::quit_app,
            commands::set_launch_on_login,
            // Download commands
            commands::download_track,
            commands::download_album,
            commands::cancel_download,
            commands::pause_downloads,
            commands::resume_downloads,
            commands::get_download_queue,
            commands::get_download_state,
            commands::clear_completed_downloads,
            commands::get_cache_settings,
            commands::set_cache_settings,
            commands::clear_cache,
            commands::get_downloaded_tracks,
            commands::remove_downloaded_track,
            // Jellyfin playlist commands
            commands::get_jellyfin_playlists,
            commands::get_jellyfin_playlist,
            commands::get_cached_jellyfin_playlist,
            commands::create_jellyfin_playlist,
            commands::rename_jellyfin_playlist,
            commands::delete_jellyfin_playlist,
            commands::add_tracks_to_jellyfin_playlist,
            commands::remove_tracks_from_jellyfin_playlist,
            commands::move_jellyfin_playlist_item,
            // Shared/imported playlist commands
            commands::import_playlist,
            commands::import_playlist_from_jellyfin_link,
            commands::get_imported_playlists,
            commands::get_imported_playlist,
            commands::delete_imported_playlist,
            // Track stats commands
            commands::refresh_track_stats,
            commands::get_server_play_counts,
            // Developer commands
            commands::get_developer_info,
            commands::open_data_directory,
            commands::open_cache_directory,
            commands::clear_failed_sync_operations,
            commands::send_test_notification,
            // Telemetry get/set/probe are tauri-plugin-telemetry's own
            // commands, registered by its `.plugin(...)` call above — never
            // listed here.
        ]))
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            desktop_shell::window::on_run_event_with(app_handle, &event, window::show_active);
            if let tauri::RunEvent::Exit = event {
                // Tauri exits without unwinding managed state, so nothing
                // drops the engine: without an explicit shutdown the mpv
                // subprocess outlives the app and keeps playing.
                if let Ok(guard) = app_handle.state::<AppState>().audio.try_read() {
                    if let Some(handle) = guard.as_ref() {
                        handle.shutdown();
                        std::thread::sleep(std::time::Duration::from_millis(300));
                    }
                }
                // `tauri_plugin_telemetry`'s own `on_event` hook takes and
                // drops the Guard on this same event, flushing the batch
                // processors — nothing left for this arm to do for telemetry.
            }
        });
}

/// Emit a download event to the frontend.
fn emit_download_event(app: &tauri::AppHandle, event: DownloadEvent) {
    let event_name = match &event {
        DownloadEvent::Progress { .. } => "download-progress",
        DownloadEvent::Completed { .. } => "download-completed",
        DownloadEvent::Failed { .. } => "download-failed",
        DownloadEvent::Cancelled { .. } => "download-cancelled",
        DownloadEvent::QueueChanged { .. } => "download-queue-changed",
        DownloadEvent::StateChanged { .. } => "download-state-changed",
        DownloadEvent::CacheEvicted { .. } => "download-cache-evicted",
    };

    if let Err(e) = app.emit(event_name, &event) {
        tracing::error!("Failed to emit {} event: {}", event_name, e);
    }
}

/// Emit an audio event to the frontend.
fn emit_audio_event(app: &tauri::AppHandle, event: AudioEvent) {
    // Feed the macOS system Now Playing surface (Control Center / menu-bar /
    // lock screen) with full metadata + artwork + position, and keep media keys
    // routed. Per Apple, updating on every state change is sufficient for
    // the scrubber; the artwork path is resolved from the on-disk cache.
    #[cfg(target_os = "macos")]
    if let AudioEvent::StateChanged {
        current_track,
        is_playing,
        current_time_ms,
        duration_ms,
        ..
    } = &event
    {
        let artwork_path = current_track.as_ref().and_then(|t| {
            app.try_state::<AppState>().and_then(|s| {
                crate::artwork::cached_artwork_path(&s.artwork_cache_dir, &t.album_jellyfin_id)
            })
        });
        media_keys::update_now_playing(
            current_track.as_ref(),
            *is_playing,
            *current_time_ms,
            *duration_ms,
            artwork_path.as_deref(),
        );
    }

    // Keep the system tray in sync with the current track.
    if let AudioEvent::StateChanged {
        current_track,
        is_playing,
        ..
    } = &event
    {
        tray::update_tray_track_info(
            app,
            current_track.as_ref().map(|t| t.name.as_str()),
            current_track.as_ref().map(|t| t.artist_name.as_str()),
            *is_playing,
        );

        // Show notification when track changes (if enabled and window not focused).
        notifications::on_track_change(app, current_track.as_ref(), *is_playing);
    }

    let event_name = match &event {
        AudioEvent::StateChanged { .. } => "playback-state-changed",
        AudioEvent::Progress { .. } => "playback-progress",
        AudioEvent::QueueChanged { .. } => "queue-changed",
        AudioEvent::Error { .. } => "playback-error",
        AudioEvent::TrackFinished { .. } => "track-finished",
        AudioEvent::DevicesChanged { .. } => "audio-devices-changed",
    };

    if let Err(e) = app.emit(event_name, &event) {
        tracing::error!("Failed to emit {} event: {}", event_name, e);
    }
}

#[cfg(test)]
mod tests {
    use super::{gdk_backend_requests_wayland, ALLOWED_TARGETS};

    /// The whole point of a named constant here rather than an inline array
    /// literal at the `tauri_plugin_telemetry::init` call site: a target a
    /// module defines but nobody adds to this list silently stays on stderr
    /// forever. The plugin's own command-span target needs no entry here —
    /// `init` adds it automatically — so `audit::TARGET` and
    /// `commands::SCROBBLE_TARGET` are bragi's whole list.
    #[test]
    fn every_allow_listed_target_is_wired_in() {
        assert!(ALLOWED_TARGETS.contains(&crate::audit::TARGET));
        assert!(ALLOWED_TARGETS.contains(&crate::commands::SCROBBLE_TARGET));
        assert!(ALLOWED_TARGETS.contains(&crate::audio::TARGET));
    }

    /// Both a bare `"wayland"` and a Wayland-first priority list like
    /// `"wayland,x11"` must be recognised as a Wayland request — GDK
    /// tries each comma-separated token in order, so a trailing `x11` never
    /// stops it reaching for Wayland first.
    #[test]
    fn detects_wayland_token_in_list() {
        assert!(gdk_backend_requests_wayland("wayland"));
        assert!(gdk_backend_requests_wayland("wayland,x11"));
        assert!(gdk_backend_requests_wayland("x11,wayland"));
        assert!(gdk_backend_requests_wayland(" wayland , x11 "));
    }

    /// A backend list with no Wayland token must be left alone, matching the
    /// exact-match behaviour for these inputs.
    #[test]
    fn does_not_flag_non_wayland_backends() {
        assert!(!gdk_backend_requests_wayland("x11"));
        assert!(!gdk_backend_requests_wayland("x11,broadway"));
        assert!(!gdk_backend_requests_wayland(""));
    }

    /// wry's WKWebView backend unconditionally overrides `NSDraggingDestination`
    /// on macOS and, whenever `dragDropEnabled` is left at its default `true`,
    /// its handler always returns `true` — so it never falls through to
    /// WKWebView's own native handling, which is the only path that delivers a
    /// drag session into the DOM. That silently ate every column-header drag:
    /// jsdom's tests passed because jsdom never runs a
    /// native drag session at all. Linux's webkitgtk backend only intercepts
    /// file-URI drags, so this never reproduced there either.
    #[test]
    fn drag_drop_disabled_so_native_column_reorder_reaches_the_dom() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("tauri.conf.json must parse");
        let windows = conf["app"]["windows"]
            .as_array()
            .expect("app.windows must be an array");
        for window in windows {
            assert_eq!(
                window["dragDropEnabled"].as_bool(),
                Some(false),
                "window {:?} must set dragDropEnabled: false",
                window["title"]
            );
        }
    }
}
