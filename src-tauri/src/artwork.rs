//! Local artwork cache.
//!
//! Downloads album and artist artwork from Jellyfin during library sync and
//! stores it in the app cache directory. A custom `artwork://` protocol
//! serves cached images to the WebView without network access.
//!
//! Files are named `{jellyfin_id}.jpg`, with the version they were fetched at
//! (Jellyfin's Primary image tag, or Subsonic's versioned coverArt id) beside
//! them in `{jellyfin_id}.ver`. A file is re-downloaded when its version no
//! longer matches the one the sync reports; a `.jpg` with no `.ver` (written
//! before versioning) is still served, and refetched by the next online sync.

use std::path::{Path, PathBuf};

use crate::mediaserver::MediaClient;

/// Download and cache artwork for a Jellyfin item unless the cached file is
/// already at `version`.
///
/// Returns the `artwork://localhost/{id}.jpg` URL on success, or `None` if
/// the download fails (caller should fall back to the remote Jellyfin URL).
pub async fn cache_artwork(
    client: &MediaClient,
    jellyfin_id: &str,
    version: &str,
    cache_dir: &Path,
) -> Option<String> {
    let safe_id = sanitise_id(jellyfin_id);
    let file_path = cache_dir.join(format!("{}.jpg", safe_id));

    // Already cached at this version — no network. (Async reads: sync stats
    // would otherwise block an executor thread under the sync's concurrent
    // artwork fan-out.)
    if is_current(&file_path, version).await {
        return Some(artwork_url(&safe_id));
    }

    // Ensure the directory exists before writing.
    if let Err(e) = tokio::fs::create_dir_all(cache_dir).await {
        tracing::warn!("Failed to create artwork cache dir: {}", e);
        return None;
    }

    // Fetch bytes server-side. For OpenSubsonic `get_artwork_url` no longer
    // yields a fetchable URL (it returns a credential-free proxy sentinel), so
    // the warm goes through `fetch_artwork_bytes`, which signs the request
    // internally. Jellyfin builds its Primary-image URL the same
    // way as before.
    let bytes = match client.fetch_artwork_bytes(jellyfin_id, 300).await {
        Ok(b) => b,
        Err(e) => {
            tracing::debug!("Artwork download failed for {}: {}", jellyfin_id, e);
            return None;
        }
    };

    if let Err(e) = tokio::fs::write(&file_path, &bytes).await {
        tracing::warn!("Failed to write artwork for {}: {}", jellyfin_id, e);
        return None;
    }
    if let Err(e) = tokio::fs::write(version_path(&file_path), version).await {
        tracing::warn!("Failed to write artwork version for {}: {}", jellyfin_id, e);
        return None;
    }

    Some(artwork_url(&safe_id))
}

fn version_path(file_path: &Path) -> PathBuf {
    file_path.with_extension("ver")
}

async fn is_current(file_path: &Path, version: &str) -> bool {
    tokio::fs::try_exists(file_path).await.unwrap_or(false)
        && tokio::fs::read_to_string(version_path(file_path))
            .await
            .is_ok_and(|v| v == version)
}

/// Resolve the on-disk path of a cached artwork file, if it exists.
///
/// Uses the same `{jellyfin_id}.jpg` naming as [`cache_artwork`]. Returns
/// `None` when nothing is cached for the given ID, so callers can fall back
/// gracefully (e.g. a text-only OS notification).
pub fn cached_artwork_path(cache_dir: &Path, jellyfin_id: &str) -> Option<PathBuf> {
    let file_path = cache_dir.join(format!("{}.jpg", sanitise_id(jellyfin_id)));
    if file_path.is_file() {
        Some(file_path)
    } else {
        None
    }
}

/// The cache version carried by a stored artwork reference: Jellyfin's Primary
/// image `tag` query value, or the id of a Subsonic `subsonic-cover:` sentinel.
/// Never anything containing the server host, so a LAN/WAN or http/https switch
/// does not refetch every cover. `None` for a reference of neither shape.
pub fn artwork_version(artwork_url: &str) -> Option<String> {
    if let Some(id) = artwork_url.strip_prefix(crate::mediaserver::client::SUBSONIC_COVER_SENTINEL)
    {
        return Some(id.to_string()).filter(|id| !id.is_empty());
    }
    url::Url::parse(artwork_url)
        .ok()?
        .query_pairs()
        .find(|(k, _)| k == "tag")
        .map(|(_, v)| v.into_owned())
        .filter(|v| !v.is_empty())
}

/// Build an `artwork://` URL for a cached image.
///
/// On macOS/iOS/Linux the webview resolves `artwork://localhost/{id}.jpg`.
/// On Windows/Android it becomes `http://artwork.localhost/{id}.jpg`.
fn artwork_url(safe_id: &str) -> String {
    format!("http://artwork.localhost/{}.jpg", safe_id)
}

/// Register the `artwork` custom protocol with the Tauri builder.
///
/// This serves images from `artwork_cache_dir/{filename}` with the correct
/// MIME type. No scope config needed — the directory is hard-coded to the
/// app's artwork cache.
pub fn register_artwork_protocol(
    builder: tauri::Builder<tauri::Wry>,
    artwork_cache_dir: PathBuf,
) -> tauri::Builder<tauri::Wry> {
    builder.register_uri_scheme_protocol("artwork", move |_ctx, request| {
        let uri = request.uri().to_string();
        let path = request.uri().path().trim_start_matches('/');
        let safe_name: String = path
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '.')
            .collect();
        let file_path = artwork_cache_dir.join(&safe_name);

        match std::fs::read(&file_path) {
            Ok(bytes) => {
                tracing::debug!("Artwork served: {} ({} bytes)", uri, bytes.len());
                tauri::http::Response::builder()
                    .status(200)
                    .header("Content-Type", "image/jpeg")
                    .header("Cache-Control", "no-cache")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(bytes)
                    .unwrap()
            }
            Err(e) => {
                tracing::warn!("Artwork not found: {} -> {:?} ({})", uri, file_path, e);
                tauri::http::Response::builder()
                    .status(404)
                    .body(Vec::new())
                    .unwrap()
            }
        }
    })
}

/// Sanitise a Jellyfin ID so it is safe to use as a filename.
///
/// Jellyfin IDs are hex strings, so this is a no-op in practice, but
/// acts as a defence-in-depth guard against path traversal.
fn sanitise_id(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bragi-artwork-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    async fn store(dir: &Path, id: &str, version: &str) {
        let file = dir.join(format!("{id}.jpg"));
        tokio::fs::write(&file, b"img").await.unwrap();
        tokio::fs::write(version_path(&file), version)
            .await
            .unwrap();
    }

    #[test]
    fn version_is_the_tag_or_cover_id_never_the_host() {
        let lan = "http://192.168.1.5:8096/Items/abc/Images/Primary?maxWidth=300&maxHeight=300&quality=90&tag=t1";
        let wan = "https://jf.example.com/Items/abc/Images/Primary?maxWidth=300&maxHeight=300&quality=90&tag=t1";
        assert_eq!(artwork_version(lan).as_deref(), Some("t1"));
        assert_eq!(artwork_version(wan), artwork_version(lan));
        assert_eq!(
            artwork_version("subsonic-cover:al-9").as_deref(),
            Some("al-9")
        );
        assert_eq!(artwork_version("subsonic-cover:"), None);
        assert_eq!(artwork_version("http://h/Items/abc/Images/Primary"), None);
    }

    #[tokio::test]
    async fn same_version_is_a_cache_hit_changed_version_is_a_miss() {
        let dir = scratch("hit");
        let file = dir.join("abc.jpg");
        store(&dir, "abc", "tag-1").await;

        assert!(is_current(&file, "tag-1").await);
        assert!(!is_current(&file, "tag-2").await);
    }

    #[tokio::test]
    async fn image_without_version_file_is_a_miss() {
        let dir = scratch("noversion");
        let file = dir.join("abc.jpg");
        tokio::fs::write(&file, b"img").await.unwrap();

        assert!(!is_current(&file, "tag-1").await);
    }
}
