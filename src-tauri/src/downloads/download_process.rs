//! Single download processing with retry logic and progress reporting.
//!
//! Handles downloading a single track with exponential backoff retries,
//! cancellation support, and atomic progress state tracking.

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::db::settings::keys;
use crate::db::Database;
use crate::mediaserver::MediaClient;
use crate::proxy::ProxyHandle;

use super::cache::cache_has_room;
use super::manager::DownloadManagerState;
use super::types::{
    CancellationToken, DownloadEvent, DownloadFormat, DownloadStatus, QueuedDownload,
};
use super::worker::{
    delete_cached_file, download_file, extension_from_content_type, get_extension_from_url,
    DownloadResult,
};

/// Maximum number of retry attempts for transient download failures.
const MAX_DOWNLOAD_RETRIES: u32 = 3;

/// Atomic progress state for a download.
///
/// Uses atomic operations to avoid spawning unbounded tasks for progress updates.
struct AtomicProgressState {
    bytes_downloaded: AtomicU64,
    total_bytes: AtomicU64,
    total_bytes_known: std::sync::atomic::AtomicBool,
    progress_percent: AtomicU8,
}

impl AtomicProgressState {
    fn new() -> Self {
        Self {
            bytes_downloaded: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
            total_bytes_known: std::sync::atomic::AtomicBool::new(false),
            progress_percent: AtomicU8::new(0),
        }
    }

    fn update(&self, bytes_downloaded: u64, total_bytes: Option<u64>) {
        self.bytes_downloaded
            .store(bytes_downloaded, Ordering::Release);
        if let Some(total) = total_bytes {
            self.total_bytes.store(total, Ordering::Release);
            self.total_bytes_known.store(true, Ordering::Release);
            let percent = if total > 0 {
                ((bytes_downloaded as f64 / total as f64) * 100.0) as u8
            } else {
                0
            };
            self.progress_percent.store(percent, Ordering::Release);
        }
    }

    fn get_bytes_downloaded(&self) -> u64 {
        self.bytes_downloaded.load(Ordering::Acquire)
    }

    fn get_total_bytes(&self) -> Option<u64> {
        if self.total_bytes_known.load(Ordering::Acquire) {
            Some(self.total_bytes.load(Ordering::Acquire))
        } else {
            None
        }
    }

    fn get_progress_percent(&self) -> u8 {
        self.progress_percent.load(Ordering::Acquire)
    }
}

/// Process a single download.
pub(super) async fn process_download<F>(
    download: QueuedDownload,
    state: Arc<RwLock<DownloadManagerState>>,
    db: Arc<Database>,
    jellyfin: Arc<RwLock<Option<MediaClient>>>,
    proxy: Arc<RwLock<Option<ProxyHandle>>>,
    event_callback: Arc<F>,
    cancellation_token: CancellationToken,
) where
    F: Fn(DownloadEvent) + Send + Sync + 'static,
{
    let track_id = download.track_id.clone();
    let track_name = download.track_name.clone();
    let artist_name = download.artist_name.clone();

    tracing::info!("Starting download: {} - {}", artist_name, track_name);

    // A download is the user's explicit choice to keep a track offline; a
    // full cache refuses it rather than silently evicting an older download
    // to make room (Apple Music/Spotify never do the latter).
    let cache_limit = state.read().await.cache_limit_bytes;
    match cache_has_room(&db, cache_limit) {
        Ok(false) => {
            update_download_failed(
                &state,
                &track_id,
                "Download cache is full — remove some downloads to free space",
                &event_callback,
            )
            .await;
            cleanup_download(&state, &track_id).await;
            return;
        }
        Ok(true) => {}
        Err(e) => {
            tracing::warn!("Failed to check cache capacity, proceeding anyway: {}", e);
        }
    }

    // Read proxy port + per-session auth token lazily (avoids race with async
    // proxy startup). The token is required on `/download` requests.
    let proxy_endpoint = proxy
        .read()
        .await
        .as_ref()
        .map(|p| (p.port(), p.token().to_string()));

    // Resolve the desired download format + bitrate. Defaults preserve today's
    // behaviour exactly: Original (the proxy path fetching the exact source file).
    let format = db
        .get_setting(keys::DOWNLOAD_FORMAT)
        .ok()
        .flatten()
        .and_then(|v| DownloadFormat::parse(&v))
        .unwrap_or_default();
    let bitrate = db
        .get_parsed(keys::DOWNLOAD_BITRATE)
        .ok()
        .flatten()
        .unwrap_or(256);

    // Build the primary download URL + HTTP client, plus the Original proxy URL
    // used as a fallback if a server-side transcode fails.
    //
    // The download client has no total timeout so large / slowly-transcoded
    // bodies aren't aborted after 30s (unlike the shared `http()` client).
    let (download_url, http_client, fallback_url) = {
        let jellyfin_guard = jellyfin.read().await;
        let client = match jellyfin_guard.as_ref() {
            Some(client) => client,
            None => {
                update_download_failed(
                    &state,
                    &track_id,
                    "Jellyfin not connected",
                    &event_callback,
                )
                .await;
                cleanup_download(&state, &track_id).await;
                return;
            }
        };

        let http_client = client.download_http().clone();
        // The Original proxy URL requires the local proxy; it is also the
        // fallback source for a failed transcode.
        let proxy_url = proxy_endpoint
            .as_ref()
            .map(|(port, token)| client.get_download_url(&track_id, *port, token));

        match format.transcode_params() {
            // Transcode: hit Jellyfin directly (auth is a baked-in header).
            Some((container, codec)) => {
                let br = if format.is_lossy() {
                    Some(bitrate)
                } else {
                    None
                };
                let url = client.get_transcoded_stream_url(&track_id, container, codec, br);
                (url, http_client, proxy_url)
            }
            // Original: requires the local proxy (unchanged behaviour).
            None => match proxy_url {
                Some(url) => (url, http_client, None),
                None => {
                    update_download_failed(
                        &state,
                        &track_id,
                        "Local proxy not started",
                        &event_callback,
                    )
                    .await;
                    cleanup_download(&state, &track_id).await;
                    return;
                }
            },
        }
    };

    // Compute a PROVISIONAL on-disk extension. For a transcode this is the
    // requested container (guaranteed correct for Jellyfin; also correct for
    // Subsonic whose URL path — `/rest/stream.view` — carries no real
    // extension). For Original we fall back to the URL-derived extension (the
    // proxy URL has none, so this is `flac`). The final extension is
    // reconciled against the response Content-Type after the bytes land.
    let provisional_extension = match format.container_extension() {
        Some(container) => container.to_string(),
        None => get_extension_from_url(&download_url),
    };
    let cache_path = {
        let state_guard = state.read().await;
        state_guard.cache_path(&track_id, &provisional_extension)
    };

    // Create atomic progress state to avoid spawning unbounded tasks
    let progress_state = Arc::new(AtomicProgressState::new());

    // Attempt the primary source with retries.
    let mut outcome = run_download_attempts(
        &http_client,
        &download_url,
        &cache_path,
        &progress_state,
        &cancellation_token,
        &event_callback,
        &track_id,
        &track_name,
        &artist_name,
    )
    .await;

    // The path whose bytes actually landed on disk (may change on fallback).
    let mut final_cache_path = cache_path.clone();
    // The format + provisional extension that actually produced the on-disk
    // bytes. These change if the transcode fails and we fall back to Original.
    let mut effective_format = format;
    let mut provisional_final_ext = provisional_extension.clone();

    // If a transcode failed, fall back to the Original proxy download so the
    // user still gets an offline copy. Recompute the extension + cache path so
    // the on-disk file isn't left with a misleading transcode extension.
    if let AttemptOutcome::Failed(ref err) = outcome {
        if format != DownloadFormat::Original {
            if let Some(fallback_url) = fallback_url {
                tracing::warn!(
                    "Transcode ({}) failed for {} - {} ({}); falling back to Original",
                    format.as_str(),
                    artist_name,
                    track_name,
                    err,
                );
                // Remove any partial transcode file before switching paths.
                let _ = delete_cached_file(&cache_path).await;

                let fallback_ext = get_extension_from_url(&fallback_url);
                let fallback_path = {
                    let state_guard = state.read().await;
                    state_guard.cache_path(&track_id, &fallback_ext)
                };

                if cancellation_token.is_cancelled() {
                    cleanup_download(&state, &track_id).await;
                    return;
                }

                outcome = run_download_attempts(
                    &http_client,
                    &fallback_url,
                    &fallback_path,
                    &progress_state,
                    &cancellation_token,
                    &event_callback,
                    &track_id,
                    &track_name,
                    &artist_name,
                )
                .await;
                final_cache_path = fallback_path;
                // The fallback is always an Original proxy download.
                effective_format = DownloadFormat::Original;
                provisional_final_ext = fallback_ext;
            }
        }
    }

    match outcome {
        AttemptOutcome::Cancelled => {
            tracing::info!("Download cancelled: {} - {}", artist_name, track_name);
            // Partial file already deleted by worker
            cleanup_download(&state, &track_id).await;
            return;
        }
        AttemptOutcome::Completed {
            bytes: file_size,
            content_type,
        } => {
            // Reconcile the provisional extension against the actual returned
            // Content-Type. For Subsonic backends the URL path carries no real
            // extension, so this is what makes MP3/AAC/Opus transcodes and
            // non-FLAC originals land on disk with the correct extension.
            let final_ext = resolve_final_extension(
                content_type.as_deref(),
                effective_format,
                &provisional_final_ext,
            );

            if final_ext != provisional_final_ext {
                let renamed_path = {
                    let state_guard = state.read().await;
                    state_guard.cache_path(&track_id, &final_ext)
                };
                match tokio::fs::rename(&final_cache_path, &renamed_path).await {
                    Ok(()) => {
                        tracing::debug!(
                            "Reconciled download extension {} -> {} for {} - {}",
                            provisional_final_ext,
                            final_ext,
                            artist_name,
                            track_name,
                        );
                        final_cache_path = renamed_path;
                    }
                    Err(e) => {
                        // Keep the provisional path rather than losing the file.
                        tracing::warn!(
                            "Failed to rename download to final extension {} (keeping {}): {}",
                            final_ext,
                            provisional_final_ext,
                            e,
                        );
                    }
                }
            }

            // Update database
            if let Err(e) = db.set_track_cached_path(
                &track_id,
                final_cache_path.to_string_lossy().as_ref(),
                file_size,
            ) {
                tracing::error!("Failed to update track cache path in database: {}", e);
            }

            // Update state
            {
                let mut state_guard = state.write().await;
                if let Some(download) = state_guard
                    .queue
                    .iter_mut()
                    .find(|d| d.track_id == track_id)
                {
                    download.status = DownloadStatus::Completed;
                    download.progress_percent = 100;
                    download.bytes_downloaded = file_size;
                    download.total_bytes = Some(file_size);
                }
            }

            event_callback(DownloadEvent::Completed {
                track_id: track_id.clone(),
                track_name: track_name.clone(),
                file_path: final_cache_path.to_string_lossy().to_string(),
                file_size_bytes: file_size,
            });

            tracing::info!(
                "Download completed: {} - {} ({} bytes)",
                artist_name,
                track_name,
                file_size
            );
        }
        AttemptOutcome::Failed(last_error) => {
            update_download_failed(&state, &track_id, &last_error, &event_callback).await;
        }
    }

    // Update progress state from atomic values before cleanup
    {
        let mut state_guard = state.write().await;
        if let Some(download) = state_guard
            .queue
            .iter_mut()
            .find(|d| d.track_id == track_id)
        {
            download.bytes_downloaded = progress_state.get_bytes_downloaded();
            download.total_bytes = progress_state.get_total_bytes();
            download.progress_percent = progress_state.get_progress_percent();
        }
    }

    cleanup_download(&state, &track_id).await;
}

/// Outcome of a bounded set of download attempts against one source URL.
enum AttemptOutcome {
    /// Download completed successfully.
    Completed {
        /// Total bytes written to disk.
        bytes: u64,
        /// The response `Content-Type`, used to resolve the final extension.
        content_type: Option<String>,
    },
    /// Download was cancelled by the user.
    Cancelled,
    /// All retries were exhausted; carries the last error message.
    Failed(String),
}

/// Resolve the final on-disk extension for a completed download.
///
/// Precedence:
/// 1. A specific, unambiguous extension derived from the response
///    `Content-Type` — except that the ambiguous `audio/ogg` yields to an
///    explicitly requested Opus/Ogg transcode container.
/// 2. Otherwise the requested transcode container (for a transcode download).
/// 3. Otherwise the provisional (URL-derived) extension.
/// 4. Otherwise `flac` (the lossless default).
///
/// This is what gives OpenSubsonic downloads a correct extension: their URL
/// path (`/rest/stream.view`, `/download/{id}`) carries no usable extension, so
/// the returned `Content-Type` is the only reliable signal for an Original
/// download, while a transcode falls back to the requested container.
fn resolve_final_extension(
    content_type: Option<&str>,
    format: DownloadFormat,
    provisional: &str,
) -> String {
    let requested_container = format.container_extension();

    // (1) Prefer a specific extension derived from the actual Content-Type.
    if let Some(ct) = content_type {
        if let Some(ext) = extension_from_content_type(ct) {
            // `audio/ogg` is ambiguous (Vorbis vs Opus). If we explicitly asked
            // for an Opus/Ogg container, trust the request over the generic type.
            if ext == "ogg" {
                if let Some(container) = requested_container {
                    if container == "opus" || container == "ogg" {
                        return container.to_string();
                    }
                }
            }
            return ext;
        }
    }

    // (2) No usable Content-Type: for a transcode, trust the requested container.
    if let Some(container) = requested_container {
        return container.to_string();
    }

    // (3) Fall back to the URL-derived provisional extension.
    if !provisional.is_empty() {
        return provisional.to_string();
    }

    // (4) ...and finally to flac (lossless default).
    "flac".to_string()
}

/// Attempt to download `download_url` into `cache_path`, retrying transient
/// failures with exponential backoff up to `MAX_DOWNLOAD_RETRIES` times.
#[allow(clippy::too_many_arguments)] // Mirrors the manager's shared handles
async fn run_download_attempts<F>(
    http_client: &reqwest::Client,
    download_url: &str,
    cache_path: &std::path::PathBuf,
    progress_state: &Arc<AtomicProgressState>,
    cancellation_token: &CancellationToken,
    event_callback: &Arc<F>,
    track_id: &str,
    track_name: &str,
    artist_name: &str,
) -> AttemptOutcome
where
    F: Fn(DownloadEvent) + Send + Sync + 'static,
{
    let mut last_error = String::new();

    for attempt in 0..MAX_DOWNLOAD_RETRIES {
        if attempt > 0 {
            let delay = std::time::Duration::from_secs(2u64.pow(attempt));
            tracing::info!(
                "Retrying download ({}/{}) after {:?}: {} - {}",
                attempt + 1,
                MAX_DOWNLOAD_RETRIES,
                delay,
                artist_name,
                track_name,
            );
            tokio::time::sleep(delay).await;

            // Check cancellation between retries
            if cancellation_token.is_cancelled() {
                return AttemptOutcome::Cancelled;
            }
        }

        // Reset progress for each attempt
        progress_state.update(0, None);

        let progress_state_for_callback = progress_state.clone();
        let track_id_for_progress = track_id.to_string();
        let track_name_for_progress = track_name.to_string();
        let artist_name_for_progress = artist_name.to_string();
        let event_callback_for_progress = event_callback.clone();

        let progress_callback = Box::new(move |bytes_downloaded: u64, total_bytes: Option<u64>| {
            // Update atomic state (no task spawning)
            progress_state_for_callback.update(bytes_downloaded, total_bytes);

            let progress_percent = progress_state_for_callback.get_progress_percent();

            // Emit event directly (callback is Send + Sync)
            event_callback_for_progress(DownloadEvent::Progress {
                track_id: track_id_for_progress.clone(),
                track_name: track_name_for_progress.clone(),
                artist_name: artist_name_for_progress.clone(),
                bytes_downloaded,
                total_bytes,
                progress_percent,
            });
        });

        // Perform the download with cancellation token
        let result = download_file(
            http_client,
            download_url,
            cache_path,
            progress_callback,
            cancellation_token,
        )
        .await;

        match result {
            Ok(DownloadResult::Cancelled) => return AttemptOutcome::Cancelled,
            Ok(DownloadResult::Completed {
                bytes,
                content_type,
            }) => {
                return AttemptOutcome::Completed {
                    bytes,
                    content_type,
                }
            }
            Err(e) => {
                // Delete partial file before retry
                let _ = delete_cached_file(cache_path).await;
                last_error = e.to_string();
                tracing::warn!(
                    "Download attempt {}/{} failed for {} - {}: {}",
                    attempt + 1,
                    MAX_DOWNLOAD_RETRIES,
                    artist_name,
                    track_name,
                    last_error,
                );
            }
        }
    }

    AttemptOutcome::Failed(last_error)
}

/// Clean up after a download completes (success, failure, or cancellation).
async fn cleanup_download(state: &Arc<RwLock<DownloadManagerState>>, track_id: &str) {
    let mut state_guard = state.write().await;
    state_guard.active_download = None;
    state_guard.remove_cancellation_token(track_id);
}

/// Update a download as failed.
async fn update_download_failed<F>(
    state: &Arc<RwLock<DownloadManagerState>>,
    track_id: &str,
    error: &str,
    event_callback: &Arc<F>,
) where
    F: Fn(DownloadEvent) + Send + Sync + 'static,
{
    let mut state_guard = state.write().await;
    let track_name = if let Some(download) = state_guard
        .queue
        .iter_mut()
        .find(|d| d.track_id == track_id)
    {
        download.status = DownloadStatus::Failed;
        download.error = Some(error.to_string());
        download.track_name.clone()
    } else {
        "Unknown".to_string()
    };
    drop(state_guard);

    tracing::error!("Download failed for {}: {}", track_id, error);
    telemetry::report_error_with_cause(
        "download failed",
        &crate::error::MessageError(error.to_string()),
    );

    event_callback(DownloadEvent::Failed {
        track_id: track_id.to_string(),
        track_name,
        error: error.to_string(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The container the downloader would provisionally pick for a format,
    /// mirroring the logic in `process_download` for the primary attempt.
    fn provisional_for(format: DownloadFormat) -> String {
        format
            .container_extension()
            .map(str::to_string)
            .unwrap_or_else(|| "flac".to_string())
    }

    #[test]
    fn resolve_transcode_without_content_type_uses_container() {
        // Server sent no (usable) Content-Type: trust the requested container.
        assert_eq!(
            resolve_final_extension(
                None,
                DownloadFormat::Mp3,
                &provisional_for(DownloadFormat::Mp3)
            ),
            "mp3"
        );
    }

    #[test]
    fn resolve_transcode_opus_ambiguous_ogg_prefers_requested_container() {
        // `audio/ogg` is ambiguous; an explicit Opus request wins.
        assert_eq!(
            resolve_final_extension(
                Some("audio/ogg"),
                DownloadFormat::Opus,
                &provisional_for(DownloadFormat::Opus)
            ),
            "opus"
        );
        // Even with codecs parameter noise.
        assert_eq!(
            resolve_final_extension(
                Some("audio/ogg; codecs=opus"),
                DownloadFormat::Opus,
                &provisional_for(DownloadFormat::Opus)
            ),
            "opus"
        );
    }

    #[test]
    fn resolve_transcode_aac_matches_content_type() {
        assert_eq!(
            resolve_final_extension(
                Some("audio/aac"),
                DownloadFormat::Aac,
                &provisional_for(DownloadFormat::Aac)
            ),
            "aac"
        );
    }

    #[test]
    fn resolve_transcode_flac_matches_content_type() {
        assert_eq!(
            resolve_final_extension(
                Some("audio/flac"),
                DownloadFormat::Flac,
                &provisional_for(DownloadFormat::Flac)
            ),
            "flac"
        );
    }

    #[test]
    fn resolve_original_uses_content_type() {
        // A non-FLAC Subsonic original: the proxied upstream Content-Type is
        // the only signal (provisional from the proxy URL is `flac`).
        assert_eq!(
            resolve_final_extension(Some("audio/mpeg"), DownloadFormat::Original, "flac"),
            "mp3"
        );
    }

    #[test]
    fn resolve_original_without_content_type_keeps_provisional() {
        assert_eq!(
            resolve_final_extension(None, DownloadFormat::Original, "flac"),
            "flac"
        );
    }

    #[test]
    fn resolve_original_generic_content_type_keeps_provisional() {
        // application/octet-stream is not a specific audio type.
        assert_eq!(
            resolve_final_extension(
                Some("application/octet-stream"),
                DownloadFormat::Original,
                "flac"
            ),
            "flac"
        );
    }

    /// Regression: a Subsonic-shaped transcode URL cannot be parsed for its
    /// extension (it resolves to `flac`), but the Content-Type-first resolver
    /// still produces the correct extension.
    #[test]
    fn subsonic_transcode_url_extension_is_unreliable_but_resolver_is_correct() {
        let subsonic_url = "https://host/rest/stream.view?format=mp3&maxBitRate=320000";
        // URL parsing is unreliable for Subsonic: yields the default `flac`.
        assert_eq!(get_extension_from_url(subsonic_url), "flac");
        // The resolver, given the requested format, yields the correct ext.
        assert_eq!(
            resolve_final_extension(
                Some("audio/mpeg"),
                DownloadFormat::Mp3,
                &provisional_for(DownloadFormat::Mp3)
            ),
            "mp3"
        );
        // ...and even without a Content-Type, the requested container wins.
        assert_eq!(
            resolve_final_extension(
                None,
                DownloadFormat::Mp3,
                &provisional_for(DownloadFormat::Mp3)
            ),
            "mp3"
        );
    }

    /// A download is the user's explicit choice to keep a track offline: a
    /// full cache must refuse the new download rather than silently evicting
    /// an older one (Apple Music and Spotify never evict a download to make
    /// room; see `cache::cache_has_room`). This drives the real
    /// `process_download` entry point with no Jellyfin client configured and
    /// still gets the cache-full refusal — never "Jellyfin not connected" —
    /// proving the capacity check runs before any network setup.
    #[tokio::test]
    async fn download_refused_when_cache_is_full() {
        use super::super::types::DownloadRequest;
        use crate::db::{CachedAlbum, CachedTrack};
        use crate::mediaserver::ServerKind;
        use std::sync::Mutex;

        let db = Arc::new(Database::open_in_memory().unwrap());
        db.upsert_album(
            &CachedAlbum {
                jellyfin_id: "album000000000000000000000000000".to_string(),
                release_mbid: None,
                name: "Existing Album".to_string(),
                artist_name: Some("Artist".to_string()),
                artist_jellyfin_id: None,
                year: None,
                genre: None,
                artwork_url: None,
                track_count: Some(1),
                duration_ms: None,
                date_created: None,
            },
            ServerKind::Jellyfin,
            None,
        )
        .unwrap();
        db.upsert_tracks(
            &[CachedTrack {
                jellyfin_id: "existing0000000000000000000000000".to_string(),
                track_mbid: None,
                artist_mbid: None,
                release_mbid: None,
                name: "Existing Track".to_string(),
                artist_name: Some("Artist".to_string()),
                album_jellyfin_id: Some("album000000000000000000000000000".to_string()),
                track_number: Some(1),
                disc_number: Some(1),
                duration_ms: Some(200_000),
                genre: None,
                artwork_url: None,
                cached_file_path: Some("/tmp/does-not-exist/existing.flac".to_string()),
                file_size_bytes: Some(100),
                normalization_gain_db: None,
                server_play_count: None,
            }],
            ServerKind::Jellyfin,
            None,
        )
        .unwrap();

        // Cache limit equals what is already downloaded: no room for another.
        let state = Arc::new(RwLock::new(DownloadManagerState::new(
            std::env::temp_dir(),
            100,
        )));
        let jellyfin = Arc::new(RwLock::new(None));
        let proxy = Arc::new(RwLock::new(None));

        let events: Arc<Mutex<Vec<DownloadEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_for_cb = events.clone();
        let event_callback = Arc::new(move |event: DownloadEvent| {
            events_for_cb.lock().unwrap().push(event);
        });

        let download = QueuedDownload::from_request(
            DownloadRequest {
                track_id: "newtrack00000000000000000000000000".to_string(),
                track_name: "New Track".to_string(),
                artist_name: "Artist".to_string(),
                album_id: None,
            },
            0,
        );

        process_download(
            download,
            state,
            db.clone(),
            jellyfin,
            proxy,
            event_callback,
            CancellationToken::new(),
        )
        .await;

        let events = events.lock().unwrap();
        let failed = events.iter().find_map(|e| match e {
            DownloadEvent::Failed { error, .. } => Some(error.clone()),
            _ => None,
        });
        assert_eq!(
            failed.as_deref(),
            Some("Download cache is full — remove some downloads to free space"),
            "must refuse with a clear cache-full message, not attempt the download"
        );
        assert_eq!(
            db.get_cached_files_size().unwrap(),
            100,
            "the existing download must survive a full cache untouched"
        );
    }
}
