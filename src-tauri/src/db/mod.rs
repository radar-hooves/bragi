//! Database module for SQLite local cache.
//!
//! Provides offline-first storage for library metadata, sync queue,
//! and user settings. Uses SQLite for cross-platform persistence.

mod albums;
mod artists;
mod imported_playlists;
mod jellyfin_playlists;
mod schema;
pub mod settings;
mod stats;
mod sync_queue;
mod tracks;
pub mod utils;

pub use albums::*;
pub use artists::*;
pub use settings::keys;
pub use stats::*;
pub use sync_queue::*;
pub use tracks::*;
pub use utils::current_timestamp;

use crate::mediaserver::ServerKind;
use rusqlite::{Connection, OptionalExtension};
use std::path::PathBuf;
use std::sync::Mutex;

/// Database connection wrapper with thread-safe access.
///
/// SQLite connections are not thread-safe, so we wrap in a Mutex.
/// For Tauri commands, each command should create its own connection
/// or use this shared instance with the mutex.
pub struct Database {
    /// Shared connection for synchronous operations.
    conn: Mutex<Connection>,
    /// Currently-active media-server backend for cache scoping.
    ///
    /// Interior-mutable so `set_active_source` can flip it on connect/toggle
    /// without rebuilding the `Database`. Every cache read filters
    /// `WHERE source = active_source` so the two backends' libraries stay
    /// sandboxed while both remain cached side-by-side. Independent of the
    /// `conn` mutex, so reads snapshot it *before* acquiring the connection.
    active_source: Mutex<ServerKind>,
}

impl Database {
    /// Open or create a database at the specified path.
    ///
    /// Initialises the schema if the database is new or needs migration.
    /// Configures SQLite for optimal performance and reliability:
    /// - WAL mode for concurrent reads during writes
    /// - Busy timeout to handle lock contention
    /// - Foreign key enforcement
    pub fn open(path: PathBuf) -> Result<Self, DatabaseError> {
        tracing::info!("Opening database at: {}", path.display());

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DatabaseError::Io(format!("Failed to create database directory: {}", e))
            })?;
        }

        let conn = Connection::open(&path)?;

        // Configure SQLite for optimal performance and reliability
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA foreign_keys = ON;",
        )?;

        crate::ordering::register_collations(&conn)?;

        // Initialise schema
        schema::initialise_schema(&conn)?;

        // Without sqlite_stat1 the planner has no way to tell a 2-value index
        // (idx_tracks_source) from a selective one, and it guessed wrong on the
        // album<->track join: 7.5s to list a 2k-album library, against 6ms once
        // stats exist. `analysis_limit` caps the sampling so this stays bounded
        // as the library grows (~50ms on 24k tracks); `optimize` is a no-op when
        // the stats are already fresh, so paying it per open costs nothing.
        conn.execute_batch(
            "PRAGMA analysis_limit = 400;
             PRAGMA optimize;",
        )?;

        // Seed the active backend from the persisted SERVER_TYPE so the very
        // first cache read after startup filters to the same backend that
        // `AppState::new_without_audio` loads the client for. Read off
        // `conn` directly — `get_setting` needs `&self`, which doesn't exist yet.
        let active_source = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [settings::keys::SERVER_TYPE],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .unwrap_or(None)
            .and_then(|s| ServerKind::parse(&s))
            .unwrap_or(ServerKind::Jellyfin);

        Ok(Self {
            conn: Mutex::new(conn),
            active_source: Mutex::new(active_source),
        })
    }

    /// Open an in-memory database (for testing).
    ///
    /// Note: WAL mode is not supported for in-memory databases,
    /// so we only enable foreign keys and busy timeout.
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, DatabaseError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "PRAGMA busy_timeout = 5000;
             PRAGMA foreign_keys = ON;",
        )?;
        crate::ordering::register_collations(&conn)?;
        schema::initialise_schema(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
            active_source: Mutex::new(ServerKind::Jellyfin),
        })
    }

    /// Execute a function with the database connection.
    ///
    /// This acquires the mutex lock and provides access to the connection.
    pub fn with_conn<F, T>(&self, f: F) -> Result<T, DatabaseError>
    where
        F: FnOnce(&Connection) -> Result<T, rusqlite::Error>,
    {
        let conn = self
            .conn
            .lock()
            .map_err(|_| DatabaseError::Lock("Failed to acquire database lock".to_string()))?;
        f(&conn).map_err(DatabaseError::from)
    }

    /// Execute a function with a mutable database connection.
    ///
    /// Use this for operations that require transaction support.
    pub fn with_conn_mut<F, T>(&self, f: F) -> Result<T, DatabaseError>
    where
        F: FnOnce(&mut Connection) -> Result<T, rusqlite::Error>,
    {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| DatabaseError::Lock("Failed to acquire database lock".to_string()))?;
        f(&mut conn).map_err(DatabaseError::from)
    }

    /// The active backend used to scope every cache read.
    ///
    /// Poisoned-lock fallback is `Jellyfin` (the safe default for existing
    /// installs); a poisoned mutex only happens if a holder panicked, which
    /// none of the trivial set/get bodies can.
    pub fn active_source(&self) -> ServerKind {
        self.active_source
            .lock()
            .map(|g| *g)
            .unwrap_or(ServerKind::Jellyfin)
    }

    /// Switch the active backend; subsequent reads/upserts are scoped to it.
    ///
    /// Kept in lock-step with the persisted `SERVER_TYPE` setting by every
    /// caller (login, the `set_active_backend` toggle, and startup seeding).
    pub fn set_active_source(&self, source: ServerKind) {
        if let Ok(mut g) = self.active_source.lock() {
            *g = source;
        }
    }

    /// Get cache statistics for the active backend.
    ///
    /// Entity counts (albums/tracks/artists) are scoped to the active source so
    /// the reported library size matches the sandboxed view the user is
    /// browsing. `pending_sync_count` stays global — the sync queue has no
    /// source column and scrobbles/skips are backend-agnostic.
    pub fn get_cache_stats(&self) -> Result<CacheStats, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| {
            let album_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM cached_albums WHERE source = ?1",
                [source],
                |row| row.get(0),
            )?;
            let track_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM cached_tracks WHERE source = ?1",
                [source],
                |row| row.get(0),
            )?;
            let artist_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM cached_artists WHERE source = ?1",
                [source],
                |row| row.get(0),
            )?;
            let pending_sync_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sync_queue WHERE synced_at IS NULL",
                [],
                |row| row.get(0),
            )?;

            Ok(CacheStats {
                album_count: album_count as u32,
                track_count: track_count as u32,
                artist_count: artist_count as u32,
                pending_sync_count: pending_sync_count as u32,
            })
        })
    }
}

/// Cache statistics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    pub album_count: u32,
    pub track_count: u32,
    pub artist_count: u32,
    pub pending_sync_count: u32,
}

/// Database error types.
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("I/O error: {0}")]
    Io(String),

    #[error("Lock error: {0}")]
    Lock(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Serialisation error: {0}")]
    Serialisation(String),
}

impl From<DatabaseError> for String {
    fn from(err: DatabaseError) -> String {
        err.to_string()
    }
}

/// Get the default database path for the current platform.
///
/// Uses Tauri's app data directory for cross-platform consistency.
pub fn get_database_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, DatabaseError> {
    use tauri::Manager;

    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| DatabaseError::Io(format!("Failed to get app data directory: {}", e)))?;

    Ok(app_data_dir.join("bragi.db"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_creation() {
        let db = Database::open_in_memory().unwrap();
        let stats = db.get_cache_stats().unwrap();

        assert_eq!(stats.album_count, 0);
        assert_eq!(stats.track_count, 0);
        assert_eq!(stats.artist_count, 0);
        assert_eq!(stats.pending_sync_count, 0);
    }
}
