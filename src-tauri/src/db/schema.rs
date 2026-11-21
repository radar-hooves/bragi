//! Database schema creation and migrations.
//!
//! Manages SQLite schema versioning and migrations for the local cache.

use rusqlite::Connection;

/// Current schema version.
const SCHEMA_VERSION: i32 = 17;

/// Initial schema creation SQL.
const SCHEMA_V1: &str = r#"
-- Library metadata cache (from Jellyfin)
CREATE TABLE IF NOT EXISTS cached_albums (
    jellyfin_id TEXT PRIMARY KEY,
    release_mbid TEXT,
    name TEXT NOT NULL,
    artist_name TEXT,
    artist_jellyfin_id TEXT,
    year INTEGER,
    genre TEXT,
    artwork_url TEXT,
    track_count INTEGER,
    duration_ms INTEGER,
    last_synced_at INTEGER NOT NULL,
    -- Media-server backend this row belongs to: 'jellyfin' | 'opensubsonic'
    source TEXT NOT NULL DEFAULT 'jellyfin',
    -- When the SERVER first saw this album (Jellyfin DateCreated / Subsonic
    -- created), epoch seconds. Distinct from last_synced_at, which every sync
    -- rewrites library-wide and so cannot express "recently added".
    date_created INTEGER
);

CREATE TABLE IF NOT EXISTS cached_tracks (
    jellyfin_id TEXT PRIMARY KEY,
    track_mbid TEXT,
    artist_mbid TEXT,
    release_mbid TEXT,
    name TEXT NOT NULL,
    artist_name TEXT,
    album_jellyfin_id TEXT REFERENCES cached_albums(jellyfin_id) ON DELETE CASCADE,
    track_number INTEGER,
    disc_number INTEGER,
    duration_ms INTEGER,
    genre TEXT,
    artwork_url TEXT,
    -- Volume normalisation (ReplayGain from Jellyfin)
    normalization_gain_db REAL,
    -- Local cache info
    cached_file_path TEXT,
    cached_at INTEGER,
    file_size_bytes INTEGER,
    -- Server-side play count reported by the media server for this row's source
    -- (Jellyfin UserData.PlayCount / OpenSubsonic inline playCount). Read-only;
    -- source-scoped via the `source` column so backends never blend.
    server_play_count INTEGER,
    last_synced_at INTEGER NOT NULL,
    -- Media-server backend this row belongs to: 'jellyfin' | 'opensubsonic'
    source TEXT NOT NULL DEFAULT 'jellyfin'
);

CREATE TABLE IF NOT EXISTS cached_artists (
    jellyfin_id TEXT PRIMARY KEY,
    artist_mbid TEXT,
    name TEXT NOT NULL,
    artwork_url TEXT,
    backdrop_url TEXT,
    album_count INTEGER,
    last_synced_at INTEGER NOT NULL,
    -- Media-server backend this row belongs to: 'jellyfin' | 'opensubsonic'
    source TEXT NOT NULL DEFAULT 'jellyfin',
    -- Jellyfin's own SortName (e.g. "Beatles" for "The Beatles"), NULL on a
    -- backend that has none (OpenSubsonic). The ordering model
    -- (src-tauri/src/ordering.rs) prefers this over stripping the article
    -- itself when present.
    sort_name TEXT
);

-- Offline sync queue (pending operations for scrobble service)
CREATE TABLE IF NOT EXISTS sync_queue (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    operation TEXT NOT NULL,  -- 'scrobble', 'skip'
    payload TEXT NOT NULL,    -- JSON payload
    created_at INTEGER NOT NULL,
    attempts INTEGER DEFAULT 0,
    last_attempt_at INTEGER,
    synced_at INTEGER,
    -- Set on a scrobble held back for lacking a MusicBrainz recording ID at
    -- queue time (the scrobble server rejects these rather than storing them).
    -- Exempt from the attempts/backoff gate; `last_attempt_at` is reused as
    -- a re-resolve cooldown stamp instead.
    held_no_mbid INTEGER
);

-- Track stats cache (from the scrobble server)
CREATE TABLE IF NOT EXISTS track_stats (
    track_mbid TEXT PRIMARY KEY,
    play_count INTEGER NOT NULL DEFAULT 0,
    skip_count INTEGER NOT NULL DEFAULT 0,
    last_played_at INTEGER,
    last_skipped_at INTEGER,
    fetched_at INTEGER NOT NULL
);

-- User preferences
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Playlist detail cache (offline-first + instant first-click reopen).
-- cached_playlists is a presence marker so a legitimately EMPTY playlist is
-- distinguishable from a never-cached one; cached_playlist_tracks holds the
-- ordered tracks. Both are refreshed on every fetch/mutation and scoped by
-- `source` so a dual-connected user's Jellyfin and OpenSubsonic playlists never
-- collide on id (the PK includes source).
CREATE TABLE IF NOT EXISTS cached_playlists (
    playlist_id TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'jellyfin',
    last_synced_at INTEGER NOT NULL,
    PRIMARY KEY (source, playlist_id)
);

CREATE TABLE IF NOT EXISTS cached_playlist_tracks (
    playlist_id TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'jellyfin',
    position INTEGER NOT NULL,
    jellyfin_id TEXT NOT NULL,
    -- MusicBrainz recording ID, for resolving shared/imported playlists.
    mbid TEXT,
    playlist_item_id TEXT NOT NULL,
    name TEXT NOT NULL,
    artist_name TEXT NOT NULL,
    album_name TEXT NOT NULL,
    album_jellyfin_id TEXT NOT NULL,
    duration_ms INTEGER NOT NULL,
    artwork_url TEXT,
    PRIMARY KEY (source, playlist_id, position)
);

-- Shared/imported playlists: a local-only, read-only snapshot of a
-- playlist exported from someone else's Bragi instance. Tracks are keyed by
-- MusicBrainz recording ID (the canonical cross-instance identifier) and
-- re-resolved against THIS library's cached_tracks on every read, so
-- resolution stays current as the local library changes — never written back
-- to this instance's own media server.
CREATE TABLE IF NOT EXISTS imported_playlists (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    imported_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS imported_playlist_tracks (
    playlist_id TEXT NOT NULL REFERENCES imported_playlists(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    mbid TEXT,
    title TEXT NOT NULL,
    artist_name TEXT NOT NULL,
    album_name TEXT,
    duration_ms INTEGER,
    PRIMARY KEY (playlist_id, position)
);

-- Normalised genre memberships, one row per (track, genre).
--
-- cached_tracks.genre keeps the RAW first tag string as an audit trail; this
-- table is what every genre listing and filter reads. genre_key is case-folded
-- for matching, genre_display carries the casing to show. `source` records how
-- the membership arrived: 'tag' from an ingest, 'derived' from the v15 -> v16
-- back-derivation of pre-existing rows.
CREATE TABLE IF NOT EXISTS track_genres (
    track_id TEXT NOT NULL REFERENCES cached_tracks(jellyfin_id) ON DELETE CASCADE,
    genre_key TEXT NOT NULL,
    genre_display TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'tag',
    PRIMARY KEY (track_id, genre_key)
);

-- Indexes for common queries
CREATE INDEX IF NOT EXISTS idx_tracks_album ON cached_tracks(album_jellyfin_id);
CREATE INDEX IF NOT EXISTS idx_tracks_mbid ON cached_tracks(track_mbid);
CREATE INDEX IF NOT EXISTS idx_albums_artist ON cached_albums(artist_jellyfin_id);
CREATE INDEX IF NOT EXISTS idx_tracks_artist_album ON cached_tracks(artist_name, album_jellyfin_id);
CREATE INDEX IF NOT EXISTS idx_albums_sort ON cached_albums(artist_name, year, name);
CREATE INDEX IF NOT EXISTS idx_artists_name ON cached_artists(name);
CREATE INDEX IF NOT EXISTS idx_tracks_album_cached ON cached_tracks(album_jellyfin_id, cached_file_path) WHERE cached_file_path IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_sync_queue_pending ON sync_queue(synced_at) WHERE synced_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_sync_queue_operation ON sync_queue(operation);
CREATE INDEX IF NOT EXISTS idx_playlist_tracks_pid ON cached_playlist_tracks(playlist_id);
CREATE INDEX IF NOT EXISTS idx_playlists_source ON cached_playlists(source);
CREATE INDEX IF NOT EXISTS idx_imported_playlist_tracks_pid ON imported_playlist_tracks(playlist_id);
CREATE INDEX IF NOT EXISTS idx_imported_playlist_tracks_mbid ON imported_playlist_tracks(mbid);
-- Backend scoping for dual-connected libraries
CREATE INDEX IF NOT EXISTS idx_track_genres_key ON track_genres(genre_key);
CREATE INDEX IF NOT EXISTS idx_tracks_source ON cached_tracks(source);
CREATE INDEX IF NOT EXISTS idx_albums_source ON cached_albums(source);
-- Serves the recently-added window: source equality, then a descending scan
-- that satisfies ORDER BY ... DESC LIMIT without a temp B-tree.
CREATE INDEX IF NOT EXISTS idx_albums_date_created ON cached_albums(source, date_created DESC);
CREATE INDEX IF NOT EXISTS idx_artists_source ON cached_artists(source);

-- Full-text search for library
CREATE VIRTUAL TABLE IF NOT EXISTS fts_tracks USING fts5(
    jellyfin_id,
    name,
    artist_name,
    content='cached_tracks',
    content_rowid='rowid'
);

CREATE VIRTUAL TABLE IF NOT EXISTS fts_albums USING fts5(
    jellyfin_id,
    name,
    artist_name,
    content='cached_albums',
    content_rowid='rowid'
);

CREATE VIRTUAL TABLE IF NOT EXISTS fts_artists USING fts5(
    jellyfin_id,
    name,
    content='cached_artists',
    content_rowid='rowid'
);

-- Triggers to keep FTS in sync
CREATE TRIGGER IF NOT EXISTS tracks_ai AFTER INSERT ON cached_tracks BEGIN
    INSERT INTO fts_tracks(rowid, jellyfin_id, name, artist_name)
    VALUES (NEW.rowid, NEW.jellyfin_id, NEW.name, NEW.artist_name);
END;

CREATE TRIGGER IF NOT EXISTS tracks_ad AFTER DELETE ON cached_tracks BEGIN
    INSERT INTO fts_tracks(fts_tracks, rowid, jellyfin_id, name, artist_name)
    VALUES ('delete', OLD.rowid, OLD.jellyfin_id, OLD.name, OLD.artist_name);
END;

CREATE TRIGGER IF NOT EXISTS tracks_au AFTER UPDATE ON cached_tracks BEGIN
    INSERT INTO fts_tracks(fts_tracks, rowid, jellyfin_id, name, artist_name)
    VALUES ('delete', OLD.rowid, OLD.jellyfin_id, OLD.name, OLD.artist_name);
    INSERT INTO fts_tracks(rowid, jellyfin_id, name, artist_name)
    VALUES (NEW.rowid, NEW.jellyfin_id, NEW.name, NEW.artist_name);
END;

CREATE TRIGGER IF NOT EXISTS albums_ai AFTER INSERT ON cached_albums BEGIN
    INSERT INTO fts_albums(rowid, jellyfin_id, name, artist_name)
    VALUES (NEW.rowid, NEW.jellyfin_id, NEW.name, NEW.artist_name);
END;

CREATE TRIGGER IF NOT EXISTS albums_ad AFTER DELETE ON cached_albums BEGIN
    INSERT INTO fts_albums(fts_albums, rowid, jellyfin_id, name, artist_name)
    VALUES ('delete', OLD.rowid, OLD.jellyfin_id, OLD.name, OLD.artist_name);
END;

CREATE TRIGGER IF NOT EXISTS albums_au AFTER UPDATE ON cached_albums BEGIN
    INSERT INTO fts_albums(fts_albums, rowid, jellyfin_id, name, artist_name)
    VALUES ('delete', OLD.rowid, OLD.jellyfin_id, OLD.name, OLD.artist_name);
    INSERT INTO fts_albums(rowid, jellyfin_id, name, artist_name)
    VALUES (NEW.rowid, NEW.jellyfin_id, NEW.name, NEW.artist_name);
END;

CREATE TRIGGER IF NOT EXISTS artists_ai AFTER INSERT ON cached_artists BEGIN
    INSERT INTO fts_artists(rowid, jellyfin_id, name)
    VALUES (NEW.rowid, NEW.jellyfin_id, NEW.name);
END;

CREATE TRIGGER IF NOT EXISTS artists_ad AFTER DELETE ON cached_artists BEGIN
    INSERT INTO fts_artists(fts_artists, rowid, jellyfin_id, name)
    VALUES ('delete', OLD.rowid, OLD.jellyfin_id, OLD.name);
END;

CREATE TRIGGER IF NOT EXISTS artists_au AFTER UPDATE ON cached_artists BEGIN
    INSERT INTO fts_artists(fts_artists, rowid, jellyfin_id, name)
    VALUES ('delete', OLD.rowid, OLD.jellyfin_id, OLD.name);
    INSERT INTO fts_artists(rowid, jellyfin_id, name)
    VALUES (NEW.rowid, NEW.jellyfin_id, NEW.name);
END;
"#;

/// Initialise the database schema.
///
/// Creates tables if they don't exist and runs any pending migrations.
pub fn initialise_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    // Get current schema version
    let current_version = get_schema_version(conn)?;

    tracing::info!(
        "Database schema version: {} (target: {})",
        current_version,
        SCHEMA_VERSION
    );

    if current_version == 0 {
        // Fresh database - create schema
        tracing::info!("Creating initial database schema...");
        conn.execute_batch(SCHEMA_V1)?;
        set_schema_version(conn, SCHEMA_VERSION)?;
        tracing::info!("Database schema created successfully");
    } else if current_version < SCHEMA_VERSION {
        // Run migrations
        run_migrations(conn, current_version)?;
    }

    Ok(())
}

/// Get the current schema version from the database.
fn get_schema_version(conn: &Connection) -> Result<i32, rusqlite::Error> {
    // Use PRAGMA user_version to store schema version
    let version: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    Ok(version)
}

/// Set the schema version in the database.
fn set_schema_version(conn: &Connection, version: i32) -> Result<(), rusqlite::Error> {
    conn.pragma_update(None, "user_version", version)?;
    Ok(())
}

/// Each migration, with the schema version it produces.
type Migration = fn(&Connection) -> Result<(), rusqlite::Error>;

const MIGRATIONS: &[(i32, Migration)] = &[
    (2, migrate_v1_to_v2),
    (3, migrate_v2_to_v3),
    (4, migrate_v3_to_v4),
    (5, migrate_v4_to_v5),
    (6, migrate_v5_to_v6),
    (7, migrate_v6_to_v7),
    (8, migrate_v7_to_v8),
    (9, migrate_v8_to_v9),
    (10, migrate_v9_to_v10),
    (11, migrate_v10_to_v11),
    (12, migrate_v11_to_v12),
    (13, migrate_v12_to_v13),
    (14, migrate_v13_to_v14),
    (15, migrate_v14_to_v15),
    (16, migrate_v15_to_v16),
    (17, migrate_v16_to_v17),
];

/// Add a column unless the table already has it, so a re-run is a no-op.
fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), rusqlite::Error> {
    let has_col: i32 = conn.query_row(
        &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = '{column}'"),
        [],
        |row| row.get(0),
    )?;
    if has_col == 0 {
        conn.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {definition};"
        ))?;
    }
    Ok(())
}

/// Run migrations from the current version to the target version.
fn run_migrations(conn: &Connection, from_version: i32) -> Result<(), rusqlite::Error> {
    tracing::info!(
        "Running migrations from version {} to {}",
        from_version,
        SCHEMA_VERSION
    );

    for (version, migrate) in MIGRATIONS {
        if from_version < *version {
            migrate(conn)?;
        }
    }

    set_schema_version(conn, SCHEMA_VERSION)?;
    tracing::info!("Migrations complete");

    Ok(())
}

/// Migration v1 -> v2: Add backdrop_url column to cached_artists.
fn migrate_v1_to_v2(conn: &Connection) -> Result<(), rusqlite::Error> {
    tracing::info!("Migrating schema v1 -> v2: adding backdrop_url to cached_artists");
    conn.execute_batch("ALTER TABLE cached_artists ADD COLUMN backdrop_url TEXT;")
}

/// Migration v2 -> v3: Add normalization_gain_db column to cached_tracks.
fn migrate_v2_to_v3(conn: &Connection) -> Result<(), rusqlite::Error> {
    tracing::info!("Migrating schema v2 -> v3: adding normalization_gain_db to cached_tracks");
    conn.execute_batch("ALTER TABLE cached_tracks ADD COLUMN normalization_gain_db REAL;")
}

/// Migration v3 -> v4: Add MusicBrainz artist and release IDs.
fn migrate_v3_to_v4(conn: &Connection) -> Result<(), rusqlite::Error> {
    tracing::info!("Migrating schema v3 -> v4: adding artist_mbid and release_mbid columns");
    conn.execute_batch(
        r#"
        ALTER TABLE cached_artists ADD COLUMN artist_mbid TEXT;
        ALTER TABLE cached_albums ADD COLUMN release_mbid TEXT;
        ALTER TABLE cached_tracks ADD COLUMN artist_mbid TEXT;
        ALTER TABLE cached_tracks ADD COLUMN release_mbid TEXT;
        "#,
    )
}

/// Migration v4 -> v5: Add covering compound indexes for library filtering.
fn migrate_v4_to_v5(conn: &Connection) -> Result<(), rusqlite::Error> {
    tracing::info!("Migrating schema v4 -> v5: adding compound indexes for library filtering");
    conn.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS idx_tracks_genre_artist_album
            ON cached_tracks(genre, artist_name, album_jellyfin_id);
        CREATE INDEX IF NOT EXISTS idx_tracks_artist_album
            ON cached_tracks(artist_name, album_jellyfin_id);
        "#,
    )
}

/// Migration v5 -> v6: Add track_stats table for the scrobble server's listening stats.
fn migrate_v5_to_v6(conn: &Connection) -> Result<(), rusqlite::Error> {
    tracing::info!("Migrating schema v5 -> v6: adding track_stats table");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS track_stats (
            track_mbid TEXT PRIMARY KEY,
            play_count INTEGER NOT NULL DEFAULT 0,
            skip_count INTEGER NOT NULL DEFAULT 0,
            last_played_at INTEGER,
            last_skipped_at INTEGER,
            fetched_at INTEGER NOT NULL
        );",
    )
}

/// Migration v6 -> v7: Add indexes for album sorting and downloaded-track counts.
fn migrate_v6_to_v7(conn: &Connection) -> Result<(), rusqlite::Error> {
    tracing::info!("Migrating schema v6 -> v7: adding album sort and download-count indexes");
    conn.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS idx_albums_sort
            ON cached_albums(artist_name, year, name);
        CREATE INDEX IF NOT EXISTS idx_artists_name
            ON cached_artists(name);
        CREATE INDEX IF NOT EXISTS idx_tracks_album_cached
            ON cached_tracks(album_jellyfin_id, cached_file_path) WHERE cached_file_path IS NOT NULL;
        "#,
    )
}

/// Migration v7 -> v8: Add the Jellyfin playlist detail cache (offline-first).
fn migrate_v7_to_v8(conn: &Connection) -> Result<(), rusqlite::Error> {
    tracing::info!("Migrating schema v7 -> v8: adding cached playlist tables");
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS cached_playlists (
            playlist_id TEXT PRIMARY KEY,
            last_synced_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS cached_playlist_tracks (
            playlist_id TEXT NOT NULL,
            position INTEGER NOT NULL,
            jellyfin_id TEXT NOT NULL,
            playlist_item_id TEXT NOT NULL,
            name TEXT NOT NULL,
            artist_name TEXT NOT NULL,
            album_name TEXT NOT NULL,
            album_jellyfin_id TEXT NOT NULL,
            duration_ms INTEGER NOT NULL,
            artwork_url TEXT,
            PRIMARY KEY (playlist_id, position)
        );
        CREATE INDEX IF NOT EXISTS idx_playlist_tracks_pid
            ON cached_playlist_tracks(playlist_id);
        "#,
    )
}

/// Migration v8 -> v9: tag cached rows with their media-server backend.
///
/// Adds a `source` column so Jellyfin and OpenSubsonic library rows can coexist
/// in one cache. `ADD COLUMN ... NOT NULL DEFAULT 'jellyfin'` backfills every
/// existing row to Jellyfin; an install from before dual backends had exactly ONE backend selected
/// by its `SERVER_TYPE` setting, so if that was OpenSubsonic we re-stamp the
/// existing rows to match — otherwise the whole cached library would read as
/// empty (active source = opensubsonic, rows labelled jellyfin) until a re-sync.
fn migrate_v8_to_v9(conn: &Connection) -> Result<(), rusqlite::Error> {
    tracing::info!("Migrating schema v8 -> v9: adding source column for dual-backend cache");
    conn.execute_batch(
        r#"
        ALTER TABLE cached_tracks  ADD COLUMN source TEXT NOT NULL DEFAULT 'jellyfin';
        ALTER TABLE cached_albums  ADD COLUMN source TEXT NOT NULL DEFAULT 'jellyfin';
        ALTER TABLE cached_artists ADD COLUMN source TEXT NOT NULL DEFAULT 'jellyfin';
        CREATE INDEX IF NOT EXISTS idx_tracks_source  ON cached_tracks(source);
        CREATE INDEX IF NOT EXISTS idx_albums_source  ON cached_albums(source);
        CREATE INDEX IF NOT EXISTS idx_artists_source ON cached_artists(source);
        "#,
    )?;

    // `.ok()` tolerates a missing settings table/row (returns None) — real
    // upgrades always have it; some minimal migration tests don't.
    let server_type: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [crate::db::settings::keys::SERVER_TYPE],
            |row| row.get(0),
        )
        .ok();

    if server_type.as_deref() == Some("opensubsonic") {
        tracing::info!("Migration v9: backfilling existing rows to opensubsonic (last backend)");
        conn.execute_batch(
            r#"
            UPDATE cached_tracks  SET source = 'opensubsonic';
            UPDATE cached_albums  SET source = 'opensubsonic';
            UPDATE cached_artists SET source = 'opensubsonic';
            "#,
        )?;
    }

    Ok(())
}

/// Migration v9 -> v10: scrub legacy token-bearing OpenSubsonic cover URLs.
///
/// Phase 1 stored the direct authenticated `getCoverArt` URL (with the salted
/// `t=`/`s=`/`u=` auth token) in `cached_*.artwork_url`. Phase 2 replaces that
/// with a credential-free `subsonic-cover:{id}` sentinel resolved to a `/cover`
/// proxy URL at read time. NULL the old values so no reusable credential lingers
/// in SQLite; the next sync/read repopulates the credential-free sentinel (only
/// the brief offline artwork fallback is lost until then). Jellyfin rows are
/// untouched — their artwork URLs were never authenticated.
fn migrate_v9_to_v10(conn: &Connection) -> Result<(), rusqlite::Error> {
    tracing::info!("Migrating schema v9 -> v10: scrubbing token-bearing OpenSubsonic cover URLs");
    conn.execute_batch(
        r#"
        UPDATE cached_tracks  SET artwork_url = NULL WHERE source = 'opensubsonic';
        UPDATE cached_albums  SET artwork_url = NULL WHERE source = 'opensubsonic';
        UPDATE cached_artists SET artwork_url = NULL WHERE source = 'opensubsonic';
        "#,
    )
}

/// Migration v10 -> v11: add server-side play count to cached_tracks.
///
/// Surfaces the play count the media server already reports (Jellyfin
/// `UserData.PlayCount`, OpenSubsonic inline `playCount`) so the visible "Plays"
/// column works for a backend-only user with no scrobble service configured.
/// Additive and source-scoped (via the existing `source` column) — no key
/// change and no cross-source blending. Existing rows get NULL until the next
/// sync repopulates them.
fn migrate_v10_to_v11(conn: &Connection) -> Result<(), rusqlite::Error> {
    tracing::info!("Migrating schema v10 -> v11: adding server_play_count to cached_tracks");
    add_column_if_missing(conn, "cached_tracks", "server_play_count", "INTEGER")?;
    Ok(())
}

/// Migration v11 -> v12: source-scope the playlist cache.
///
/// Phase-1 playlists were Jellyfin-only, so `cached_playlists.playlist_id` was
/// the whole PK. OpenSubsonic playlists now share these tables, and a
/// dual-connected user could have a Jellyfin and a Subsonic playlist with the
/// same id — which would collide on the old PK and cross-contaminate the cache.
/// Recreate both tables with a `source`-prefixed composite PK, backfilling every
/// existing row to `jellyfin` (every playlist cached before OpenSubsonic playlists is Jellyfin's).
///
/// Guarded on the presence of the `source` column so re-running is a no-op.
fn migrate_v11_to_v12(conn: &Connection) -> Result<(), rusqlite::Error> {
    let has_col: i32 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('cached_playlists') WHERE name = 'source'",
        [],
        |row| row.get(0),
    )?;
    if has_col > 0 {
        return Ok(());
    }
    tracing::info!("Migrating schema v11 -> v12: source-scoping the playlist cache");
    conn.execute_batch(
        r#"
        CREATE TABLE cached_playlists_new (
            playlist_id TEXT NOT NULL,
            source TEXT NOT NULL DEFAULT 'jellyfin',
            last_synced_at INTEGER NOT NULL,
            PRIMARY KEY (source, playlist_id)
        );
        INSERT INTO cached_playlists_new (playlist_id, source, last_synced_at)
            SELECT playlist_id, 'jellyfin', last_synced_at FROM cached_playlists;
        DROP TABLE cached_playlists;
        ALTER TABLE cached_playlists_new RENAME TO cached_playlists;

        CREATE TABLE cached_playlist_tracks_new (
            playlist_id TEXT NOT NULL,
            source TEXT NOT NULL DEFAULT 'jellyfin',
            position INTEGER NOT NULL,
            jellyfin_id TEXT NOT NULL,
            playlist_item_id TEXT NOT NULL,
            name TEXT NOT NULL,
            artist_name TEXT NOT NULL,
            album_name TEXT NOT NULL,
            album_jellyfin_id TEXT NOT NULL,
            duration_ms INTEGER NOT NULL,
            artwork_url TEXT,
            PRIMARY KEY (source, playlist_id, position)
        );
        INSERT INTO cached_playlist_tracks_new (
            playlist_id, source, position, jellyfin_id, playlist_item_id, name,
            artist_name, album_name, album_jellyfin_id, duration_ms, artwork_url
        )
            SELECT playlist_id, 'jellyfin', position, jellyfin_id, playlist_item_id, name,
                   artist_name, album_name, album_jellyfin_id, duration_ms, artwork_url
            FROM cached_playlist_tracks;
        DROP TABLE cached_playlist_tracks;
        ALTER TABLE cached_playlist_tracks_new RENAME TO cached_playlist_tracks;

        CREATE INDEX IF NOT EXISTS idx_playlist_tracks_pid ON cached_playlist_tracks(playlist_id);
        CREATE INDEX IF NOT EXISTS idx_playlists_source ON cached_playlists(source);
        "#,
    )
}

/// Migration v12 -> v13: add held_no_mbid to sync_queue.
///
/// The scrobble server rejects a scrobble/skip with no MusicBrainz recording
/// ID instead of silently storing it. Bragi partitions mbid-less queued
/// scrobbles out before ever sending them and marks them `held_no_mbid`
/// instead of `synced`, so they survive to be re-resolved against live
/// Jellyfin metadata on a later drain rather than being falsely marked synced
/// or silently churning through backoff to eventual deletion.
///
/// Guarded on column presence (mirrors `migrate_v10_to_v11`) so re-running is
/// a no-op instead of erroring on "duplicate column name".
fn migrate_v12_to_v13(conn: &Connection) -> Result<(), rusqlite::Error> {
    tracing::info!("Migrating schema v12 -> v13: adding held_no_mbid to sync_queue");
    add_column_if_missing(conn, "sync_queue", "held_no_mbid", "INTEGER")?;
    Ok(())
}

/// Migration v13 -> v14: shared/imported playlists.
///
/// Adds `mbid` to `cached_playlist_tracks` (needed to export a playlist as a
/// shareable, MusicBrainz-keyed code) and the `imported_playlists` /
/// `imported_playlist_tracks` tables (a local-only, read-only snapshot of a
/// playlist imported from someone else's Bragi instance).
///
/// Guarded on column presence (mirrors `migrate_v10_to_v11`) so re-running is
/// a no-op instead of erroring on "duplicate column name".
fn migrate_v13_to_v14(conn: &Connection) -> Result<(), rusqlite::Error> {
    tracing::info!("Migrating schema v13 -> v14: adding shared/imported playlist support");
    add_column_if_missing(conn, "cached_playlist_tracks", "mbid", "TEXT")?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS imported_playlists (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            imported_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS imported_playlist_tracks (
            playlist_id TEXT NOT NULL REFERENCES imported_playlists(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            mbid TEXT,
            title TEXT NOT NULL,
            artist_name TEXT NOT NULL,
            album_name TEXT,
            duration_ms INTEGER,
            PRIMARY KEY (playlist_id, position)
        );

        CREATE INDEX IF NOT EXISTS idx_imported_playlist_tracks_pid ON imported_playlist_tracks(playlist_id);
        CREATE INDEX IF NOT EXISTS idx_imported_playlist_tracks_mbid ON imported_playlist_tracks(mbid);
        "#,
    )?;
    Ok(())
}

/// Migration v14 -> v15: add `date_created` to cached_albums.
///
/// Existing rows keep `NULL` until the next full sync backfills them from the
/// server; `get_recently_added_albums` coalesces to `last_synced_at` so the view
/// degrades to the old behaviour meanwhile rather than emptying out.
fn migrate_v14_to_v15(conn: &Connection) -> Result<(), rusqlite::Error> {
    tracing::info!("Migrating schema v14 -> v15: adding date_created to cached_albums");
    add_column_if_missing(conn, "cached_albums", "date_created", "INTEGER")?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_albums_date_created
             ON cached_albums(source, date_created DESC);",
    )?;
    Ok(())
}

/// Migration v15 -> v16: normalised genre memberships.
///
/// Creates `track_genres` and back-derives every membership from the raw
/// `cached_tracks.genre` strings already cached, so an existing library gets
/// correct genres without a forced re-sync. `cached_tracks.genre` is kept as
/// the audit trail rather than dropped.
fn migrate_v15_to_v16(conn: &Connection) -> Result<(), rusqlite::Error> {
    tracing::info!("Migrating schema v15 -> v16: adding track_genres");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS track_genres (
             track_id TEXT NOT NULL REFERENCES cached_tracks(jellyfin_id) ON DELETE CASCADE,
             genre_key TEXT NOT NULL,
             genre_display TEXT NOT NULL,
             source TEXT NOT NULL DEFAULT 'tag',
             PRIMARY KEY (track_id, genre_key)
         );
         CREATE INDEX IF NOT EXISTS idx_track_genres_key ON track_genres(genre_key);",
    )?;

    let rows: Vec<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT jellyfin_id, genre FROM cached_tracks
             WHERE genre IS NOT NULL AND genre != ''",
        )?;
        let mapped = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        mapped.collect::<Result<Vec<_>, _>>()?
    };

    let mut memberships = 0usize;
    {
        // ONE transaction around the whole back-derivation. In autocommit every
        // insert is its own fsynced WAL commit, and a real library produces tens
        // of thousands of memberships — the app opens its database during Tauri
        // setup, so that is minutes of no window on the first launch after the
        // upgrade. Correctness does not need it (the DDL is guarded, the insert
        // is ON CONFLICT DO NOTHING, and set_schema_version only lands after
        // every step, so an interrupted run safely re-runs); the wait does.
        let tx = conn.unchecked_transaction()?;
        {
            // ON CONFLICT DO NOTHING, not a plain INSERT: re-running this against
            // a partially populated table (a re-run, or a live sync that got there
            // first) must not duplicate or fail.
            let mut insert = tx.prepare(
                "INSERT INTO track_genres (track_id, genre_key, genre_display, source)
                 VALUES (?1, ?2, ?3, 'derived')
                 ON CONFLICT(track_id, genre_key) DO NOTHING",
            )?;
            for (track_id, raw) in &rows {
                for genre in crate::genres::split_genres(std::slice::from_ref(raw)) {
                    insert.execute(rusqlite::params![track_id, genre.key, genre.display])?;
                    memberships += 1;
                }
            }
        }
        tx.commit()?;
    }

    // The genre column is no longer read by anything — every listing and filter
    // goes through track_genres — but this index was still being maintained on
    // every one of ~23k upserts per sync.
    conn.execute_batch("DROP INDEX IF EXISTS idx_tracks_genre_artist_album;")?;

    tracing::info!(
        "Back-derived {} genre memberships from {} cached tracks",
        memberships,
        rows.len()
    );
    Ok(())
}

/// Migration v16 -> v17: add sort_name to cached_artists.
fn migrate_v16_to_v17(conn: &Connection) -> Result<(), rusqlite::Error> {
    tracing::info!("Migrating schema v16 -> v17: adding sort_name to cached_artists");
    conn.execute_batch("ALTER TABLE cached_artists ADD COLUMN sort_name TEXT;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_creation() {
        let conn = Connection::open_in_memory().unwrap();
        initialise_schema(&conn).unwrap();

        // Verify tables exist
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='cached_albums'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Verify schema version
        let version = get_schema_version(&conn).unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_idempotent_schema_creation() {
        let conn = Connection::open_in_memory().unwrap();

        // Run twice - should not error
        initialise_schema(&conn).unwrap();
        initialise_schema(&conn).unwrap();

        let version = get_schema_version(&conn).unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn migrate_v7_to_v8_adds_playlist_tables_idempotently() {
        let conn = Connection::open_in_memory().unwrap();
        // IF NOT EXISTS DDL must be safe to apply more than once.
        migrate_v7_to_v8(&conn).unwrap();
        migrate_v7_to_v8(&conn).unwrap();

        for table in ["cached_playlists", "cached_playlist_tracks"] {
            let count: i32 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "{table} must exist after the v7->v8 migration");
        }
    }

    #[test]
    fn migrate_v8_to_v9_adds_source_and_backfills_jellyfin() {
        let conn = Connection::open_in_memory().unwrap();
        // Minimal pre-v9 tables (no source column) with one existing Jellyfin row.
        conn.execute_batch(
            r#"
            CREATE TABLE cached_tracks  (jellyfin_id TEXT PRIMARY KEY, last_synced_at INTEGER NOT NULL);
            CREATE TABLE cached_albums  (jellyfin_id TEXT PRIMARY KEY, last_synced_at INTEGER NOT NULL);
            CREATE TABLE cached_artists (jellyfin_id TEXT PRIMARY KEY, last_synced_at INTEGER NOT NULL);
            INSERT INTO cached_tracks (jellyfin_id, last_synced_at) VALUES ('old-track', 1000);
            "#,
        )
        .unwrap();

        migrate_v8_to_v9(&conn).unwrap();

        // Existing rows must backfill to 'jellyfin' (all pre-#107 data is Jellyfin).
        let source: String = conn
            .query_row(
                "SELECT source FROM cached_tracks WHERE jellyfin_id = 'old-track'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            source, "jellyfin",
            "existing rows must backfill to jellyfin"
        );

        // The column must exist on all three cached tables.
        for table in ["cached_tracks", "cached_albums", "cached_artists"] {
            let has_source: i32 = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = 'source'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                has_source, 1,
                "{table} must have a source column after v8->v9"
            );
        }
    }

    #[test]
    fn migrate_v8_to_v9_backfills_opensubsonic_when_that_was_the_backend() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            INSERT INTO settings (key, value) VALUES ('server_type', 'opensubsonic');
            CREATE TABLE cached_tracks  (jellyfin_id TEXT PRIMARY KEY, last_synced_at INTEGER NOT NULL);
            CREATE TABLE cached_albums  (jellyfin_id TEXT PRIMARY KEY, last_synced_at INTEGER NOT NULL);
            CREATE TABLE cached_artists (jellyfin_id TEXT PRIMARY KEY, last_synced_at INTEGER NOT NULL);
            INSERT INTO cached_tracks (jellyfin_id, last_synced_at) VALUES ('ss-track', 1000);
            "#,
        )
        .unwrap();

        migrate_v8_to_v9(&conn).unwrap();

        // An OpenSubsonic-only install from before dual backends must re-stamp its rows to
        // opensubsonic, not leave them at the 'jellyfin' column default.
        let source: String = conn
            .query_row(
                "SELECT source FROM cached_tracks WHERE jellyfin_id = 'ss-track'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(source, "opensubsonic");
    }

    #[test]
    fn migrate_v9_to_v10_scrubs_opensubsonic_cover_urls_only() {
        let conn = Connection::open_in_memory().unwrap();
        // Minimal post-v9 tables (source column present) with one row per backend.
        conn.execute_batch(
            r#"
            CREATE TABLE cached_tracks  (jellyfin_id TEXT PRIMARY KEY, artwork_url TEXT, source TEXT NOT NULL);
            CREATE TABLE cached_albums  (jellyfin_id TEXT PRIMARY KEY, artwork_url TEXT, source TEXT NOT NULL);
            CREATE TABLE cached_artists (jellyfin_id TEXT PRIMARY KEY, artwork_url TEXT, source TEXT NOT NULL);
            INSERT INTO cached_albums (jellyfin_id, artwork_url, source)
                VALUES ('ss-al', 'https://nav/rest/getCoverArt.view?id=al&t=deadbeef&s=abc&u=demo', 'opensubsonic');
            INSERT INTO cached_albums (jellyfin_id, artwork_url, source)
                VALUES ('jf-al', 'https://jf/Items/jf-al/Images/Primary?maxWidth=300', 'jellyfin');
            INSERT INTO cached_tracks (jellyfin_id, artwork_url, source)
                VALUES ('ss-tr', 'https://nav/rest/getCoverArt.view?id=tr&t=deadbeef', 'opensubsonic');
            INSERT INTO cached_artists (jellyfin_id, artwork_url, source)
                VALUES ('ss-ar', 'https://nav/rest/getCoverArt.view?id=ar&t=deadbeef', 'opensubsonic');
            "#,
        )
        .unwrap();

        migrate_v9_to_v10(&conn).unwrap();

        // OpenSubsonic cover URLs (token-bearing) are scrubbed to NULL.
        for (table, id) in [
            ("cached_albums", "ss-al"),
            ("cached_tracks", "ss-tr"),
            ("cached_artists", "ss-ar"),
        ] {
            let art: Option<String> = conn
                .query_row(
                    &format!("SELECT artwork_url FROM {table} WHERE jellyfin_id = ?1"),
                    [id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(art, None, "{table}/{id} cover URL must be scrubbed");
        }

        // Jellyfin rows are left intact (their artwork was never authenticated).
        let jf: Option<String> = conn
            .query_row(
                "SELECT artwork_url FROM cached_albums WHERE jellyfin_id = 'jf-al'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            jf.as_deref(),
            Some("https://jf/Items/jf-al/Images/Primary?maxWidth=300"),
            "Jellyfin artwork must be preserved"
        );

        // Idempotent: re-running changes nothing further.
        migrate_v9_to_v10(&conn).unwrap();
        let jf_again: Option<String> = conn
            .query_row(
                "SELECT artwork_url FROM cached_albums WHERE jellyfin_id = 'jf-al'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            jf_again.as_deref(),
            Some("https://jf/Items/jf-al/Images/Primary?maxWidth=300")
        );
    }

    #[test]
    fn migrate_v10_to_v11_adds_server_play_count_idempotently() {
        let conn = Connection::open_in_memory().unwrap();
        // Minimal pre-v11 cached_tracks (no server_play_count column).
        conn.execute_batch(
            "CREATE TABLE cached_tracks (jellyfin_id TEXT PRIMARY KEY, last_synced_at INTEGER NOT NULL);",
        )
        .unwrap();

        // Applying twice must not error (the guard makes the ADD COLUMN a no-op).
        migrate_v10_to_v11(&conn).unwrap();
        migrate_v10_to_v11(&conn).unwrap();

        let has_col: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('cached_tracks') WHERE name = 'server_play_count'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            has_col, 1,
            "cached_tracks must have server_play_count after v10->v11"
        );
    }

    #[test]
    fn fresh_schema_has_server_play_count_column() {
        let conn = Connection::open_in_memory().unwrap();
        initialise_schema(&conn).unwrap();
        let has_col: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('cached_tracks') WHERE name = 'server_play_count'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            has_col, 1,
            "fresh cached_tracks must have server_play_count"
        );
    }

    #[test]
    fn migrate_v11_to_v12_source_scopes_playlist_cache() {
        let conn = Connection::open_in_memory().unwrap();
        // Minimal pre-v12 playlist tables (no source column) with one row each.
        conn.execute_batch(
            r#"
            CREATE TABLE cached_playlists (
                playlist_id TEXT PRIMARY KEY,
                last_synced_at INTEGER NOT NULL
            );
            CREATE TABLE cached_playlist_tracks (
                playlist_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                jellyfin_id TEXT NOT NULL,
                playlist_item_id TEXT NOT NULL,
                name TEXT NOT NULL,
                artist_name TEXT NOT NULL,
                album_name TEXT NOT NULL,
                album_jellyfin_id TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                artwork_url TEXT,
                PRIMARY KEY (playlist_id, position)
            );
            INSERT INTO cached_playlists (playlist_id, last_synced_at) VALUES ('pl-1', 1000);
            INSERT INTO cached_playlist_tracks
                (playlist_id, position, jellyfin_id, playlist_item_id, name, artist_name, album_name, album_jellyfin_id, duration_ms, artwork_url)
                VALUES ('pl-1', 0, 'tr-1', 'item-1', 'A', 'Artist', 'Album', 'al-1', 200000, NULL);
            "#,
        )
        .unwrap();

        // Applying twice must be safe (the source-column guard makes it a no-op).
        migrate_v11_to_v12(&conn).unwrap();
        migrate_v11_to_v12(&conn).unwrap();

        // Both tables gain the source column, existing rows backfill to jellyfin.
        for table in ["cached_playlists", "cached_playlist_tracks"] {
            let has_source: i32 = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = 'source'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(has_source, 1, "{table} must have a source column after v12");
        }
        let src: String = conn
            .query_row(
                "SELECT source FROM cached_playlists WHERE playlist_id = 'pl-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            src, "jellyfin",
            "existing playlist rows backfill to jellyfin"
        );

        // The new composite PK lets a same-id playlist coexist per source.
        conn.execute(
            "INSERT INTO cached_playlists (playlist_id, source, last_synced_at) VALUES ('pl-1', 'opensubsonic', 2000)",
            [],
        )
        .unwrap();
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM cached_playlists WHERE playlist_id = 'pl-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2, "same id must coexist across two sources");
    }

    #[test]
    fn fresh_schema_has_source_column() {
        let conn = Connection::open_in_memory().unwrap();
        initialise_schema(&conn).unwrap();
        for table in ["cached_tracks", "cached_albums", "cached_artists"] {
            let has_source: i32 = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = 'source'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(has_source, 1, "fresh {table} must have a source column");
        }
    }

    #[test]
    fn migrate_v12_to_v13_adds_held_no_mbid_idempotently() {
        let conn = Connection::open_in_memory().unwrap();
        // Minimal pre-v13 sync_queue (no held_no_mbid column).
        conn.execute_batch(
            "CREATE TABLE sync_queue (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                operation TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                attempts INTEGER DEFAULT 0,
                last_attempt_at INTEGER,
                synced_at INTEGER
            );",
        )
        .unwrap();

        // Applying twice must not error (the guard makes the ADD COLUMN a no-op).
        migrate_v12_to_v13(&conn).unwrap();
        migrate_v12_to_v13(&conn).unwrap();

        let has_col: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('sync_queue') WHERE name = 'held_no_mbid'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            has_col, 1,
            "sync_queue must have held_no_mbid after v12->v13"
        );
    }

    #[test]
    fn fresh_schema_has_held_no_mbid_column() {
        let conn = Connection::open_in_memory().unwrap();
        initialise_schema(&conn).unwrap();
        let has_col: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('sync_queue') WHERE name = 'held_no_mbid'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(has_col, 1, "fresh sync_queue must have held_no_mbid");
    }

    #[test]
    fn migrate_v14_to_v15_adds_date_created_idempotently() {
        let conn = Connection::open_in_memory().unwrap();
        // Minimal pre-v15 cached_albums: no date_created column.
        conn.execute_batch(
            "CREATE TABLE cached_albums (
                jellyfin_id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                last_synced_at INTEGER NOT NULL,
                source TEXT NOT NULL DEFAULT 'jellyfin'
            );",
        )
        .unwrap();

        // Applying twice must not error (guarded ADD COLUMN + CREATE INDEX IF NOT EXISTS).
        migrate_v14_to_v15(&conn).unwrap();
        migrate_v14_to_v15(&conn).unwrap();

        let has_col: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('cached_albums') WHERE name = 'date_created'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            has_col, 1,
            "cached_albums must have date_created after v14->v15"
        );

        let has_index: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_albums_date_created'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            has_index, 1,
            "the recently-added index must exist after v14->v15"
        );
    }

    /// A fresh install must land on the same shape a migrated database reaches.
    #[test]
    fn fresh_schema_has_date_created_column_and_index() {
        let conn = Connection::open_in_memory().unwrap();
        initialise_schema(&conn).unwrap();

        let has_col: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('cached_albums') WHERE name = 'date_created'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(has_col, 1, "SCHEMA_V1 must include date_created");

        let has_index: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_albums_date_created'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            has_index, 1,
            "SCHEMA_V1 must include idx_albums_date_created"
        );
    }

    #[test]
    fn migrate_v13_to_v14_adds_playlist_mbid_and_imported_tables_idempotently() {
        let conn = Connection::open_in_memory().unwrap();
        // Minimal pre-v14 cached_playlist_tracks (no mbid column, no imported_* tables).
        conn.execute_batch(
            "CREATE TABLE cached_playlist_tracks (
                playlist_id TEXT NOT NULL,
                source TEXT NOT NULL DEFAULT 'jellyfin',
                position INTEGER NOT NULL,
                jellyfin_id TEXT NOT NULL,
                playlist_item_id TEXT NOT NULL,
                name TEXT NOT NULL,
                artist_name TEXT NOT NULL,
                album_name TEXT NOT NULL,
                album_jellyfin_id TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                artwork_url TEXT,
                PRIMARY KEY (source, playlist_id, position)
            );",
        )
        .unwrap();

        // Applying twice must not error (guarded ADD COLUMN + CREATE TABLE IF NOT EXISTS).
        migrate_v13_to_v14(&conn).unwrap();
        migrate_v13_to_v14(&conn).unwrap();

        let has_col: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('cached_playlist_tracks') WHERE name = 'mbid'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            has_col, 1,
            "cached_playlist_tracks must have mbid after v13->v14"
        );

        let table_count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('imported_playlists', 'imported_playlist_tracks')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            table_count, 2,
            "imported playlist tables must exist after v13->v14"
        );
    }

    #[test]
    fn migrate_v15_to_v16_back_derives_memberships_without_a_resync() {
        let conn = Connection::open_in_memory().unwrap();
        // Minimal pre-v16 cached_tracks holding raw multi-value tag strings.
        conn.execute_batch(
            "CREATE TABLE cached_tracks (
                jellyfin_id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                genre TEXT,
                last_synced_at INTEGER NOT NULL,
                source TEXT NOT NULL DEFAULT 'jellyfin'
            );
            INSERT INTO cached_tracks (jellyfin_id, name, genre, last_synced_at) VALUES
                ('t1', 'Combined', 'Alternative Metal;Heavy Metal;Rock', 0),
                ('t2', 'Plain',    'Heavy Metal',                        0),
                ('t3', 'Cased',    'nu metal;ROCK',                      0),
                ('t4', 'Untagged', NULL,                                 0),
                ('t5', 'Blank',    '',                                   0),
                ('t6', 'Numeric',  '17;Rock',                            0);",
        )
        .unwrap();

        // Applying twice must not error or duplicate (guarded DDL + ON CONFLICT).
        migrate_v15_to_v16(&conn).unwrap();
        migrate_v15_to_v16(&conn).unwrap();

        let total: i32 = conn
            .query_row("SELECT COUNT(*) FROM track_genres", [], |r| r.get(0))
            .unwrap();
        // t1: 3, t2: 1, t3: 2, t6: 1 (the ID3v1 code 17 is dropped).
        assert_eq!(total, 7, "back-derivation must be idempotent");

        let t1: Vec<String> = {
            let mut stmt = conn
                .prepare(
                    "SELECT genre_key FROM track_genres WHERE track_id = 't1' ORDER BY genre_key",
                )
                .unwrap();
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows
        };
        assert_eq!(t1, vec!["alternative metal", "heavy metal", "rock"]);

        // The raw tag survives as the audit trail — nothing rewrites it.
        let raw: String = conn
            .query_row(
                "SELECT genre FROM cached_tracks WHERE jellyfin_id = 't1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raw, "Alternative Metal;Heavy Metal;Rock");

        let derived: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM track_genres WHERE source = 'derived'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(derived, 7, "migration-derived rows are marked as such");

        // The index nothing reads any more is gone (and dropping it twice is fine).
        let dead_index: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_tracks_genre_artist_album'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(dead_index, 0);
    }

    #[test]
    fn fresh_schema_has_track_genres() {
        let conn = Connection::open_in_memory().unwrap();
        initialise_schema(&conn).unwrap();

        let has_table: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='track_genres'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(has_table, 1, "fresh schema must have track_genres");

        let has_index: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_track_genres_key'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            has_index, 1,
            "fresh schema must index track_genres(genre_key)"
        );
    }

    #[test]
    fn fresh_schema_has_imported_playlist_support() {
        let conn = Connection::open_in_memory().unwrap();
        initialise_schema(&conn).unwrap();

        let has_col: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('cached_playlist_tracks') WHERE name = 'mbid'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(has_col, 1, "fresh cached_playlist_tracks must have mbid");

        let table_count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('imported_playlists', 'imported_playlist_tracks')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            table_count, 2,
            "fresh schema must have imported playlist tables"
        );
    }
}
