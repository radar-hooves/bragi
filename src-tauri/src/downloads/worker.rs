//! Download worker for streaming files from Jellyfin.
//!
//! Handles the actual HTTP download with progress reporting.

use std::path::PathBuf;

use bytes::Bytes;
use reqwest::Client;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use super::types::CancellationToken;
use crate::error::AppError;

/// Result of a download operation.
#[derive(Debug)]
pub enum DownloadResult {
    /// Download completed successfully.
    Completed {
        /// Total bytes downloaded.
        bytes: u64,
        /// The `Content-Type` header returned by the server, if any. Used to
        /// resolve the correct on-disk extension when the URL path carries no
        /// usable extension (e.g. OpenSubsonic `stream.view` / `download/{id}`).
        content_type: Option<String>,
    },
    /// Download was cancelled by user.
    Cancelled,
}

/// Progress callback type for download updates.
pub type ProgressCallback = Box<dyn Fn(u64, Option<u64>) + Send + Sync>;

/// Download a file from a URL to a local path with progress reporting.
///
/// Checks the cancellation token after each chunk is downloaded. If cancelled,
/// stops downloading immediately, deletes the partial file, and returns
/// `DownloadResult::Cancelled`.
///
/// # Arguments
/// * `client` - HTTP client to use for the request
/// * `url` - URL to download from
/// * `destination` - Local file path to save to
/// * `on_progress` - Callback for progress updates (bytes_downloaded, total_bytes)
/// * `cancellation_token` - Token to check for cancellation requests
///
/// # Returns
/// * `Ok(DownloadResult::Completed { bytes, content_type })` if the download
///   completed successfully (carries the response `Content-Type` for extension
///   resolution)
/// * `Ok(DownloadResult::Cancelled)` if download was cancelled
/// * `Err(AppError)` if an error occurred
pub async fn download_file(
    client: &Client,
    url: &str,
    destination: &PathBuf,
    on_progress: ProgressCallback,
    cancellation_token: &CancellationToken,
) -> Result<DownloadResult, AppError> {
    tracing::debug!(
        "Starting download from {} to {}",
        url,
        destination.display()
    );

    // Check cancellation before starting
    if cancellation_token.is_cancelled() {
        return Ok(DownloadResult::Cancelled);
    }

    // Ensure parent directory exists
    if let Some(parent) = destination.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            map_io_error(e, &format!("create cache directory '{}'", parent.display()))
        })?;
    }

    // Start the request. `without_url()` strips it before it ever reaches the
    // returned error's `Display`: the local proxy signs `?tk=<token>` into this
    // exact URL, and this error's message goes on to the download queue
    // (user-visible) and to telemetry alike.
    let response =
        client.get(url).send().await.map_err(|e| {
            AppError::Network(format!("Download request failed: {}", e.without_url()))
        })?;

    if !response.status().is_success() {
        return Err(AppError::Network(format!(
            "Download failed with status: {}",
            response.status()
        )));
    }

    // Capture the Content-Type header (owned) before consuming the body so the
    // caller can resolve the correct on-disk extension. Never logged.
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());

    // Get content length if available
    let total_bytes = response.content_length();

    // Create the destination file
    let mut file = File::create(destination)
        .await
        .map_err(|e| map_io_error(e, &format!("create file '{}'", destination.display())))?;

    // Download in chunks
    let mut bytes_downloaded: u64 = 0;
    let mut stream = response.bytes_stream();

    use futures_util::StreamExt;

    while let Some(chunk_result) = stream.next().await {
        // Check cancellation after each chunk
        if cancellation_token.is_cancelled() {
            tracing::debug!("Download cancelled, cleaning up partial file");
            drop(file); // Release file handle before deletion
            let _ = delete_cached_file(destination).await;
            return Ok(DownloadResult::Cancelled);
        }

        let chunk: Bytes = chunk_result
            .map_err(|e| AppError::Network(format!("Error reading download stream: {}", e)))?;

        file.write_all(&chunk)
            .await
            .map_err(|e| map_io_error(e, &format!("write to file '{}'", destination.display())))?;

        bytes_downloaded += chunk.len() as u64;
        on_progress(bytes_downloaded, total_bytes);
    }

    // Final cancellation check before committing
    if cancellation_token.is_cancelled() {
        tracing::debug!("Download cancelled after completion, cleaning up");
        drop(file);
        let _ = delete_cached_file(destination).await;
        return Ok(DownloadResult::Cancelled);
    }

    // Ensure file is flushed and synced
    file.flush()
        .await
        .map_err(|e| map_io_error(e, &format!("flush file '{}'", destination.display())))?;

    file.sync_all()
        .await
        .map_err(|e| map_io_error(e, &format!("sync file '{}'", destination.display())))?;

    tracing::debug!(
        "Download completed: {} bytes to {}",
        bytes_downloaded,
        destination.display()
    );

    Ok(DownloadResult::Completed {
        bytes: bytes_downloaded,
        content_type,
    })
}

/// Maps an I/O error to the appropriate AppError variant.
fn map_io_error(error: std::io::Error, operation: &str) -> AppError {
    match error.kind() {
        std::io::ErrorKind::NotFound => {
            AppError::NotFound(format!("File system error during {}: {}", operation, error))
        }
        std::io::ErrorKind::PermissionDenied => {
            AppError::InvalidOperation(format!("Permission denied during {}: {}", operation, error))
        }
        std::io::ErrorKind::AlreadyExists => {
            AppError::InvalidOperation(format!("Already exists during {}: {}", operation, error))
        }
        _ => AppError::Config(format!("Failed to {}: {}", operation, error)),
    }
}

/// Delete a cached file.
///
/// Handles the case where the file doesn't exist gracefully (no error).
/// This avoids TOCTOU race conditions by not checking existence before deletion.
pub async fn delete_cached_file(path: &PathBuf) -> Result<(), AppError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => {
            tracing::debug!("Deleted cached file: {}", path.display());
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // File doesn't exist, that's fine
            tracing::debug!(
                "Cached file already deleted or never existed: {}",
                path.display()
            );
            Ok(())
        }
        Err(e) => Err(map_io_error(
            e,
            &format!("delete cached file '{}'", path.display()),
        )),
    }
}

/// Get the file extension from a URL or default to flac.
pub fn get_extension_from_url(url: &str) -> String {
    // Try to extract extension from URL path
    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(mut segments) = parsed.path_segments() {
            if let Some(last) = segments.next_back() {
                if let Some(dot_pos) = last.rfind('.') {
                    let ext = &last[dot_pos + 1..];
                    // Validate it's a known audio extension
                    if matches!(
                        ext.to_lowercase().as_str(),
                        "flac" | "mp3" | "m4a" | "aac" | "ogg" | "opus" | "wav" | "wma"
                    ) {
                        return ext.to_lowercase();
                    }
                }
            }
        }
    }

    // Default to flac for lossless downloads
    "flac".to_string()
}

/// Map an audio MIME type to a canonical on-disk extension.
///
/// Parameters (e.g. `; charset=utf-8`, `; codecs=opus`) are stripped and the
/// type is lowercased before matching. Generic/unknown types such as
/// `application/octet-stream` return `None` so the caller can fall back to the
/// requested transcode container or a URL-derived extension.
///
/// Note: `audio/ogg` is ambiguous (it can carry Vorbis or Opus). It maps to
/// `ogg` here; callers that requested a specific Opus/Ogg container should
/// prefer that container over this mapping.
pub fn extension_from_content_type(ct: &str) -> Option<String> {
    let mime = ct.split(';').next().unwrap_or("").trim().to_lowercase();
    let ext = match mime.as_str() {
        "audio/mpeg" => "mp3",
        "audio/mp4" | "audio/x-m4a" | "audio/aac+mp4" => "m4a",
        "audio/aac" | "audio/aacp" => "aac",
        "audio/flac" | "audio/x-flac" => "flac",
        "audio/opus" => "opus",
        "audio/ogg" => "ogg",
        "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
        "audio/webm" => "webm",
        _ => return None,
    };
    Some(ext.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_extension_from_url() {
        assert_eq!(
            get_extension_from_url("https://example.com/audio/track.mp3"),
            "mp3"
        );
        assert_eq!(
            get_extension_from_url("https://example.com/audio/track.FLAC"),
            "flac"
        );
        assert_eq!(
            get_extension_from_url("https://example.com/audio/track.m4a?token=abc"),
            "m4a"
        );
        // Unknown extension defaults to flac
        assert_eq!(
            get_extension_from_url("https://example.com/audio/stream"),
            "flac"
        );
        // Transcode-shaped URLs: the query string must not confuse parsing.
        assert_eq!(
            get_extension_from_url(
                "https://host/Audio/ID/stream.mp3?audioCodec=mp3&audioBitRate=320000"
            ),
            "mp3"
        );
        assert_eq!(
            get_extension_from_url(
                "https://host/Audio/ID/stream.m4a?audioCodec=aac&audioBitRate=256000"
            ),
            "m4a"
        );
        assert_eq!(
            get_extension_from_url(
                "https://host/Audio/ID/stream.opus?audioCodec=opus&audioBitRate=128000"
            ),
            "opus"
        );
        assert_eq!(
            get_extension_from_url("https://host/Audio/ID/stream.flac?audioCodec=flac"),
            "flac"
        );
    }

    #[test]
    fn test_extension_from_content_type_known_types() {
        assert_eq!(
            extension_from_content_type("audio/mpeg").as_deref(),
            Some("mp3")
        );
        assert_eq!(
            extension_from_content_type("audio/mp4").as_deref(),
            Some("m4a")
        );
        assert_eq!(
            extension_from_content_type("audio/x-m4a").as_deref(),
            Some("m4a")
        );
        assert_eq!(
            extension_from_content_type("audio/aac+mp4").as_deref(),
            Some("m4a")
        );
        assert_eq!(
            extension_from_content_type("audio/aac").as_deref(),
            Some("aac")
        );
        assert_eq!(
            extension_from_content_type("audio/aacp").as_deref(),
            Some("aac")
        );
        assert_eq!(
            extension_from_content_type("audio/flac").as_deref(),
            Some("flac")
        );
        assert_eq!(
            extension_from_content_type("audio/x-flac").as_deref(),
            Some("flac")
        );
        assert_eq!(
            extension_from_content_type("audio/opus").as_deref(),
            Some("opus")
        );
        assert_eq!(
            extension_from_content_type("audio/ogg").as_deref(),
            Some("ogg")
        );
        assert_eq!(
            extension_from_content_type("audio/wav").as_deref(),
            Some("wav")
        );
        assert_eq!(
            extension_from_content_type("audio/x-wav").as_deref(),
            Some("wav")
        );
        assert_eq!(
            extension_from_content_type("audio/wave").as_deref(),
            Some("wav")
        );
        assert_eq!(
            extension_from_content_type("audio/webm").as_deref(),
            Some("webm")
        );
    }

    #[test]
    fn test_extension_from_content_type_parameterised_and_cased() {
        // Parameters stripped, case-insensitive.
        assert_eq!(
            extension_from_content_type("audio/mpeg; charset=utf-8").as_deref(),
            Some("mp3")
        );
        assert_eq!(
            extension_from_content_type("audio/ogg; codecs=opus").as_deref(),
            Some("ogg")
        );
        assert_eq!(
            extension_from_content_type("AUDIO/FLAC").as_deref(),
            Some("flac")
        );
        assert_eq!(
            extension_from_content_type("  audio/aac  ; rate=44100").as_deref(),
            Some("aac")
        );
    }

    #[test]
    fn test_extension_from_content_type_unknown_is_none() {
        assert_eq!(
            extension_from_content_type("application/octet-stream"),
            None
        );
        assert_eq!(extension_from_content_type("text/html"), None);
        assert_eq!(extension_from_content_type(""), None);
        assert_eq!(extension_from_content_type("audio/unknown-codec"), None);
    }
}
