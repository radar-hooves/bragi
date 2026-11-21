//! Bridges bragi's own settings persistence to `tauri-plugin-telemetry`'s
//! [`TelemetryStore`] seam — the plugin owns the live exporter, this app
//! still owns where the two saved values live (the settings table it always
//! had).
//!
//! The store is handed to the plugin's `init()` when the builder chain is
//! constructed, before the app's own `setup` hook opens the database (Tauri
//! runs every plugin's `setup` before the app's own), so it cannot hold the
//! database directly. It holds [`DbCell`] instead, a cell the app's `setup`
//! hook fills in once `Database::open` returns.

use std::sync::{Arc, OnceLock};

use tauri_plugin_telemetry::{Exporter, TelemetryStore};

use crate::db::settings::keys;
use crate::db::Database;

/// Filled in by the app's `setup` hook once the database is open.
pub type DbCell = Arc<OnceLock<Arc<Database>>>;

pub struct BragiTelemetryStore(pub DbCell);

impl TelemetryStore for BragiTelemetryStore {
    fn load(&self) -> Option<Exporter> {
        let db = self.0.get()?;
        let endpoint = db.get_setting(keys::TELEMETRY_ENDPOINT).ok().flatten()?;
        let endpoint = endpoint.trim();
        if endpoint.is_empty() {
            return None;
        }
        let headers_helper = db
            .get_setting(keys::TELEMETRY_HEADERS_HELPER)
            .ok()
            .flatten()
            .map(|h| h.trim().to_owned())
            .filter(|h| !h.is_empty());
        Some(Exporter {
            endpoint: endpoint.to_owned(),
            headers_helper,
        })
    }

    fn save(&self, exporter: Option<Exporter>) -> Result<(), String> {
        let db = self
            .0
            .get()
            .ok_or_else(|| "the database is not open yet".to_string())?;
        let (endpoint, headers_helper) = match exporter {
            Some(e) => (e.endpoint, e.headers_helper.unwrap_or_default()),
            None => (String::new(), String::new()),
        };
        db.set_settings(&[
            (keys::TELEMETRY_ENDPOINT, endpoint.as_str()),
            (keys::TELEMETRY_HEADERS_HELPER, headers_helper.as_str()),
        ])
        .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with_open_db() -> (BragiTelemetryStore, DbCell) {
        let cell: DbCell = Arc::new(OnceLock::new());
        let set = cell.set(Arc::new(Database::open_in_memory().unwrap()));
        assert!(set.is_ok());
        (BragiTelemetryStore(cell.clone()), cell)
    }

    #[test]
    fn load_before_the_database_opens_is_none() {
        let store = BragiTelemetryStore(Arc::new(OnceLock::new()));
        assert!(store.load().is_none());
    }

    #[test]
    fn save_before_the_database_opens_is_an_error() {
        let store = BragiTelemetryStore(Arc::new(OnceLock::new()));
        assert!(store.save(None).is_err());
    }

    #[test]
    fn round_trips_a_saved_exporter() {
        let (store, _cell) = store_with_open_db();
        let exporter = Exporter {
            endpoint: "https://otlp.example".to_owned(),
            headers_helper: Some("headers-helper".to_owned()),
        };
        store.save(Some(exporter.clone())).unwrap();
        assert_eq!(store.load(), Some(exporter));
    }

    #[test]
    fn saving_none_clears_both_saved_values() {
        let (store, _cell) = store_with_open_db();
        store
            .save(Some(Exporter {
                endpoint: "https://otlp.example".to_owned(),
                headers_helper: None,
            }))
            .unwrap();
        store.save(None).unwrap();
        assert!(store.load().is_none());
    }
}
