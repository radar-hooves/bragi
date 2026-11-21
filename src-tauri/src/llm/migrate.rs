//! Moves a setup made with the retired Claudette preset onto the
//! OpenAI-compatible provider, at the endpoint, model and key it used, so AI
//! Playlists keep working after the update.

use crate::db::settings::keys;
use crate::db::Database;
use crate::error::AppError;

use super::{credentials, Provider};

/// What the Claudette preset filled in when a setting was left empty.
const CLAUDETTE_BASE_URL: &str = "http://localhost:8420";
const CLAUDETTE_MODEL: &str = "claude-sonnet-4-20250514";

/// Whether the saved provider is the retired Claudette preset.
fn is_claudette(db: &Database) -> Result<bool, AppError> {
    Ok(db.get_setting(keys::LLM_PROVIDER)?.as_deref() == Some("claudette"))
}

/// Rewrite the settings of a Claudette setup for the OpenAI-compatible provider.
fn migrate_settings(db: &Database) -> Result<(), AppError> {
    let kept = |key: &str, preset: &str| -> Result<String, AppError> {
        Ok(db
            .get_setting(key)?
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| preset.to_string()))
    };
    let base_url = kept(keys::LLM_BASE_URL, CLAUDETTE_BASE_URL)?;
    let model = kept(keys::LLM_MODEL, CLAUDETTE_MODEL)?;
    db.set_settings(&[
        (keys::LLM_BASE_URL, base_url.as_str()),
        (keys::LLM_MODEL, model.as_str()),
        (keys::LLM_PROVIDER, Provider::OpenAi.as_str()),
    ])?;
    Ok(())
}

/// Migrate a Claudette setup, once: the key first, so a failure leaves the
/// provider as it was and the next launch tries again. Returns whether it ran.
pub fn migrate_claudette(db: &Database) -> Result<bool, AppError> {
    if !is_claudette(db)? {
        return Ok(false);
    }
    credentials::adopt_claudette_key()?;
    migrate_settings(db)?;
    tracing::info!("Moved the Claudette preset onto the OpenAI-compatible provider");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setting(db: &Database, key: &str) -> Option<String> {
        db.get_setting(key).unwrap()
    }

    #[test]
    fn a_claudette_setup_keeps_its_endpoint_and_model() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting(keys::LLM_PROVIDER, "claudette").unwrap();
        db.set_setting(keys::LLM_BASE_URL, "http://claudette.lan:8420")
            .unwrap();
        db.set_setting(keys::LLM_MODEL, "claude-opus-4").unwrap();

        assert!(is_claudette(&db).unwrap());
        migrate_settings(&db).unwrap();

        assert_eq!(setting(&db, keys::LLM_PROVIDER).as_deref(), Some("openai"));
        assert_eq!(
            setting(&db, keys::LLM_BASE_URL).as_deref(),
            Some("http://claudette.lan:8420")
        );
        assert_eq!(
            setting(&db, keys::LLM_MODEL).as_deref(),
            Some("claude-opus-4")
        );
        assert!(!is_claudette(&db).unwrap());
    }

    #[test]
    fn a_claudette_setup_on_the_preset_defaults_gets_them_written_out() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting(keys::LLM_PROVIDER, "claudette").unwrap();
        db.set_setting(keys::LLM_BASE_URL, "").unwrap();

        migrate_settings(&db).unwrap();

        assert_eq!(
            setting(&db, keys::LLM_BASE_URL).as_deref(),
            Some(CLAUDETTE_BASE_URL)
        );
        assert_eq!(
            setting(&db, keys::LLM_MODEL).as_deref(),
            Some(CLAUDETTE_MODEL)
        );
        assert_eq!(
            Provider::parse(&setting(&db, keys::LLM_PROVIDER).unwrap()),
            Some(Provider::OpenAi)
        );
    }

    #[test]
    fn any_other_provider_is_left_alone() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting(keys::LLM_PROVIDER, "ollama").unwrap();
        assert!(!migrate_claudette(&db).unwrap());
        assert_eq!(setting(&db, keys::LLM_PROVIDER).as_deref(), Some("ollama"));
        assert_eq!(setting(&db, keys::LLM_BASE_URL), None);
    }
}
