//! Credentials management for the music service.
//!
//! Securely stores API credentials in the system keyring.
//! All fields are stored as a single JSON entry to minimise
//! macOS Keychain access prompts (one prompt per entry).

use crate::error::AppError;

/// Keyring service name for credential storage.
const KEYRING_SERVICE: &str = "bragi-music-service";

/// Keyring account key for the consolidated credentials entry.
const CREDENTIALS_KEY: &str = "credentials";

/// All music service credentials serialised as a single keyring entry.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredCredentials {
    service_url: String,
    api_token: String,
}

/// Credentials for connecting to the music service.
#[derive(Clone)]
pub struct MusicServiceCredentials {
    /// Base URL of the music service (e.g. "https://music.example.com").
    pub service_url: String,

    /// API token for authentication.
    pub api_token: String,
}

// Custom Debug implementation to redact API token
impl std::fmt::Debug for MusicServiceCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MusicServiceCredentials")
            .field("service_url", &self.service_url)
            .field("api_token", &"[REDACTED]")
            .finish()
    }
}

/// Store music service credentials securely in the system keyring.
///
/// On macOS this uses Keychain; on Windows, Credential Manager;
/// on Linux, Secret Service (e.g. GNOME Keyring).
pub fn store_music_service_credentials(
    credentials: &MusicServiceCredentials,
) -> Result<(), AppError> {
    let stored = StoredCredentials {
        service_url: credentials.service_url.clone(),
        api_token: credentials.api_token.clone(),
    };
    let json = serde_json::to_string(&stored)
        .map_err(|e| AppError::Config(format!("Failed to serialise credentials: {e}")))?;

    keyring_set(CREDENTIALS_KEY, &json)?;
    tracing::info!("Stored music service credentials in system keyring");
    Ok(())
}

/// Load music service credentials from the system keyring.
///
/// Returns `None` if credentials haven't been stored yet.
/// Automatically migrates legacy per-key entries to the new consolidated format.
pub fn load_music_service_credentials() -> Result<Option<MusicServiceCredentials>, AppError> {
    // Try consolidated entry first.
    if let Some(json) = keyring_get(CREDENTIALS_KEY)? {
        let stored: StoredCredentials = serde_json::from_str(&json)
            .map_err(|e| AppError::Config(format!("Failed to parse stored credentials: {e}")))?;
        return Ok(Some(MusicServiceCredentials {
            service_url: stored.service_url,
            api_token: stored.api_token,
        }));
    }

    // Fall back to legacy per-key entries and migrate if found.
    migrate_legacy_credentials()
}

/// Clear stored music service credentials.
pub fn clear_music_service_credentials() -> Result<(), AppError> {
    keyring_delete(CREDENTIALS_KEY)?;
    // Also clean up any leftover legacy entries.
    for key in ["service_url", "api_token"] {
        keyring_delete(key)?;
    }
    tracing::info!("Cleared music service credentials from system keyring");
    Ok(())
}

/// Migrate legacy per-key credential entries to the consolidated format.
///
/// If both required keys exist, stores them as a single JSON blob
/// and deletes the old entries. Returns `None` if no legacy entries found.
fn migrate_legacy_credentials() -> Result<Option<MusicServiceCredentials>, AppError> {
    let service_url = match keyring_get("service_url")? {
        Some(v) => v,
        None => return Ok(None),
    };
    let api_token = match keyring_get("api_token")? {
        Some(v) => v,
        None => return Ok(None),
    };

    let stored = StoredCredentials {
        service_url: service_url.clone(),
        api_token: api_token.clone(),
    };
    let json = serde_json::to_string(&stored)
        .map_err(|e| AppError::Config(format!("Failed to serialise credentials: {e}")))?;
    keyring_set(CREDENTIALS_KEY, &json)?;

    // Clean up legacy entries.
    for key in ["service_url", "api_token"] {
        keyring_delete(key)?;
    }

    tracing::info!("Migrated legacy music service credentials to consolidated format");
    Ok(Some(MusicServiceCredentials {
        service_url,
        api_token,
    }))
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
