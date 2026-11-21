//! Input validation for download-related operations.
//!
//! Validates track and album IDs to prevent path traversal and injection attacks.

use crate::error::AppError;

/// Whether `id` is a safe opaque media item ID: non-empty, at most 64 chars,
/// only `[A-Za-z0-9_-]`. The charset rejects `.`, `/` and `\\` (path
/// traversal, SSRF) and accepts Jellyfin 32-hex / UUID ids and OpenSubsonic
/// base62 / coverArt ids.
pub fn is_valid_item_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Validates a track or album ID; `id_type` names it in the error.
pub fn validate_jellyfin_id(id: &str, id_type: &str) -> Result<(), AppError> {
    if is_valid_item_id(id) {
        Ok(())
    } else {
        Err(AppError::InvalidOperation(format!("Invalid {id_type} ID")))
    }
}

/// Sanitises a track ID for use in file paths.
///
/// Removes any characters that could cause path traversal or filesystem issues.
/// Returns only alphanumeric characters and hyphens.
///
/// # Arguments
/// * `track_id` - The track ID to sanitise
///
/// # Returns
/// A sanitised version of the track ID safe for filesystem use.
pub fn sanitise_for_path(track_id: &str) -> String {
    track_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_hex_id() {
        assert!(validate_jellyfin_id("a1b2c3d4e5f67890a1b2c3d4e5f67890", "track").is_ok());
    }

    #[test]
    fn test_valid_uuid_id() {
        assert!(validate_jellyfin_id("a1b2c3d4-e5f6-7890-a1b2-c3d4e5f67890", "track").is_ok());
    }

    #[test]
    fn test_empty_id() {
        assert!(validate_jellyfin_id("", "track").is_err());
    }

    #[test]
    fn test_path_traversal_slash() {
        assert!(validate_jellyfin_id("../../../etc/passwd", "track").is_err());
    }

    #[test]
    fn test_path_traversal_backslash() {
        assert!(validate_jellyfin_id("..\\..\\windows\\system32", "track").is_err());
    }

    #[test]
    fn test_path_traversal_dots() {
        assert!(validate_jellyfin_id("valid..id", "track").is_err());
    }

    #[test]
    fn test_accepts_subsonic_ids() {
        // OpenSubsonic base62 id + coverArt id (with '-' and '_').
        assert!(validate_jellyfin_id("2cahnhu6UPen2wbYDm4CHK", "track").is_ok());
        assert!(validate_jellyfin_id("al-2cahnhu6UPen2wbYDm4CHK_640a93a8", "album").is_ok());
    }

    #[test]
    fn test_rejects_overlong() {
        assert!(validate_jellyfin_id(&"a".repeat(65), "track").is_err());
    }

    #[test]
    fn test_rejects_illegal_chars() {
        // '.', space, and other non-[A-Za-z0-9_-] are rejected.
        assert!(validate_jellyfin_id("a1b2.c3d4", "track").is_err());
        assert!(validate_jellyfin_id("id with space", "track").is_err());
    }

    #[test]
    fn test_sanitise_for_path() {
        assert_eq!(sanitise_for_path("abc-123"), "abc-123");
        assert_eq!(sanitise_for_path("../../../etc/passwd"), "etcpasswd");
        assert_eq!(sanitise_for_path("track/with\\slashes"), "trackwithslashes");
    }
}
