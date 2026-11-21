//! LLM API-key storage.
//!
//! Keys live in the system keyring (release) / dev file store (debug), as a
//! single consolidated JSON entry to minimise macOS Keychain prompts — the same
//! pattern as `services::credentials`. Keys are NEVER stored in settings and
//! NEVER returned to the frontend (see `commands::ai_playlist`).

use super::provider::Provider;
use crate::error::AppError;
use std::time::Duration;

/// Keyring service name for LLM credentials.
const KEYRING_SERVICE: &str = "bragi-llm";

/// Keyring account key for the consolidated credentials entry.
const CREDENTIALS_KEY: &str = "credentials";

/// Hard limit for a keyring read issued from an async command. A stalled macOS
/// Keychain authorization prompt is a *blocking* synchronous OS call that never
/// returns until the user answers it; two seconds is ample for a real read yet
/// short enough that a pending/ignored prompt can't wedge the UI.
const KEYRING_READ_TIMEOUT: Duration = Duration::from_secs(2);

/// All per-provider LLM keys serialised as one keyring entry.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct StoredKeys {
    #[serde(skip_serializing_if = "Option::is_none")]
    openai: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    anthropic: Option<String>,
    /// The key of the Claudette preset, read so [`adopt_claudette_key`] can
    /// carry it to the OpenAI-compatible provider, and never written back.
    #[serde(default, skip_serializing)]
    claudette: Option<String>,
}

impl StoredKeys {
    fn get(&self, provider: Provider) -> Option<&String> {
        match provider {
            Provider::OpenAi => self.openai.as_ref(),
            Provider::Anthropic => self.anthropic.as_ref(),
            Provider::Ollama => None,
        }
    }

    fn set(&mut self, provider: Provider, value: Option<String>) {
        match provider {
            Provider::OpenAi => self.openai = value,
            Provider::Anthropic => self.anthropic = value,
            Provider::Ollama => {}
        }
    }
}

fn load_all() -> Result<StoredKeys, AppError> {
    match crate::credential_store::credential_get(KEYRING_SERVICE, CREDENTIALS_KEY)? {
        Some(json) => serde_json::from_str(&json)
            .map_err(|e| AppError::Config(format!("Failed to parse stored LLM keys: {e}"))),
        None => Ok(StoredKeys::default()),
    }
}

fn save_all(keys: &StoredKeys) -> Result<(), AppError> {
    let json = serde_json::to_string(keys)
        .map_err(|e| AppError::Config(format!("Failed to serialise LLM keys: {e}")))?;
    crate::credential_store::credential_set(KEYRING_SERVICE, CREDENTIALS_KEY, &json)
}

/// Store (or, with `None`, clear) the API key for a provider.
pub fn set_key(provider: Provider, value: Option<String>) -> Result<(), AppError> {
    let mut keys = load_all()?;
    keys.set(provider, value.filter(|v| !v.is_empty()));
    save_all(&keys)?;
    tracing::info!("Updated LLM API key for provider {}", provider.as_str());
    Ok(())
}

/// Resolve the API key to use for a provider: the stored key, else empty.
pub fn resolve_key(provider: Provider) -> Result<String, AppError> {
    Ok(load_all()?.get(provider).cloned().unwrap_or_default())
}

/// The label the Claudette preset sent as its key when none was stored.
const CLAUDETTE_DEFAULT_KEY: &str = "bragi";

/// Give the OpenAI-compatible provider the key a Claudette setup used, unless it
/// already holds one of its own. Returns whether `keys` changed.
fn adopt_claudette(keys: &mut StoredKeys) -> bool {
    let legacy = keys.claudette.take();
    if keys.openai.is_some() {
        return legacy.is_some();
    }
    keys.openai = Some(legacy.unwrap_or_else(|| CLAUDETTE_DEFAULT_KEY.to_string()));
    true
}

/// Carry a Claudette setup's key to the OpenAI-compatible provider and drop the
/// Claudette entry (see [`crate::llm::migrate`]).
pub fn adopt_claudette_key() -> Result<(), AppError> {
    let mut keys = load_all()?;
    if adopt_claudette(&mut keys) {
        save_all(&keys)?;
    }
    Ok(())
}

/// Whether a non-default key is stored for a provider (drives the settings UI's
/// "key set" indicator without exposing the value).
pub fn has_key(provider: Provider) -> Result<bool, AppError> {
    Ok(load_all()?.get(provider).is_some())
}

/// Async, non-blocking variant of [`has_key`] for use inside Tauri commands.
///
/// [`has_key`] performs a synchronous keyring read. On macOS an unanswered
/// Keychain prompt blocks that read indefinitely, so calling it inline from an
/// `async fn` (as `get_settings` did) let a single stalled prompt hang the whole
/// command future — the "AI Playlist button never appears" regression.
/// Here the read runs on the blocking thread pool under a timeout; a timeout,
/// join failure, or error all resolve to `false` ("no stored key") so the caller
/// always makes progress. Callers should still skip this entirely for providers
/// that need no key (see `Provider::needs_api_key`).
pub async fn has_key_timeout(provider: Provider) -> bool {
    let handle = tokio::task::spawn_blocking(move || has_key(provider));
    match tokio::time::timeout(KEYRING_READ_TIMEOUT, handle).await {
        Ok(Ok(Ok(present))) => present,
        Ok(Ok(Err(e))) => {
            tracing::warn!("LLM keyring lookup failed: {e}");
            false
        }
        Ok(Err(e)) => {
            tracing::warn!("LLM keyring lookup task failed to join: {e}");
            false
        }
        Err(_) => {
            tracing::warn!(
                "LLM keyring lookup timed out after {}s (a macOS Keychain prompt may be pending); treating as no stored key",
                KEYRING_READ_TIMEOUT.as_secs()
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A keyring-backed lookup must resolve well within the timeout and yield a
    /// boolean rather than hanging the caller — the core guarantee.
    #[tokio::test]
    async fn has_key_timeout_returns_promptly() {
        let start = std::time::Instant::now();
        let _present = has_key_timeout(Provider::OpenAi).await;
        assert!(
            start.elapsed() < KEYRING_READ_TIMEOUT,
            "has_key_timeout should return well within the timeout"
        );
    }

    /// Ollama stores no key, so the lookup is always false regardless of what a
    /// secret backend holds.
    #[tokio::test]
    async fn has_key_timeout_false_for_ollama() {
        assert!(!has_key_timeout(Provider::Ollama).await);
    }

    fn stored(json: &str) -> StoredKeys {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_claudette_key_moves_to_the_openai_provider() {
        let mut keys = stored(r#"{"claudette":"label"}"#);
        assert!(adopt_claudette(&mut keys));
        assert_eq!(
            keys.get(Provider::OpenAi).map(String::as_str),
            Some("label")
        );
        assert_eq!(
            serde_json::to_string(&keys).unwrap(),
            r#"{"openai":"label"}"#
        );
    }

    #[test]
    fn a_claudette_setup_with_no_stored_key_gets_the_label_it_sent() {
        let mut keys = stored("{}");
        assert!(adopt_claudette(&mut keys));
        assert_eq!(
            keys.get(Provider::OpenAi).map(String::as_str),
            Some(CLAUDETTE_DEFAULT_KEY)
        );
    }

    #[test]
    fn an_openai_key_already_stored_is_kept() {
        let mut keys = stored(r#"{"openai":"own","claudette":"label"}"#);
        assert!(adopt_claudette(&mut keys));
        assert_eq!(keys.get(Provider::OpenAi).map(String::as_str), Some("own"));
        assert_eq!(serde_json::to_string(&keys).unwrap(), r#"{"openai":"own"}"#);

        let mut untouched = stored(r#"{"openai":"own"}"#);
        assert!(!adopt_claudette(&mut untouched));
    }
}
