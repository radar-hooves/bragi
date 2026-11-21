//! Artist database operations.
//!
//! CRUD operations for cached artist metadata.

use crate::mediaserver::ServerKind;
use crate::models::Artist;
use rusqlite::{params, Connection, Row};

use super::utils::{current_timestamp, escape_fts_query};
use super::{Database, DatabaseError};

/// Artist data for database insertion.
#[derive(Debug, Clone)]
pub struct CachedArtist {
    pub jellyfin_id: String,
    pub artist_mbid: Option<String>,
    pub name: String,
    pub artwork_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub album_count: Option<u32>,
    /// Jellyfin's own SortName, when the backend reports one.
    pub sort_name: Option<String>,
}

impl Database {
    /// Insert or update multiple artists in a transaction.
    ///
    /// # Arguments
    ///
    /// * `artists` - The artists to upsert
    /// * `timestamp` - Optional sync timestamp. If None, uses current time.
    ///   Pass a shared timestamp during sync operations to ensure
    ///   consistent stale entry detection.
    pub fn upsert_artists(
        &self,
        artists: &[CachedArtist],
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
                    INSERT INTO cached_artists (
                        jellyfin_id, artist_mbid, name, artwork_url, backdrop_url, album_count,
                        last_synced_at, source, sort_name
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                    ON CONFLICT(jellyfin_id) DO UPDATE SET
                        artist_mbid = excluded.artist_mbid,
                        name = excluded.name,
                        artwork_url = excluded.artwork_url,
                        backdrop_url = excluded.backdrop_url,
                        album_count = excluded.album_count,
                        last_synced_at = excluded.last_synced_at,
                        source = excluded.source,
                        sort_name = excluded.sort_name
                    "#,
                )?;

                for artist in artists {
                    stmt.execute(params![
                        artist.jellyfin_id,
                        artist.artist_mbid,
                        artist.name,
                        artist.artwork_url,
                        artist.backdrop_url,
                        artist.album_count,
                        now,
                        source,
                        artist.sort_name,
                    ])?;
                }
            }

            tx.commit()?;
            Ok(())
        })
    }

    /// Get all cached artists for the active backend.
    pub fn get_all_artists(&self) -> Result<Vec<Artist>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT jellyfin_id, artist_mbid, name, artwork_url, backdrop_url, album_count, sort_name
                FROM cached_artists
                WHERE source = ?1
                ORDER BY COALESCE(NULLIF(sort_name, ''), name) COLLATE BRAGI_ARTIST
                "#,
            )?;

            let artists = stmt
                .query_map([source], artist_from_row)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(artists)
        })
    }

    /// Search artists by name using FTS (scoped to the active backend).
    pub fn search_artists(&self, query: &str) -> Result<Vec<Artist>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| search_artists_internal(conn, query, source))
    }

    /// Delete a backend's artists not synced since the given timestamp.
    ///
    /// Scoped to `source` (the backend being synced, not the active source) so a
    /// sync of one backend can never delete the other backend's artists.
    pub fn delete_stale_artists(
        &self,
        since_timestamp: i64,
        source: &str,
    ) -> Result<u32, DatabaseError> {
        self.with_conn(|conn| {
            let rows = conn.execute(
                "DELETE FROM cached_artists WHERE last_synced_at < ?1 AND source = ?2",
                params![since_timestamp, source],
            )?;
            Ok(rows as u32)
        })
    }
}

/// Convert a database row to an Artist model.
fn artist_from_row(row: &Row<'_>) -> Result<Artist, rusqlite::Error> {
    Ok(Artist {
        jellyfin_id: row.get(0)?,
        mbid: row.get(1)?,
        name: row.get(2)?,
        artwork_url: row.get(3)?,
        backdrop_url: row.get(4)?,
        album_count: row.get::<_, Option<u32>>(5)?.unwrap_or(0),
        sort_name: row.get(6)?,
    })
}

/// Internal search function that takes a connection, scoped to `source`.
pub fn search_artists_internal(
    conn: &Connection,
    query: &str,
    source: &str,
) -> Result<Vec<Artist>, rusqlite::Error> {
    // Escape FTS special characters and add prefix matching
    let fts_query = format!("{}*", escape_fts_query(query));

    let mut stmt = conn.prepare(
        r#"
        SELECT a.jellyfin_id, a.artist_mbid, a.name, a.artwork_url, a.backdrop_url, a.album_count, a.sort_name
        FROM cached_artists a
        JOIN fts_artists f ON a.jellyfin_id = f.jellyfin_id
        WHERE fts_artists MATCH ?1 AND a.source = ?2
        ORDER BY rank
        LIMIT 50
        "#,
    )?;

    let artists = stmt
        .query_map(params![fts_query, source], artist_from_row)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(artists)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_artist() -> CachedArtist {
        CachedArtist {
            jellyfin_id: "artist12345678901234567890123456".to_string(),
            artist_mbid: Some("mb-artist-id".to_string()),
            name: "Test Artist".to_string(),
            artwork_url: Some("http://example.com/artist.jpg".to_string()),
            backdrop_url: Some("http://example.com/backdrop.jpg".to_string()),
            album_count: Some(5),
            sort_name: None,
        }
    }

    #[test]
    fn test_upsert_and_get_artist() {
        let db = Database::open_in_memory().unwrap();
        let artist = test_artist();

        db.upsert_artists(std::slice::from_ref(&artist), ServerKind::Jellyfin, None)
            .unwrap();

        let retrieved = db.get_all_artists().unwrap().remove(0);
        assert_eq!(retrieved.name, "Test Artist");
        assert_eq!(retrieved.album_count, 5);
    }

    #[test]
    fn test_get_all_artists() {
        let db = Database::open_in_memory().unwrap();

        let artist1 = CachedArtist {
            jellyfin_id: "artist11111111111111111111111111".to_string(),
            name: "Artist A".to_string(),
            ..test_artist()
        };
        let artist2 = CachedArtist {
            jellyfin_id: "artist22222222222222222222222222".to_string(),
            name: "Artist B".to_string(),
            ..test_artist()
        };

        db.upsert_artists(&[artist1, artist2], ServerKind::Jellyfin, None)
            .unwrap();

        let artists = db.get_all_artists().unwrap();
        assert_eq!(artists.len(), 2);
        // Should be sorted by name
        assert_eq!(artists[0].name, "Artist A");
        assert_eq!(artists[1].name, "Artist B");
    }

    /// Artists sort ignoring a leading "The", and prefer Jellyfin's own
    /// SortName over stripping it ourselves when the backend reports one.
    #[test]
    fn test_get_all_artists_orders_by_sort_name_and_strips_leading_article() {
        let db = Database::open_in_memory().unwrap();

        // No sort_name: falls back to stripping the leading article itself.
        let beatles = CachedArtist {
            jellyfin_id: "artist-beatles-00000000000000000".to_string(),
            name: "The Beatles".to_string(),
            sort_name: None,
            ..test_artist()
        };
        // Explicit sort_name wins even though it wouldn't sort first on its
        // un-stripped display name.
        let air = CachedArtist {
            jellyfin_id: "artist-air-000000000000000000000".to_string(),
            name: "Air".to_string(),
            sort_name: Some("Air".to_string()),
            ..test_artist()
        };
        let zappa = CachedArtist {
            jellyfin_id: "artist-zappa-0000000000000000000".to_string(),
            name: "Zappa, Frank".to_string(),
            sort_name: Some("Zappa Frank".to_string()),
            ..test_artist()
        };

        db.upsert_artists(&[beatles, air, zappa], ServerKind::Jellyfin, None)
            .unwrap();

        let artists = db.get_all_artists().unwrap();
        let names: Vec<&str> = artists.iter().map(|a| a.name.as_str()).collect();
        // Air, Beatles (via "Beatles" after stripping "The"), Zappa (via "Zappa Frank")
        assert_eq!(names, vec!["Air", "The Beatles", "Zappa, Frank"]);
    }
}
