//! Credential storage: one `desktop_shell::secrets::SecretStore`, its four
//! `bragi-*` services taken by `with_service` under one lock, so the entries
//! earlier releases wrote (keyring items and `credentials.json`, keyed by
//! `bragi-*` service) are read as they are. The kit's `for_app` would key
//! everything under the bundle identifier and strand them.

use crate::error::AppError;
use desktop_shell::secrets::SecretStore;
use std::path::PathBuf;
use std::sync::Mutex;

/// The store for the current data directory, shared so the four services take
/// one lock. The directory never changes in the app; a test that points HOME
/// elsewhere gets a store of its own rather than whichever one came first.
fn store(service: &str) -> Result<SecretStore, AppError> {
    static ROOT: Mutex<Option<(PathBuf, SecretStore)>> = Mutex::new(None);
    let path = dirs::data_dir()
        .ok_or_else(|| AppError::Config("Unable to determine data directory".to_string()))?
        .join(crate::app_id::APP_ID)
        .join("credentials.json");
    let mut root = ROOT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((shared, store)) = root.as_ref() {
        if *shared == path {
            return Ok(store.with_service(service));
        }
    }
    let store = SecretStore::new("bragi-jellyfin", path.clone());
    let scoped = store.with_service(service);
    *root = Some((path, store));
    Ok(scoped)
}

fn reported(e: std::io::Error) -> AppError {
    let e = AppError::Config(format!("Credential store: {e}"));
    telemetry::report_error_with_cause("credential or keyring failure", &e);
    e
}

/// Store a value; `service` and `key` identify the entry.
pub fn credential_set(service: &str, key: &str, value: &str) -> Result<(), AppError> {
    store(service)?.set(key, value).map_err(reported)
}

/// Retrieve a value; `Ok(None)` if absent.
pub fn credential_get(service: &str, key: &str) -> Result<Option<String>, AppError> {
    store(service)?.get(key).map_err(reported)
}

/// Delete a value; idempotent.
pub fn credential_delete(service: &str, key: &str) -> Result<(), AppError> {
    store(service)?.delete(key).map_err(reported)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV: Mutex<()> = Mutex::new(());

    /// The shared store follows the data directory: pointed at a second one, it
    /// reads that directory's file, not the first one it opened.
    #[test]
    fn the_shared_store_follows_the_data_directory() {
        let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
        for (dir, value) in [("a", "first"), ("b", "second")] {
            let home =
                std::env::temp_dir().join(format!("bragi-cred-dir-{}-{dir}", std::process::id()));
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
            let path = dirs::data_dir()
                .expect("data dir")
                .join(crate::app_id::APP_ID)
                .join("credentials.json");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, format!(r#"{{"bragi":{{"k":"{value}"}}}}"#)).unwrap();
            assert_eq!(
                credential_get("bragi", "k").unwrap().as_deref(),
                Some(value)
            );
            let _ = std::fs::remove_dir_all(&home);
        }
    }

    /// A `credentials.json` written by release 2026.10.9 (before the keyring 4
    /// and dirs 7 upgrades) must still be read, and written back unchanged.
    #[test]
    fn credentials_written_by_the_previous_release_are_still_read() {
        let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let home = std::env::temp_dir().join(format!("bragi-cred-test-{}", std::process::id()));
        let data = home.join(".local/share");
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_DATA_HOME", &data);
        let path = dirs::data_dir()
            .expect("data dir")
            .join(crate::app_id::APP_ID)
            .join("credentials.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"bragi":{"jellyfin_credentials":"{\"token\":\"t\"}","device_id":"d"},"bragi-llm":{"credentials":"{\"openai\":\"sk-previous\"}"}}"#,
        )
        .unwrap();

        assert_eq!(
            credential_get("bragi", "jellyfin_credentials")
                .unwrap()
                .as_deref(),
            Some("{\"token\":\"t\"}")
        );
        credential_set("bragi", "other", "v").unwrap();
        assert_eq!(
            credential_get("bragi", "device_id").unwrap().as_deref(),
            Some("d")
        );
        assert_eq!(
            credential_get("bragi", "other").unwrap().as_deref(),
            Some("v")
        );
        // The AI Playlists provider key saved by the previous release resolves,
        // and a new key joins it in the same consolidated entry.
        use crate::llm::{credentials, provider::Provider};
        assert_eq!(
            credentials::resolve_key(Provider::OpenAi).unwrap(),
            "sk-previous"
        );
        credentials::set_key(Provider::Anthropic, Some("sk-new".into())).unwrap();
        assert_eq!(
            credentials::resolve_key(Provider::Anthropic).unwrap(),
            "sk-new"
        );
        assert_eq!(
            credentials::resolve_key(Provider::OpenAi).unwrap(),
            "sk-previous"
        );
        credential_delete("bragi", "other").unwrap();
        assert_eq!(credential_get("bragi", "other").unwrap(), None);
        let _ = std::fs::remove_dir_all(&home);
    }
}
