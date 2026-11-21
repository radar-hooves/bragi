//! Album database operations.
//!
//! CRUD operations for cached album metadata.

use crate::mediaserver::ServerKind;
use crate::models::Album;
use rusqlite::{params, Connection, Row};

use super::utils::{current_timestamp, escape_fts_query};
use super::{Database, DatabaseError};

/// Album data for database insertion.
#[derive(Debug, Clone)]
pub struct CachedAlbum {
    pub jellyfin_id: String,
    pub release_mbid: Option<String>,
    pub name: String,
    pub artist_name: Option<String>,
    pub artist_jellyfin_id: Option<String>,
    pub year: Option<u32>,
    pub genre: Option<String>,
    pub artwork_url: Option<String>,
    pub track_count: Option<u32>,
    pub duration_ms: Option<u64>,
    /// Server-reported date-added, epoch seconds. `None` when the backend
    /// doesn't report one, which leaves the row falling back to `last_synced_at`.
    pub date_created: Option<i64>,
}

impl Database {
    /// Insert or update an album in the cache.
    ///
    /// # Arguments
    ///
    /// * `album` - The album data to upsert
    /// * `timestamp` - Optional sync timestamp. If None, uses current time.
    ///   Pass a shared timestamp during sync operations to ensure
    ///   consistent stale entry detection.
    pub fn upsert_album(
        &self,
        album: &CachedAlbum,
        source: ServerKind,
        timestamp: Option<i64>,
    ) -> Result<(), DatabaseError> {
        self.upsert_albums(std::slice::from_ref(album), source, timestamp)
    }

    /// Insert or update multiple albums in a transaction.
    ///
    /// # Arguments
    ///
    /// * `albums` - The albums to upsert
    /// * `timestamp` - Optional sync timestamp. If None, uses current time.
    ///   Pass a shared timestamp during sync operations to ensure
    ///   consistent stale entry detection.
    pub fn upsert_albums(
        &self,
        albums: &[CachedAlbum],
        source: ServerKind,
        timestamp: Option<i64>,
    ) -> Result<(), DatabaseError> {
        let now = timestamp.unwrap_or_else(current_timestamp);
        let source = source.as_str();

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            {
                let mut stmt = tx.prepare(
                    r#"
                    INSERT INTO cached_albums (
                        jellyfin_id, release_mbid, name, artist_name, artist_jellyfin_id,
                        year, genre, artwork_url, track_count, duration_ms, last_synced_at, source,
                        date_created
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                    ON CONFLICT(jellyfin_id) DO UPDATE SET
                        release_mbid = excluded.release_mbid,
                        name = excluded.name,
                        artist_name = excluded.artist_name,
                        artist_jellyfin_id = excluded.artist_jellyfin_id,
                        year = excluded.year,
                        genre = excluded.genre,
                        artwork_url = excluded.artwork_url,
                        track_count = excluded.track_count,
                        duration_ms = excluded.duration_ms,
                        last_synced_at = excluded.last_synced_at,
                        source = excluded.source,
                        -- Never regress a known date-added to NULL: a backend
                        -- that stops reporting it (or a partial sync) must not
                        -- wipe it.
                        date_created = COALESCE(excluded.date_created, cached_albums.date_created)
                    "#,
                )?;

                for album in albums {
                    stmt.execute(params![
                        album.jellyfin_id,
                        album.release_mbid,
                        album.name,
                        album.artist_name,
                        album.artist_jellyfin_id,
                        album.year,
                        album.genre,
                        album.artwork_url,
                        album.track_count,
                        album.duration_ms.map(|d| d as i64),
                        now,
                        source,
                        album.date_created,
                    ])?;
                }
            }

            tx.commit()?;
            Ok(())
        })
    }

    /// Get all cached albums for the active backend.
    ///
    /// Grouped by artist using the same key [`Database::get_all_artists`]
    /// sorts by (Jellyfin's own sort name where the artist row has one,
    /// falling back to article-stripping), so an artist orders the same way
    /// here as on the Artists page; within an artist, newest release first,
    /// matching [`Database::get_albums_by_artist`].
    pub fn get_all_albums(&self) -> Result<Vec<Album>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT a.jellyfin_id, a.release_mbid, a.name, a.artist_name,
                       a.artist_jellyfin_id, a.year, a.genre, a.artwork_url,
                       a.track_count, a.duration_ms, a.date_created,
                       COUNT(t.jellyfin_id) AS downloaded_track_count
                FROM cached_albums a
                -- The unary + keeps the planner off idx_tracks_source (two
                -- distinct values, useless here) and on idx_tracks_album_cached.
                -- PRAGMA optimize in Database::open already fixes the plan; this
                -- makes it hold even on a DB whose stats are missing or stale.
                LEFT JOIN cached_tracks t ON t.album_jellyfin_id = a.jellyfin_id
                    AND +t.source = a.source
                    AND t.cached_file_path IS NOT NULL
                LEFT JOIN cached_artists ar ON ar.jellyfin_id = a.artist_jellyfin_id
                WHERE a.source = ?1
                GROUP BY a.jellyfin_id
                ORDER BY COALESCE(NULLIF(ar.sort_name, ''), a.artist_name) COLLATE BRAGI_ARTIST,
                         a.year DESC, a.name COLLATE BRAGI_FOLD
                "#,
            )?;

            let albums = stmt
                .query_map([source], album_from_row)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(albums)
        })
    }

    /// Get albums by artist Jellyfin ID (scoped to the active backend).
    ///
    /// Newest-first by release year, as an artist's albums list reads in Apple
    /// Music and Spotify, falling back to the album title for an equal or
    /// missing year.
    pub fn get_albums_by_artist(
        &self,
        artist_jellyfin_id: &str,
    ) -> Result<Vec<Album>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT a.jellyfin_id, a.release_mbid, a.name, a.artist_name,
                       a.artist_jellyfin_id, a.year, a.genre, a.artwork_url,
                       a.track_count, a.duration_ms, a.date_created,
                       COUNT(t.jellyfin_id) AS downloaded_track_count
                FROM cached_albums a
                LEFT JOIN cached_tracks t ON t.album_jellyfin_id = a.jellyfin_id
                    AND t.source = a.source
                    AND t.cached_file_path IS NOT NULL
                WHERE a.artist_jellyfin_id = ?1 AND a.source = ?2
                GROUP BY a.jellyfin_id
                ORDER BY a.year DESC, a.name COLLATE BRAGI_FOLD
                "#,
            )?;

            let albums = stmt
                .query_map(params![artist_jellyfin_id, source], album_from_row)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(albums)
        })
    }

    /// Search albums by name using FTS (scoped to the active backend).
    pub fn search_albums(&self, query: &str) -> Result<Vec<Album>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| search_albums_internal(conn, query, source))
    }

    /// Delete a backend's albums not synced since the given timestamp.
    ///
    /// Scoped to `source` (the backend being synced, not the active source) so a
    /// sync of one backend can never delete the other backend's albums.
    pub fn delete_stale_albums(
        &self,
        since_timestamp: i64,
        source: &str,
    ) -> Result<u32, DatabaseError> {
        self.with_conn(|conn| {
            let rows = conn.execute(
                "DELETE FROM cached_albums WHERE last_synced_at < ?1 AND source = ?2",
                params![since_timestamp, source],
            )?;
            Ok(rows as u32)
        })
    }

    /// Get downloaded track counts for the active backend's albums.
    ///
    /// Returns a map of album jellyfin_id → downloaded track count.
    /// Used to enrich media-server album responses with local cache state.
    pub fn get_album_download_counts(
        &self,
    ) -> Result<std::collections::HashMap<String, u32>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT t.album_jellyfin_id, COUNT(*) AS cnt
                FROM cached_tracks t
                WHERE t.cached_file_path IS NOT NULL
                  AND t.album_jellyfin_id IS NOT NULL
                  AND t.source = ?1
                GROUP BY t.album_jellyfin_id
                "#,
            )?;
            let rows = stmt.query_map([source], |row| {
                let album_id: String = row.get(0)?;
                let count: u32 = row.get(1)?;
                Ok((album_id, count))
            })?;
            let mut map = std::collections::HashMap::new();
            for row in rows {
                let (album_id, count) = row?;
                map.insert(album_id, count);
            }
            Ok(map)
        })
    }

    /// Get albums added within the specified number of days (active backend).
    ///
    /// Ordered by the server's own date-added, most recent first.
    ///
    /// `last_synced_at` cannot answer this question: every sync rewrites it
    /// across the whole library with one shared timestamp, so filtering on it
    /// returned either the entire library or nothing at all, ordered by name.
    /// Rows cached before the v15 schema have no `date_created` until
    /// the next full sync backfills them, so fall back to `last_synced_at` for
    /// those — the old behaviour, but only where there's nothing better.
    pub fn get_recently_added_albums(&self, days: u32) -> Result<Vec<Album>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| {
            // Calculate the cutoff timestamp (days ago from now)
            let cutoff = current_timestamp() - (days as i64 * 24 * 60 * 60);

            let mut stmt = conn.prepare(
                r#"
                SELECT a.jellyfin_id, a.release_mbid, a.name, a.artist_name,
                       a.artist_jellyfin_id, a.year, a.genre, a.artwork_url,
                       a.track_count, a.duration_ms, a.date_created,
                       COUNT(t.jellyfin_id) AS downloaded_track_count
                FROM cached_albums a
                LEFT JOIN cached_tracks t ON t.album_jellyfin_id = a.jellyfin_id
                    AND +t.source = a.source
                    AND t.cached_file_path IS NOT NULL
                WHERE COALESCE(a.date_created, a.last_synced_at) >= ?1
                  AND a.source = ?2
                GROUP BY a.jellyfin_id
                ORDER BY COALESCE(a.date_created, a.last_synced_at) DESC, a.name COLLATE BRAGI_FOLD
                LIMIT ?3
                "#,
            )?;

            let albums = stmt
                .query_map(
                    params![cutoff, source, RECENTLY_ADDED_LIMIT],
                    album_from_row,
                )?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(albums)
        })
    }
}

/// Upper bound on a "recently added" page.
///
/// The window is a user setting (30 days by default), so on a library that has
/// just been bulk-imported every album is legitimately recent. This caps the IPC
/// payload and the rendered grid at a size a human would actually scroll, rather
/// than shipping the whole library across the bridge.
const RECENTLY_ADDED_LIMIT: i64 = 500;

/// Convert a database row to an Album model.
fn album_from_row(row: &Row<'_>) -> Result<Album, rusqlite::Error> {
    Ok(Album {
        jellyfin_id: row.get(0)?,
        release_mbid: row.get(1)?,
        name: row.get(2)?,
        artist_name: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
        artist_jellyfin_id: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        year: row.get(5)?,
        genre: row.get(6)?,
        artwork_url: row.get(7)?,
        track_count: row.get::<_, Option<u32>>(8)?.unwrap_or(0),
        duration_ms: row.get::<_, Option<i64>>(9)?.unwrap_or(0) as u64,
        date_created: row.get(10)?,
        downloaded_track_count: row.get::<_, Option<u32>>(11)?.unwrap_or(0),
    })
}

/// Internal search function that takes a connection, scoped to `source`.
pub fn search_albums_internal(
    conn: &Connection,
    query: &str,
    source: &str,
) -> Result<Vec<Album>, rusqlite::Error> {
    // Escape FTS special characters and add prefix matching
    let fts_query = format!("{}*", escape_fts_query(query));

    let mut stmt = conn.prepare(
        r#"
        SELECT a.jellyfin_id, a.release_mbid, a.name, a.artist_name,
               a.artist_jellyfin_id, a.year, a.genre, a.artwork_url,
               a.track_count, a.duration_ms, a.date_created,
               COUNT(t.jellyfin_id) AS downloaded_track_count
        FROM cached_albums a
        JOIN fts_albums f ON a.jellyfin_id = f.jellyfin_id
        LEFT JOIN cached_tracks t ON t.album_jellyfin_id = a.jellyfin_id
            AND +t.source = a.source
            AND t.cached_file_path IS NOT NULL
        WHERE fts_albums MATCH ?1 AND a.source = ?2
        GROUP BY a.jellyfin_id
        ORDER BY rank
        LIMIT 50
        "#,
    )?;

    let albums = stmt
        .query_map(params![fts_query, source], album_from_row)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(albums)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_album() -> CachedAlbum {
        CachedAlbum {
            jellyfin_id: "abc123def456abc123def456abc12345".to_string(),
            release_mbid: Some("mb-release-id".to_string()),
            name: "Test Album".to_string(),
            artist_name: Some("Test Artist".to_string()),
            artist_jellyfin_id: Some("artist123456789012345678901234".to_string()),
            year: Some(2024),
            genre: Some("Rock".to_string()),
            artwork_url: Some("http://example.com/art.jpg".to_string()),
            track_count: Some(10),
            duration_ms: Some(3600000),
            date_created: None,
        }
    }

    fn get_album(db: &Database, id: &str) -> Option<Album> {
        db.get_all_albums()
            .unwrap()
            .into_iter()
            .find(|a| a.jellyfin_id == id)
    }

    #[test]
    fn test_upsert_and_get_album() {
        let db = Database::open_in_memory().unwrap();
        let album = test_album();

        db.upsert_album(&album, ServerKind::Jellyfin, None).unwrap();

        let retrieved = get_album(&db, &album.jellyfin_id).unwrap();
        assert_eq!(retrieved.name, "Test Album");
        assert_eq!(retrieved.artist_name, "Test Artist");
        assert_eq!(retrieved.year, Some(2024));
    }

    #[test]
    fn test_get_all_albums() {
        let db = Database::open_in_memory().unwrap();

        let album1 = CachedAlbum {
            jellyfin_id: "album1111111111111111111111111111".to_string(),
            name: "Album A".to_string(),
            ..test_album()
        };
        let album2 = CachedAlbum {
            jellyfin_id: "album2222222222222222222222222222".to_string(),
            name: "Album B".to_string(),
            ..test_album()
        };

        db.upsert_albums(&[album1, album2], ServerKind::Jellyfin, None)
            .unwrap();

        let albums = db.get_all_albums().unwrap();
        assert_eq!(albums.len(), 2);
    }

    /// An artist's albums list is newest-first, matching Apple Music/Spotify;
    /// a missing or equal year falls back to the (folded) album title.
    #[test]
    fn test_get_albums_by_artist_orders_newest_first() {
        let db = Database::open_in_memory().unwrap();
        let artist_id = "artist123456789012345678901234";

        let oldest = CachedAlbum {
            jellyfin_id: "album-oldest-000000000000000000".to_string(),
            name: "First Album".to_string(),
            year: Some(1990),
            artist_jellyfin_id: Some(artist_id.to_string()),
            ..test_album()
        };
        let newest = CachedAlbum {
            jellyfin_id: "album-newest-000000000000000000".to_string(),
            name: "Latest Album".to_string(),
            year: Some(2020),
            artist_jellyfin_id: Some(artist_id.to_string()),
            ..test_album()
        };
        let no_year_b = CachedAlbum {
            jellyfin_id: "album-noyear-b-0000000000000000".to_string(),
            name: "B Side".to_string(),
            year: None,
            artist_jellyfin_id: Some(artist_id.to_string()),
            ..test_album()
        };
        let no_year_a = CachedAlbum {
            jellyfin_id: "album-noyear-a-0000000000000000".to_string(),
            name: "Á Side".to_string(),
            year: None,
            artist_jellyfin_id: Some(artist_id.to_string()),
            ..test_album()
        };

        db.upsert_albums(
            &[oldest, newest, no_year_b, no_year_a],
            ServerKind::Jellyfin,
            None,
        )
        .unwrap();

        let albums = db.get_albums_by_artist(artist_id).unwrap();
        let names: Vec<&str> = albums.iter().map(|a| a.name.as_str()).collect();
        // Newest year first; a missing year sorts after every real year and
        // ties break on the (accent-folded) title.
        assert_eq!(
            names,
            vec!["Latest Album", "First Album", "Á Side", "B Side"]
        );
    }

    /// get_all_albums groups by the same artist key the Artists page uses
    /// (sort_name, falling back to article-stripping) and, within an artist,
    /// orders newest release first — matching get_albums_by_artist.
    #[test]
    fn test_get_all_albums_orders_same_artist_newest_first_and_by_sort_name() {
        use crate::db::CachedArtist;

        let db = Database::open_in_memory().unwrap();
        let artist_id = "artist123456789012345678901234";

        // No sort_name reported: "Zappa, Frank" would otherwise sort under Z.
        let artist = CachedArtist {
            jellyfin_id: artist_id.to_string(),
            artist_mbid: None,
            name: "Zappa, Frank".to_string(),
            artwork_url: None,
            backdrop_url: None,
            album_count: Some(2),
            sort_name: Some("Zappa Frank".to_string()),
        };
        let other_artist = CachedArtist {
            jellyfin_id: "artist-air-000000000000000000000".to_string(),
            artist_mbid: None,
            name: "Air".to_string(),
            artwork_url: None,
            backdrop_url: None,
            album_count: Some(1),
            sort_name: None,
        };
        db.upsert_artists(&[artist, other_artist], ServerKind::Jellyfin, None)
            .unwrap();

        let oldest = CachedAlbum {
            jellyfin_id: "album-oldest-000000000000000000".to_string(),
            name: "First Album".to_string(),
            year: Some(1990),
            artist_name: Some("Zappa, Frank".to_string()),
            artist_jellyfin_id: Some(artist_id.to_string()),
            ..test_album()
        };
        let newest = CachedAlbum {
            jellyfin_id: "album-newest-000000000000000000".to_string(),
            name: "Latest Album".to_string(),
            year: Some(2020),
            artist_name: Some("Zappa, Frank".to_string()),
            artist_jellyfin_id: Some(artist_id.to_string()),
            ..test_album()
        };
        let air_album = CachedAlbum {
            jellyfin_id: "album-air-0000000000000000000000".to_string(),
            name: "Moon Safari".to_string(),
            year: Some(1998),
            artist_name: Some("Air".to_string()),
            artist_jellyfin_id: Some("artist-air-000000000000000000000".to_string()),
            ..test_album()
        };

        db.upsert_albums(&[oldest, newest, air_album], ServerKind::Jellyfin, None)
            .unwrap();

        let albums = db.get_all_albums().unwrap();
        let names: Vec<&str> = albums.iter().map(|a| a.name.as_str()).collect();
        // Air (sort_name "Air") before Zappa (sort_name "Zappa Frank"), and
        // within Zappa, newest release first.
        assert_eq!(names, vec!["Moon Safari", "Latest Album", "First Album"]);
    }

    /// The bug: every sync stamps the whole library with one shared
    /// `last_synced_at`, so ordering by it put albums in name order and the
    /// window matched everything. Ordering must follow the server's date-added.
    #[test]
    fn recently_added_orders_by_date_created_not_sync_stamp() {
        let db = Database::open_in_memory().unwrap();
        let now = current_timestamp();

        // Names chosen so alphabetical order is the exact reverse of date-added:
        // if the sort ever falls back to `a.name`, this test fails.
        let old = CachedAlbum {
            jellyfin_id: "album-aaa-oldest-0000000000000000".to_string(),
            name: "A Oldest".to_string(),
            date_created: Some(now - 20 * 86400),
            ..test_album()
        };
        let new = CachedAlbum {
            jellyfin_id: "album-zzz-newest-0000000000000000".to_string(),
            name: "Z Newest".to_string(),
            date_created: Some(now - 86400),
            ..test_album()
        };

        // One shared sync stamp across both rows — exactly what the sync does.
        db.upsert_albums(&[old, new], ServerKind::Jellyfin, Some(now))
            .unwrap();

        let albums = db.get_recently_added_albums(30).unwrap();
        assert_eq!(albums.len(), 2);
        assert_eq!(
            albums[0].name, "Z Newest",
            "newest date_created must sort first, regardless of name or sync stamp"
        );
        assert_eq!(albums[1].name, "A Oldest");
    }

    /// An album added to the server long ago must fall outside a short window
    /// even though this sync just touched its `last_synced_at`.
    #[test]
    fn recently_added_window_excludes_old_albums_touched_by_a_fresh_sync() {
        let db = Database::open_in_memory().unwrap();
        let now = current_timestamp();

        let ancient = CachedAlbum {
            jellyfin_id: "album-ancient-000000000000000000".to_string(),
            name: "Ancient".to_string(),
            date_created: Some(now - 400 * 86400),
            ..test_album()
        };
        db.upsert_albums(&[ancient], ServerKind::Jellyfin, Some(now))
            .unwrap();

        assert!(
            db.get_recently_added_albums(30).unwrap().is_empty(),
            "a fresh sync stamp must not drag a year-old album into the window"
        );
    }

    /// Rows cached before the v15 schema have no `date_created`. They must keep
    /// the ordering they had before `date_created` existed rather than vanishing from the view.
    #[test]
    fn recently_added_falls_back_to_sync_stamp_when_date_created_is_null() {
        let db = Database::open_in_memory().unwrap();
        let now = current_timestamp();

        let legacy = CachedAlbum {
            jellyfin_id: "album-legacy-0000000000000000000".to_string(),
            name: "Legacy".to_string(),
            date_created: None,
            ..test_album()
        };
        db.upsert_albums(&[legacy], ServerKind::Jellyfin, Some(now))
            .unwrap();

        let albums = db.get_recently_added_albums(30).unwrap();
        assert_eq!(
            albums.len(),
            1,
            "NULL date_created must coalesce, not filter"
        );
        assert_eq!(albums[0].date_created, None);
    }

    /// A later sync that reports no date (partial sync, or a backend that
    /// stopped sending it) must not wipe a date we already know.
    #[test]
    fn upsert_never_regresses_a_known_date_created_to_null() {
        let db = Database::open_in_memory().unwrap();
        let known = current_timestamp() - 5 * 86400;

        let with_date = CachedAlbum {
            date_created: Some(known),
            ..test_album()
        };
        db.upsert_album(&with_date, ServerKind::Jellyfin, None)
            .unwrap();

        let without_date = CachedAlbum {
            date_created: None,
            ..test_album()
        };
        db.upsert_album(&without_date, ServerKind::Jellyfin, None)
            .unwrap();

        let retrieved = get_album(&db, &test_album().jellyfin_id).unwrap();
        assert_eq!(
            retrieved.date_created,
            Some(known),
            "a None from a later sync must not overwrite a known date"
        );
    }
}
