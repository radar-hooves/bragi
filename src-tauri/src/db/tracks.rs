//! Track database operations.
//!
//! CRUD operations for cached track metadata.

use crate::mediaserver::ServerKind;
use crate::models::Track;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::utils::{current_timestamp, escape_fts_query};
use super::{Database, DatabaseError};

/// The `Track` projection every track query shares: the 17 columns
/// `track_from_row` reads, from `cached_tracks t` joined to its album `a`.
macro_rules! track_select {
    () => {
        "SELECT t.jellyfin_id, t.track_mbid, t.artist_mbid, t.release_mbid, \
         t.name, t.artist_name, \
         t.album_jellyfin_id, COALESCE(a.name, '') as album_name, \
         t.track_number, t.disc_number, t.duration_ms, t.genre, \
         a.year, t.artwork_url, t.cached_file_path, t.file_size_bytes, \
         t.normalization_gain_db \
         FROM cached_tracks t \
         LEFT JOIN cached_albums a ON t.album_jellyfin_id = a.jellyfin_id"
    };
}

/// Parameters for library view filtering and sorting.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryViewParams {
    /// Filter by genre names (OR within set).
    pub genres: Option<Vec<String>>,
    /// Filter by artist names (OR within set).
    pub artists: Option<Vec<String>>,
    /// Filter by album Jellyfin IDs (OR within set).
    pub album_ids: Option<Vec<String>>,
    /// Only return tracks with cached audio files.
    pub offline_only: Option<bool>,
    /// Whitelisted sort key.
    pub sort: Option<String>,
}

/// Column browser data for the library view.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnBrowserData {
    pub genres: Vec<String>,
    pub artists: Vec<String>,
    pub albums: Vec<ColumnBrowserAlbum>,
    pub total_track_count: u32,
}

/// An album entry in the column browser.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnBrowserAlbum {
    pub id: String,
    pub name: String,
    pub artist_name: String,
}

/// Complete library view result with column browser data and filtered tracks.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryViewResult {
    pub column_browser: ColumnBrowserData,
    pub tracks: Vec<Track>,
}

/// Track data for database insertion.
#[derive(Debug, Clone)]
pub struct CachedTrack {
    pub jellyfin_id: String,
    pub track_mbid: Option<String>,
    pub artist_mbid: Option<String>,
    pub release_mbid: Option<String>,
    pub name: String,
    pub artist_name: Option<String>,
    pub album_jellyfin_id: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub duration_ms: Option<u64>,
    pub genre: Option<String>,
    pub artwork_url: Option<String>,
    pub cached_file_path: Option<String>,
    pub file_size_bytes: Option<u64>,
    /// ReplayGain normalization value in dB from Jellyfin.
    pub normalization_gain_db: Option<f32>,
    /// Server-side play count reported by the media server (Jellyfin
    /// `UserData.PlayCount` / OpenSubsonic inline `playCount`). `None` when the
    /// backend doesn't report one. Read-only; source-scoped via `source`.
    pub server_play_count: Option<u32>,
}

impl CachedTrack {
    /// A server-reported track as a cache row, with its play count if known.
    pub fn from_track(t: &Track, server_play_count: Option<u32>) -> Self {
        Self {
            jellyfin_id: t.jellyfin_id.clone(),
            track_mbid: t.mbid.clone(),
            artist_mbid: t.artist_mbid.clone(),
            release_mbid: t.release_mbid.clone(),
            name: t.name.clone(),
            artist_name: Some(t.artist_name.clone()),
            album_jellyfin_id: Some(t.album_jellyfin_id.clone()),
            track_number: Some(t.track_number),
            disc_number: Some(t.disc_number),
            duration_ms: Some(t.duration_ms),
            genre: t.genre.clone(),
            artwork_url: t.artwork_url.clone(),
            cached_file_path: t.cached_file_path.clone(),
            file_size_bytes: t.file_size_bytes,
            normalization_gain_db: t.normalization_gain_db,
            server_play_count,
        }
    }
}

impl Database {
    /// Insert or update multiple tracks in a transaction.
    ///
    /// # Arguments
    ///
    /// * `tracks` - The tracks to upsert
    /// * `timestamp` - Optional sync timestamp. If None, uses current time.
    ///   Pass a shared timestamp during sync operations to ensure
    ///   consistent stale entry detection.
    pub fn upsert_tracks(
        &self,
        tracks: &[CachedTrack],
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
                    INSERT INTO cached_tracks (
                        jellyfin_id, track_mbid, artist_mbid, release_mbid, name, artist_name,
                        album_jellyfin_id, track_number, disc_number, duration_ms, genre,
                        artwork_url, normalization_gain_db, cached_file_path, file_size_bytes,
                        server_play_count, last_synced_at, source
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
                    ON CONFLICT(jellyfin_id) DO UPDATE SET
                        track_mbid = excluded.track_mbid,
                        artist_mbid = excluded.artist_mbid,
                        release_mbid = excluded.release_mbid,
                        name = excluded.name,
                        artist_name = excluded.artist_name,
                        album_jellyfin_id = excluded.album_jellyfin_id,
                        track_number = excluded.track_number,
                        disc_number = excluded.disc_number,
                        duration_ms = excluded.duration_ms,
                        genre = excluded.genre,
                        artwork_url = excluded.artwork_url,
                        normalization_gain_db = excluded.normalization_gain_db,
                        cached_file_path = COALESCE(excluded.cached_file_path, cached_tracks.cached_file_path),
                        file_size_bytes = COALESCE(excluded.file_size_bytes, cached_tracks.file_size_bytes),
                        server_play_count = COALESCE(excluded.server_play_count, cached_tracks.server_play_count),
                        last_synced_at = excluded.last_synced_at,
                        source = excluded.source
                    "#,
                )?;

                for track in tracks {
                    stmt.execute(params![
                        track.jellyfin_id,
                        track.track_mbid,
                        track.artist_mbid,
                        track.release_mbid,
                        track.name,
                        track.artist_name,
                        track.album_jellyfin_id,
                        track.track_number,
                        track.disc_number,
                        track.duration_ms.map(|d| d as i64),
                        track.genre,
                        track.artwork_url,
                        track.normalization_gain_db,
                        track.cached_file_path,
                        track.file_size_bytes.map(|s| s as i64),
                        track.server_play_count,
                        now,
                        source,
                    ])?;
                }
            }

            for track in tracks {
                write_track_genres(&tx, &track.jellyfin_id, track.genre.as_deref())?;
            }

            tx.commit()?;
            Ok(())
        })
    }

    /// Get a track by Jellyfin ID (scoped to the active backend).
    pub fn get_track(&self, jellyfin_id: &str) -> Result<Option<Track>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(concat!(
                track_select!(),
                r#"
                WHERE t.jellyfin_id = ?1 AND t.source = ?2
                "#
            ))?;

            let track = stmt
                .query_row(params![jellyfin_id, source], track_from_row)
                .optional()?;

            Ok(track)
        })
    }

    /// Find a unique cached track by artist/album/title (case-insensitive),
    /// scoped to the active backend. Ambiguous matches stay unresolved.
    ///
    /// Used to re-resolve a held scrobble/skip that was queued with no
    /// usable Jellyfin id in the first place — such a row can never be
    /// looked up by id, only by the metadata it already carries.
    pub fn find_track_by_metadata(
        &self,
        artist: &str,
        album: Option<&str>,
        title: &str,
    ) -> Result<Option<Track>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(concat!(
                track_select!(),
                r#"
                WHERE t.source = ?1
                  AND t.name = ?2 COLLATE NOCASE
                  AND t.artist_name = ?3 COLLATE NOCASE
                  AND (?4 IS NULL OR a.name = ?4 COLLATE NOCASE)
                LIMIT 2
                "#
            ))?;

            let matches = stmt
                .query_map(params![source, title, artist, album], track_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(if matches.len() == 1 {
                matches.into_iter().next()
            } else {
                None
            })
        })
    }

    /// Get all cached tracks for the active backend.
    pub fn get_all_tracks(&self) -> Result<Vec<Track>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(concat!(
                track_select!(),
                r#"
                LEFT JOIN cached_artists ar ON ar.jellyfin_id = a.artist_jellyfin_id
                WHERE t.source = ?1
                ORDER BY COALESCE(NULLIF(ar.sort_name, ''), t.artist_name) COLLATE BRAGI_ARTIST,
                         a.name COLLATE BRAGI_FOLD, t.disc_number, t.track_number
                "#
            ))?;

            let tracks = stmt
                .query_map([source], track_from_row)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(tracks)
        })
    }

    /// Get tracks for a specific album.
    pub fn get_tracks_by_album(
        &self,
        album_jellyfin_id: &str,
    ) -> Result<Vec<Track>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(concat!(
                track_select!(),
                r#"
                WHERE t.album_jellyfin_id = ?1 AND t.source = ?2
                ORDER BY t.disc_number, t.track_number, t.name COLLATE BRAGI_FOLD
                "#
            ))?;

            let tracks = stmt
                .query_map(params![album_jellyfin_id, source], track_from_row)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(tracks)
        })
    }

    /// Search tracks by name using FTS (scoped to the active backend).
    pub fn search_tracks(&self, query: &str) -> Result<Vec<Track>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| search_tracks_internal(conn, query, source))
    }

    /// Update the cached file path for a track.
    pub fn set_track_cached_path(
        &self,
        jellyfin_id: &str,
        file_path: &str,
        file_size_bytes: u64,
    ) -> Result<(), DatabaseError> {
        let now = current_timestamp();

        let rows_affected = self.with_conn(|conn| {
            conn.execute(
                r#"
                UPDATE cached_tracks
                SET cached_file_path = ?1,
                    file_size_bytes = ?2,
                    cached_at = ?3
                WHERE jellyfin_id = ?4
                "#,
                params![file_path, file_size_bytes as i64, now, jellyfin_id],
            )
        })?;
        if rows_affected == 0 {
            return Err(DatabaseError::NotFound(format!(
                "Track {} not found in cached_tracks — cannot set cached path",
                jellyfin_id
            )));
        }
        Ok(())
    }

    /// Clear the cached file path for a track.
    pub fn clear_track_cached_path(&self, jellyfin_id: &str) -> Result<(), DatabaseError> {
        self.with_conn(|conn| {
            conn.execute(
                r#"
                UPDATE cached_tracks
                SET cached_file_path = NULL,
                    file_size_bytes = NULL,
                    cached_at = NULL
                WHERE jellyfin_id = ?1
                "#,
                [jellyfin_id],
            )?;
            Ok(())
        })
    }

    /// Get all tracks with cached files for the active backend.
    pub fn get_cached_tracks(&self) -> Result<Vec<Track>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(concat!(
                track_select!(),
                r#"
                WHERE t.cached_file_path IS NOT NULL AND t.source = ?1
                ORDER BY t.cached_at DESC
                "#
            ))?;

            let tracks = stmt
                .query_map([source], track_from_row)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(tracks)
        })
    }

    /// Per-genre counts for the active backend, aggregated in SQL.
    ///
    /// Returned A-Z, case-insensitive. A genre with no display casing recorded
    /// cannot occur (the join table always stores one), so an unmatched key
    /// falls back to itself rather than being dropped.
    pub fn genre_stats(&self) -> Result<Vec<GenreStat>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| {
            let displays = genre_displays(conn, source)?;
            let mut stmt = conn.prepare_cached(
                "SELECT g.genre_key, \
                        COUNT(DISTINCT g.track_id), \
                        COUNT(DISTINCT t.artist_name), \
                        COUNT(DISTINCT t.album_jellyfin_id), \
                        COALESCE(SUM(t.duration_ms), 0), \
                        MIN(a.year), MAX(a.year) \
                 FROM track_genres g \
                 JOIN cached_tracks t ON t.jellyfin_id = g.track_id \
                 LEFT JOIN cached_albums a ON a.jellyfin_id = t.album_jellyfin_id \
                 WHERE t.source = ?1 \
                 GROUP BY g.genre_key",
            )?;
            let mut mosaics = genre_mosaics(conn, source)?;
            let mut stats = stmt
                .query_map([source], |row| {
                    let key: String = row.get(0)?;
                    let album_ids = mosaics.remove(&key).unwrap_or_default();
                    Ok(GenreStat {
                        name: displays.get(&key).cloned().unwrap_or(key),
                        track_count: row.get::<_, i64>(1)? as u32,
                        artist_count: row.get::<_, i64>(2)? as u32,
                        album_count: row.get::<_, i64>(3)? as u32,
                        duration_ms: row.get::<_, i64>(4)?.max(0) as u64,
                        year_min: row.get(5)?,
                        year_max: row.get(6)?,
                        album_ids,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            stats.sort_by_key(|g| (g.name.to_lowercase(), g.name.clone()));
            Ok(stats)
        })
    }

    /// Distinct normalised genres across the active backend's library (cheap —
    /// no track rows). Used by AI-playlist candidate pre-filtering.
    ///
    /// Reads the `track_genres` join table, so a multi-value tag string
    /// contributes each of its genres rather than appearing as one junk
    /// "genre". One row per case-folded key, displayed in the
    /// lexicographically first casing seen for it — deterministic, so the list
    /// does not reshuffle between launches.
    pub fn distinct_genres(&self) -> Result<Vec<String>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| list_genres(conn, source))
    }

    /// Distinct non-empty artist names across the active backend's library
    /// (cheap — no track rows). Used to resolve AI-playlist artist mentions.
    ///
    /// Ordered by the same sort_name-or-article-strip key as every other
    /// artist ordering: a track carries no artist id of its
    /// own, so this reaches cached_artists' sort_name via the track's album.
    pub fn distinct_artists(&self) -> Result<Vec<String>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(DISTINCT_ARTISTS_SQL)?;
            let artists = stmt
                .query_map([source], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(artists)
        })
    }

    /// Server-side play counts for the active backend, keyed by server id.
    ///
    /// Scoped to `active_source()` (like [`Database::get_all_track_mbids`]) so a
    /// Subsonic library's counts never blend with a Jellyfin one. Only rows with
    /// a non-NULL, non-zero `server_play_count` are returned — a zero conveys no
    /// information and lets the frontend fall back to the scrobble server cleanly.
    pub fn get_server_play_counts(
        &self,
    ) -> Result<std::collections::HashMap<String, u32>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT jellyfin_id, server_play_count FROM cached_tracks \
                 WHERE source = ?1 AND server_play_count IS NOT NULL AND server_play_count > 0",
            )?;
            let rows = stmt.query_map([source], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
            })?;
            let mut map = std::collections::HashMap::new();
            for row in rows {
                let (id, count) = row?;
                map.insert(id, count);
            }
            Ok(map)
        })
    }

    /// Get total size of cached audio files in bytes.
    ///
    /// Deliberately GLOBAL (not source-scoped): this drives disk accounting and
    /// LRU eviction, which must account for both backends' downloads.
    pub fn get_cached_files_size(&self) -> Result<u64, DatabaseError> {
        self.with_conn(|conn| {
            let size: i64 = conn.query_row(
                "SELECT COALESCE(SUM(file_size_bytes), 0) FROM cached_tracks WHERE cached_file_path IS NOT NULL",
                [],
                |row| row.get(0),
            )?;
            Ok(size as u64)
        })
    }

    /// Delete a backend's tracks not synced since the given timestamp.
    ///
    /// Scoped to `source` (not the active source) so a sync of one backend can
    /// never delete the other backend's rows, even if the active backend was
    /// toggled mid-sync. The caller passes the backend it is syncing.
    pub fn delete_stale_tracks(
        &self,
        since_timestamp: i64,
        source: &str,
    ) -> Result<u32, DatabaseError> {
        self.with_conn(|conn| {
            let rows = conn.execute(
                "DELETE FROM cached_tracks WHERE last_synced_at < ?1 AND source = ?2",
                params![since_timestamp, source],
            )?;
            Ok(rows as u32)
        })
    }

    /// Get cached tracks for LRU eviction.
    ///
    /// Returns (jellyfin_id, cached_file_path, file_size_bytes) tuples
    /// ordered by oldest cached_at first (for LRU eviction). Deliberately GLOBAL
    /// (not source-scoped): eviction frees disk across both backends.
    pub fn get_cached_tracks_for_eviction(
        &self,
    ) -> Result<Vec<(String, String, u64)>, DatabaseError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT jellyfin_id, cached_file_path, file_size_bytes
                FROM cached_tracks
                WHERE cached_file_path IS NOT NULL
                ORDER BY cached_at ASC
                "#,
            )?;

            let tracks = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)? as u64,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(tracks)
        })
    }
    /// Query the library view with column browser data and filtered tracks.
    ///
    /// Executes five queries in a single lock acquisition:
    /// 1. Total track count (always unfiltered)
    /// 2. Genres (always unfiltered)
    /// 3. Artists (filtered by selected genres only)
    /// 4. Albums (filtered by genres + artists)
    /// 5. Tracks (filtered by all params)
    pub fn query_library_view(
        &self,
        params: &LibraryViewParams,
    ) -> Result<LibraryViewResult, DatabaseError> {
        // Snapshot the active backend once; every sub-query is scoped to it so
        // the column browser and track list only ever show one library.
        let source = self.active_source().as_str();
        self.with_conn(|conn| {
            // 1. Total track count (scoped to the active backend, else unfiltered)
            let total_track_count: u32 = {
                let mut stmt = conn
                    .prepare_cached("SELECT COUNT(*) FROM cached_tracks WHERE source = ?1")?;
                stmt.query_row([source], |row| row.get::<_, i64>(0))? as u32
            };

            // 2. Genres (scoped to the active backend, else unfiltered).
            // Same normalised list the Genres view shows.
            let genres: Vec<String> = list_genres(conn, source)?;

            // 3. Artists (scoped to the active backend, filtered by selected genres)
            let artists: Vec<String> = {
                let genre_values = non_empty_filter(&params.genres);

                if let Some(values) = &genre_values {
                    // Membership test against track_genres, not string equality
                    // on the raw tag. source is ?1; keys start at ?2.
                    let values = genre_keys(values);
                    let placeholders = numbered_placeholders(2, values.len());
                    // GROUP BY (not DISTINCT) so the sort_name-or-article-strip
                    // key, unselected, can still drive ORDER BY — same key as
                    // DISTINCT_ARTISTS_SQL and every other artist ordering.
                    let sql = format!(
                        "SELECT t.artist_name FROM cached_tracks t \
                         LEFT JOIN cached_albums a ON t.album_jellyfin_id = a.jellyfin_id \
                         LEFT JOIN cached_artists ar ON ar.jellyfin_id = a.artist_jellyfin_id \
                         WHERE t.source = ?1 AND t.artist_name IS NOT NULL \
                           AND t.jellyfin_id IN (SELECT track_id FROM track_genres WHERE genre_key IN ({})) \
                         GROUP BY t.artist_name \
                         ORDER BY MIN(COALESCE(NULLIF(ar.sort_name, ''), t.artist_name)) COLLATE BRAGI_ARTIST",
                        placeholders
                    );
                    let mut stmt = conn.prepare_cached(&sql)?;
                    let mut bind: Vec<&dyn rusqlite::types::ToSql> =
                        vec![&source as &dyn rusqlite::types::ToSql];
                    bind.extend(values.iter().map(|v| v as &dyn rusqlite::types::ToSql));
                    let result = stmt
                        .query_map(bind.as_slice(), |row| row.get(0))?
                        .collect::<Result<Vec<_>, _>>()?;
                    result
                } else {
                    let mut stmt = conn.prepare_cached(DISTINCT_ARTISTS_SQL)?;
                    let result = stmt
                        .query_map([source], |row| row.get(0))?
                        .collect::<Result<Vec<_>, _>>()?;
                    result
                }
            };

            // 4. Albums (scoped to the active backend, filtered by genres + artists)
            let albums: Vec<ColumnBrowserAlbum> = {
                let genre_values = non_empty_filter(&params.genres);
                let artist_values = non_empty_filter(&params.artists);

                // Seed source as ?1 so the genre/artist blocks below auto-place
                // at ?2.. via their `start = bind_values.len() + 1` logic.
                let mut conditions = vec!["t.source = ?1".to_string()];
                let mut bind_values: Vec<String> = vec![source.to_string()];

                conditions.push("t.album_jellyfin_id IS NOT NULL".to_string());

                if let Some(values) = &genre_values {
                    let values = genre_keys(values);
                    let start = bind_values.len() + 1;
                    let placeholders = numbered_placeholders(start, values.len());
                    conditions.push(format!(
                        "t.jellyfin_id IN (SELECT track_id FROM track_genres WHERE genre_key IN ({}))",
                        placeholders
                    ));
                    bind_values.extend(values);
                }

                if let Some(values) = &artist_values {
                    let start = bind_values.len() + 1;
                    let placeholders = numbered_placeholders(start, values.len());
                    conditions.push(format!("t.artist_name IN ({})", placeholders));
                    bind_values.extend(values.iter().cloned());
                }

                let where_clause = conditions.join(" AND ");
                let sql = format!(
                    "SELECT DISTINCT t.album_jellyfin_id, COALESCE(a.name, ''), COALESCE(a.artist_name, '') \
                     FROM cached_tracks t \
                     LEFT JOIN cached_albums a ON t.album_jellyfin_id = a.jellyfin_id \
                     WHERE {} \
                     ORDER BY a.name COLLATE BRAGI_FOLD",
                    where_clause
                );

                let mut stmt = conn.prepare_cached(&sql)?;
                let rows = stmt.query_map(params_from_iter(bind_values.iter()), |row| {
                    Ok(ColumnBrowserAlbum {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        artist_name: row.get(2)?,
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>()?
            };

            // 5. Tracks (scoped to the active backend, filtered by all params)
            let (sql, bind_values) = build_tracks_query(params, source);
            let mut stmt = conn.prepare(&sql)?;
            let tracks = stmt
                .query_map(params_from_iter(bind_values.iter()), track_from_row)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(LibraryViewResult {
                column_browser: ColumnBrowserData {
                    genres,
                    artists,
                    albums,
                    total_track_count,
                },
                tracks,
            })
        })
    }

    /// Query tracks with filtering and sorting (without column browser data),
    /// scoped to the active backend.
    pub fn query_tracks(&self, params: &LibraryViewParams) -> Result<Vec<Track>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| {
            let (sql, bind_values) = build_tracks_query(params, source);
            let mut stmt = conn.prepare(&sql)?;
            let tracks = stmt
                .query_map(params_from_iter(bind_values.iter()), track_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(tracks)
        })
    }
}

/// Every distinct non-empty artist name for the active backend, ordered by
/// cached_artists' own sort_name where a track's album has one, falling back
/// to article-stripping the denormalised name otherwise. A
/// track carries no artist id of its own, so this reaches sort_name via the
/// track's album's `artist_jellyfin_id` — the same route `build_tracks_query`
/// and `get_all_tracks` use. `GROUP BY` (not `DISTINCT`) so the sort key,
/// which isn't in the selected column, can still drive `ORDER BY`.
const DISTINCT_ARTISTS_SQL: &str = "SELECT t.artist_name \
     FROM cached_tracks t \
     LEFT JOIN cached_albums a ON t.album_jellyfin_id = a.jellyfin_id \
     LEFT JOIN cached_artists ar ON ar.jellyfin_id = a.artist_jellyfin_id \
     WHERE t.source = ?1 AND t.artist_name IS NOT NULL AND t.artist_name != '' \
     GROUP BY t.artist_name \
     ORDER BY MIN(COALESCE(NULLIF(ar.sort_name, ''), t.artist_name)) COLLATE BRAGI_ARTIST";

/// Build a complete track query SQL string and bind parameter values.
///
/// Constructs WHERE conditions from the given filter parameters and maps
/// the sort key to a whitelisted ORDER BY clause.
fn build_tracks_query(params: &LibraryViewParams, source: &str) -> (String, Vec<String>) {
    // Seed source as the first bound predicate (?1) so the library view is
    // scoped to the active backend; the filter blocks below auto-place
    // at ?2.. via their `start = bind_values.len() + 1` logic.
    let mut conditions: Vec<String> = vec!["t.source = ?1".to_string()];
    let mut bind_values: Vec<String> = vec![source.to_string()];

    if let Some(values) = non_empty_filter(&params.genres) {
        // A genre filter is a membership test against track_genres, so a track
        // tagged "Alternative Metal;Heavy Metal;Rock" is returned for each of
        // the three, and a track in several selected genres is still returned
        // once — the IN-subquery form cannot duplicate a row the way a JOIN
        // would, which is what keeps the StatusBar total honest.
        let values = genre_keys(&values);
        let start = bind_values.len() + 1;
        let placeholders = numbered_placeholders(start, values.len());
        conditions.push(format!(
            "t.jellyfin_id IN (SELECT track_id FROM track_genres WHERE genre_key IN ({}))",
            placeholders
        ));
        bind_values.extend(values);
    }

    // The narrowest selected column wins: an album selection subsumes the artist
    // one for the TRACK list. Bragi derives a track's artist_name and an album's
    // artistName from Jellyfin's AlbumArtist, but that tag is not always present
    // — both then fall back to their own first contributing artist, which on a
    // compilation disagree. ANDing the two would silently show one contributor's
    // tracks, or none, for an album the user just opened. The column browser's
    // own album list is still narrowed by artist, so this only ever widens the
    // track list back to the album the user actually picked.
    let album_selected = non_empty_filter(&params.album_ids).is_some();

    if let Some(values) = non_empty_filter(&params.artists).filter(|_| !album_selected) {
        let start = bind_values.len() + 1;
        let placeholders = numbered_placeholders(start, values.len());
        conditions.push(format!("t.artist_name IN ({})", placeholders));
        bind_values.extend(values);
    }

    if let Some(values) = non_empty_filter(&params.album_ids) {
        let start = bind_values.len() + 1;
        let placeholders = numbered_placeholders(start, values.len());
        conditions.push(format!("t.album_jellyfin_id IN ({})", placeholders));
        bind_values.extend(values);
    }

    if params.offline_only.unwrap_or(false) {
        conditions.push("t.cached_file_path IS NOT NULL".to_string());
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Tie-breaks mirror trackColumns.ts's TIE_BREAKS chain: whichever column
    // is the primary sort, the runner-up is always album order (artist/album,
    // then disc, then track) so a tie never falls back to source-row order.
    //
    // The artist key prefers cached_artists' own sort_name over the
    // denormalised t.artist_name, matching every other artist ordering in
    // this file and get_all_albums — an artist otherwise sorted differently
    // here than on the Artists page.
    const ARTIST_KEY: &str =
        "COALESCE(NULLIF(ar.sort_name, ''), t.artist_name) COLLATE BRAGI_ARTIST";
    let track_number_order =
        format!("{ARTIST_KEY}, a.name COLLATE BRAGI_FOLD, t.disc_number, t.track_number");
    let order_by = match params.sort.as_deref() {
        Some("track-number") => track_number_order.clone(),
        Some("name-asc") => {
            format!("t.name COLLATE BRAGI_FOLD ASC, {ARTIST_KEY}, a.name COLLATE BRAGI_FOLD")
        }
        Some("name-desc") => {
            format!("t.name COLLATE BRAGI_FOLD DESC, {ARTIST_KEY}, a.name COLLATE BRAGI_FOLD")
        }
        Some("artist-asc") => {
            format!("{ARTIST_KEY} ASC, a.name COLLATE BRAGI_FOLD, t.disc_number, t.track_number")
        }
        Some("album-asc") => {
            format!("a.name COLLATE BRAGI_FOLD ASC, {ARTIST_KEY}, t.disc_number, t.track_number")
        }
        Some("duration-asc") => "t.duration_ms ASC".to_string(),
        Some("duration-desc") => "t.duration_ms DESC".to_string(),
        _ => track_number_order.clone(),
    };

    let sql = format!(
        concat!(
            track_select!(),
            " LEFT JOIN cached_artists ar ON ar.jellyfin_id = a.artist_jellyfin_id {} ORDER BY {}"
        ),
        where_clause, order_by
    );

    (sql, bind_values)
}

/// Generate numbered SQL placeholders starting from `start` for `count` parameters.
///
/// e.g. `numbered_placeholders(3, 2)` returns `"?3, ?4"`.
fn numbered_placeholders(start: usize, count: usize) -> String {
    (start..start + count)
        .map(|i| format!("?{}", i))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Return `Some(values)` if the filter is present and non-empty, otherwise `None`.
fn non_empty_filter(filter: &Option<Vec<String>>) -> Option<Vec<String>> {
    filter.as_ref().filter(|v| !v.is_empty()).cloned()
}

/// Per-genre aggregate for the Genres view.
///
/// Counted in SQL over `track_genres`, replacing a client-side walk of every
/// track. A track in three genres contributes to all three, once each.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenreStat {
    /// Display name; also what a genre filter is bound with.
    pub name: String,
    pub track_count: u32,
    pub artist_count: u32,
    pub album_count: u32,
    pub duration_ms: u64,
    pub year_min: Option<u32>,
    pub year_max: Option<u32>,
    /// Up to four album ids for the grid tile's cover mosaic, ordered by track
    /// count then album id — deterministic, so a genre's tile is the same
    /// every launch.
    pub album_ids: Vec<String>,
}

/// How many covers a grid tile's mosaic holds.
const MOSAIC_TILES: usize = 4;

/// The library's distinct (genre key, display casing) pairs for one backend.
///
/// DISTINCT keeps this to the handful of casings actually tagged, not one row
/// per track — it is cheap enough to reduce in Rust, where the choice of which
/// casing to show can be stated readably.
const GENRE_CASINGS_SQL: &str = "SELECT DISTINCT g.genre_key, g.genre_display \
     FROM track_genres g \
     JOIN cached_tracks t ON t.jellyfin_id = g.track_id \
     WHERE t.source = ?1";

/// Pick the casing to display for a genre tagged more than one way.
///
/// Prefers a mixed-case spelling ("Nu Metal") over an all-caps or all-lower one
/// ("NU METAL", "nu metal"), falling back to the lexicographically first when
/// every variant is one case — so a genuinely capitalised genre like "EDM"
/// survives. Deterministic either way: the list must not reshuffle between
/// launches.
fn preferred_display(mut candidates: Vec<String>) -> String {
    candidates.sort();
    candidates
        .iter()
        .find(|d| **d != d.to_uppercase() && **d != d.to_lowercase())
        .cloned()
        .unwrap_or_else(|| candidates.into_iter().next().unwrap_or_default())
}

/// Up to [`MOSAIC_TILES`] album ids per genre for the grid tile's mosaic.
///
/// Ordered by track count descending, tie-broken by album id, so a genre's
/// tiles never reshuffle between renders or launches.
fn genre_mosaics(
    conn: &Connection,
    source: &str,
) -> Result<HashMap<String, Vec<String>>, rusqlite::Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT g.genre_key, t.album_jellyfin_id, COUNT(*) \
         FROM track_genres g \
         JOIN cached_tracks t ON t.jellyfin_id = g.track_id \
         WHERE t.source = ?1 AND t.album_jellyfin_id IS NOT NULL \
         GROUP BY g.genre_key, t.album_jellyfin_id",
    )?;
    let rows = stmt
        .query_map([source], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut by_genre: HashMap<String, Vec<(i64, String)>> = HashMap::new();
    for (key, album_id, count) in rows {
        by_genre.entry(key).or_default().push((count, album_id));
    }

    Ok(by_genre
        .into_iter()
        .map(|(key, mut albums)| {
            albums.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            albums.truncate(MOSAIC_TILES);
            (key, albums.into_iter().map(|(_, id)| id).collect())
        })
        .collect())
}

/// Genre key -> the casing to display for it, for one backend.
fn genre_displays(
    conn: &Connection,
    source: &str,
) -> Result<HashMap<String, String>, rusqlite::Error> {
    let mut stmt = conn.prepare_cached(GENRE_CASINGS_SQL)?;
    let rows = stmt
        .query_map([source], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut by_key: HashMap<String, Vec<String>> = HashMap::new();
    for (key, display) in rows {
        by_key.entry(key).or_default().push(display);
    }

    Ok(by_key
        .into_iter()
        .map(|(key, casings)| (key, preferred_display(casings)))
        .collect())
}

/// The library's normalised genre list for one backend, A-Z, case-insensitive.
fn list_genres(conn: &Connection, source: &str) -> Result<Vec<String>, rusqlite::Error> {
    let mut genres: Vec<String> = genre_displays(conn, source)?.into_values().collect();
    genres.sort_by_key(|g| (g.to_lowercase(), g.clone()));
    Ok(genres)
}

/// Replace a track's normalised genre memberships from its raw tag string.
///
/// Delete-then-insert rather than a bare insert: a re-sync must be idempotent
/// (no duplicate memberships) and must also drop a genre the tags no longer
/// carry. `cached_tracks.genre` keeps the raw string as the audit trail.
fn write_track_genres(
    conn: &Connection,
    track_id: &str,
    raw_tag: Option<&str>,
) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM track_genres WHERE track_id = ?1", [track_id])?;

    let Some(raw) = raw_tag.filter(|r| !r.trim().is_empty()) else {
        return Ok(());
    };

    let mut stmt = conn.prepare_cached(
        "INSERT INTO track_genres (track_id, genre_key, genre_display, source)
         VALUES (?1, ?2, ?3, 'tag')
         ON CONFLICT(track_id, genre_key) DO NOTHING",
    )?;
    for genre in crate::genres::split_genres(std::slice::from_ref(&raw.to_string())) {
        stmt.execute(params![track_id, genre.key, genre.display])?;
    }
    Ok(())
}

/// Convert caller-supplied genre names (display casing) into matching keys.
fn genre_keys(values: &[String]) -> Vec<String> {
    values.iter().map(|v| crate::genres::genre_key(v)).collect()
}

/// Convert a database row to a Track model.
fn track_from_row(row: &Row<'_>) -> Result<Track, rusqlite::Error> {
    Ok(Track {
        jellyfin_id: row.get(0)?,
        mbid: row.get(1)?,
        artist_mbid: row.get(2)?,
        release_mbid: row.get(3)?,
        name: row.get(4)?,
        artist_name: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        album_jellyfin_id: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
        album_name: row.get(7)?,
        track_number: row.get::<_, Option<u32>>(8)?.unwrap_or(0),
        disc_number: row.get::<_, Option<u32>>(9)?.unwrap_or(1),
        duration_ms: row.get::<_, Option<i64>>(10)?.unwrap_or(0) as u64,
        genre: row.get(11)?,
        year: row.get(12)?,
        artwork_url: row.get(13)?,
        cached_file_path: row.get(14)?,
        file_size_bytes: row.get::<_, Option<i64>>(15)?.map(|s| s as u64),
        normalization_gain_db: row.get(16)?,
    })
}

/// Internal search function that takes a connection, scoped to `source`.
///
/// The FTS5 tables carry no `source` column (they mirror only id/name); the
/// source filter is applied on the joined base table before `LIMIT 50`, so you
/// get up to 50 active-backend hits rather than a post-filtered truncation.
pub fn search_tracks_internal(
    conn: &Connection,
    query: &str,
    source: &str,
) -> Result<Vec<Track>, rusqlite::Error> {
    // Escape FTS special characters and add prefix matching. If escaping leaves
    // nothing (e.g. a punctuation/emoji-only query), there is nothing to match —
    // return empty rather than issuing a bare "*" that FTS5 rejects.
    let escaped = escape_fts_query(query);
    if escaped.is_empty() {
        return Ok(Vec::new());
    }
    let fts_query = format!("{escaped}*");

    // Column list MUST match `track_from_row`'s indices (jellyfin_id, track_mbid,
    // artist_mbid, release_mbid, name, …) — omitting artist_mbid/release_mbid
    // misaligns every later field and errors on null-album rows.
    let mut stmt = conn.prepare(concat!(
        track_select!(),
        r#"
        JOIN fts_tracks f ON t.jellyfin_id = f.jellyfin_id
        WHERE fts_tracks MATCH ?1 AND t.source = ?2
        ORDER BY rank
        LIMIT 50
        "#
    ))?;

    let tracks = stmt
        .query_map(params![fts_query, source], track_from_row)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(tracks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::CachedAlbum;

    fn test_track() -> CachedTrack {
        CachedTrack {
            jellyfin_id: "track12345678901234567890123456".to_string(),
            track_mbid: Some("mb-track-id".to_string()),
            artist_mbid: Some("mb-artist-id".to_string()),
            release_mbid: Some("mb-release-id".to_string()),
            name: "Test Track".to_string(),
            artist_name: Some("Test Artist".to_string()),
            album_jellyfin_id: Some("album12345678901234567890123456".to_string()),
            track_number: Some(1),
            disc_number: Some(1),
            duration_ms: Some(180000),
            genre: Some("Rock".to_string()),
            artwork_url: Some("http://example.com/art.jpg".to_string()),
            cached_file_path: None,
            file_size_bytes: None,
            normalization_gain_db: Some(-2.5),
            server_play_count: None,
        }
    }

    /// Seed a multi-genre, multi-artist, multi-album dataset for library view tests.
    ///
    /// Creates:
    /// - 3 albums: "Alpha Album" (Artist A, Rock), "Beta Album" (Artist B, Jazz),
    ///   "Gamma Album" (Artist A, Jazz)
    /// - 6 tracks across the albums with varied genres and durations
    /// - 1 track with a cached file path (for offline_only tests)
    fn seed_library_dataset(db: &Database) {
        let albums = [
            CachedAlbum {
                jellyfin_id: "album-alpha-0001".to_string(),
                release_mbid: None,
                name: "Alpha Album".to_string(),
                artist_name: Some("Artist A".to_string()),
                artist_jellyfin_id: Some("artist-a".to_string()),
                year: Some(2020),
                genre: Some("Rock".to_string()),
                artwork_url: None,
                track_count: Some(2),
                duration_ms: Some(360000),
                date_created: None,
            },
            CachedAlbum {
                jellyfin_id: "album-beta-0002".to_string(),
                release_mbid: None,
                name: "Beta Album".to_string(),
                artist_name: Some("Artist B".to_string()),
                artist_jellyfin_id: Some("artist-b".to_string()),
                year: Some(2021),
                genre: Some("Jazz".to_string()),
                artwork_url: None,
                track_count: Some(2),
                duration_ms: Some(500000),
                date_created: None,
            },
            CachedAlbum {
                jellyfin_id: "album-gamma-0003".to_string(),
                release_mbid: None,
                name: "Gamma Album".to_string(),
                artist_name: Some("Artist A".to_string()),
                artist_jellyfin_id: Some("artist-a".to_string()),
                year: Some(2022),
                genre: Some("Jazz".to_string()),
                artwork_url: None,
                track_count: Some(2),
                duration_ms: Some(400000),
                date_created: None,
            },
        ];
        for album in &albums {
            db.upsert_album(album, ServerKind::Jellyfin, None).unwrap();
        }

        let tracks = [
            CachedTrack {
                jellyfin_id: "track-a1".to_string(),
                track_mbid: None,
                artist_mbid: None,
                release_mbid: None,
                name: "Crimson".to_string(),
                artist_name: Some("Artist A".to_string()),
                album_jellyfin_id: Some("album-alpha-0001".to_string()),
                track_number: Some(1),
                disc_number: Some(1),
                duration_ms: Some(200000),
                genre: Some("Rock".to_string()),
                artwork_url: None,
                cached_file_path: Some("/cache/track-a1.flac".to_string()),
                file_size_bytes: Some(10_000_000),
                normalization_gain_db: None,
                server_play_count: None,
            },
            CachedTrack {
                jellyfin_id: "track-a2".to_string(),
                track_mbid: None,
                artist_mbid: None,
                release_mbid: None,
                name: "Daybreak".to_string(),
                artist_name: Some("Artist A".to_string()),
                album_jellyfin_id: Some("album-alpha-0001".to_string()),
                track_number: Some(2),
                disc_number: Some(1),
                duration_ms: Some(160000),
                genre: Some("Rock".to_string()),
                artwork_url: None,
                cached_file_path: None,
                file_size_bytes: None,
                normalization_gain_db: None,
                server_play_count: None,
            },
            CachedTrack {
                jellyfin_id: "track-b1".to_string(),
                track_mbid: None,
                artist_mbid: None,
                release_mbid: None,
                name: "Ember".to_string(),
                artist_name: Some("Artist B".to_string()),
                album_jellyfin_id: Some("album-beta-0002".to_string()),
                track_number: Some(1),
                disc_number: Some(1),
                duration_ms: Some(250000),
                genre: Some("Jazz".to_string()),
                artwork_url: None,
                cached_file_path: None,
                file_size_bytes: None,
                normalization_gain_db: None,
                server_play_count: None,
            },
            CachedTrack {
                jellyfin_id: "track-b2".to_string(),
                track_mbid: None,
                artist_mbid: None,
                release_mbid: None,
                name: "Fable".to_string(),
                artist_name: Some("Artist B".to_string()),
                album_jellyfin_id: Some("album-beta-0002".to_string()),
                track_number: Some(2),
                disc_number: Some(1),
                duration_ms: Some(250000),
                genre: Some("Jazz".to_string()),
                artwork_url: None,
                cached_file_path: None,
                file_size_bytes: None,
                normalization_gain_db: None,
                server_play_count: None,
            },
            CachedTrack {
                jellyfin_id: "track-c1".to_string(),
                track_mbid: None,
                artist_mbid: None,
                release_mbid: None,
                name: "Aurora".to_string(),
                artist_name: Some("Artist A".to_string()),
                album_jellyfin_id: Some("album-gamma-0003".to_string()),
                track_number: Some(1),
                disc_number: Some(1),
                duration_ms: Some(180000),
                genre: Some("Jazz".to_string()),
                artwork_url: None,
                cached_file_path: None,
                file_size_bytes: None,
                normalization_gain_db: None,
                server_play_count: None,
            },
            CachedTrack {
                jellyfin_id: "track-c2".to_string(),
                track_mbid: None,
                artist_mbid: None,
                release_mbid: None,
                name: "Bliss".to_string(),
                artist_name: Some("Artist A".to_string()),
                album_jellyfin_id: Some("album-gamma-0003".to_string()),
                track_number: Some(2),
                disc_number: Some(1),
                duration_ms: Some(220000),
                genre: Some("Jazz".to_string()),
                artwork_url: None,
                cached_file_path: None,
                file_size_bytes: None,
                normalization_gain_db: None,
                server_play_count: None,
            },
        ];
        db.upsert_tracks(&tracks, ServerKind::Jellyfin, None)
            .unwrap();
    }

    // ----------------------------------------------------------------

    /// Album + tracks whose raw genre tags are multi-value strings, as an
    /// unconfigured Jellyfin returns them.
    fn seed_multi_genre_dataset(db: &Database) {
        db.upsert_album(
            &CachedAlbum {
                jellyfin_id: "album-mg-0001".to_string(),
                release_mbid: None,
                name: "Tag Soup".to_string(),
                artist_name: Some("Artist M".to_string()),
                artist_jellyfin_id: Some("artist-m".to_string()),
                year: Some(2020),
                genre: Some("Alternative Metal;Heavy Metal;Rock".to_string()),
                artwork_url: None,
                track_count: Some(3),
                duration_ms: Some(600000),
                date_created: None,
            },
            ServerKind::Jellyfin,
            None,
        )
        .unwrap();

        let mk = |id: &str, name: &str, genre: &str| CachedTrack {
            jellyfin_id: id.to_string(),
            track_mbid: None,
            artist_mbid: None,
            release_mbid: None,
            name: name.to_string(),
            artist_name: Some("Artist M".to_string()),
            album_jellyfin_id: Some("album-mg-0001".to_string()),
            track_number: Some(1),
            disc_number: Some(1),
            duration_ms: Some(200000),
            genre: Some(genre.to_string()),
            artwork_url: None,
            cached_file_path: None,
            file_size_bytes: None,
            normalization_gain_db: None,
            server_play_count: None,
        };

        let tracks = [
            mk(
                "track-mg-1",
                "Combined",
                "Alternative Metal;Heavy Metal;Rock",
            ),
            mk("track-mg-2", "Plain", "Heavy Metal"),
            mk("track-mg-3", "Cased", "nu metal;ROCK"),
            mk("track-mg-4", "Shouty", "Nu Metal"),
        ];
        db.upsert_tracks(&tracks, ServerKind::Jellyfin, None)
            .unwrap();
    }

    fn memberships(db: &Database, track_id: &str) -> Vec<String> {
        db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT genre_key FROM track_genres WHERE track_id = ?1 ORDER BY genre_key",
            )?;
            let rows = stmt
                .query_map([track_id], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap()
    }

    #[test]
    fn an_album_selection_subsumes_the_artist_filter_for_the_track_list() {
        let db = Database::open_in_memory().unwrap();
        db.upsert_album(
            &CachedAlbum {
                jellyfin_id: "album-comp-001".to_string(),
                release_mbid: None,
                name: "Compilation".to_string(),
                // No ALBUMARTIST upstream: the album resolved to one contributor.
                artist_name: Some("Artist One".to_string()),
                artist_jellyfin_id: None,
                year: None,
                genre: None,
                artwork_url: None,
                track_count: Some(2),
                duration_ms: None,
                date_created: None,
            },
            ServerKind::Jellyfin,
            None,
        )
        .unwrap();

        let mk = |id: &str, artist: &str| CachedTrack {
            artist_name: Some(artist.to_string()),
            album_jellyfin_id: Some("album-comp-001".to_string()),
            jellyfin_id: id.to_string(),
            ..test_track()
        };
        db.upsert_tracks(
            &[
                mk("track-comp-1", "Artist One"),
                mk("track-comp-2", "Artist Two"),
            ],
            ServerKind::Jellyfin,
            None,
        )
        .unwrap();

        // Opening the album selects its artist too; the other contributor's
        // track must not vanish from an album the user just opened.
        let params = LibraryViewParams {
            artists: Some(vec!["Artist One".to_string()]),
            album_ids: Some(vec!["album-comp-001".to_string()]),
            ..Default::default()
        };
        assert_eq!(db.query_tracks(&params).unwrap().len(), 2);

        // With no album selected the artist filter still binds.
        let params = LibraryViewParams {
            artists: Some(vec!["Artist One".to_string()]),
            ..Default::default()
        };
        assert_eq!(db.query_tracks(&params).unwrap().len(), 1);
    }

    #[test]
    fn multi_value_tag_becomes_one_membership_per_genre() {
        let db = Database::open_in_memory().unwrap();
        seed_multi_genre_dataset(&db);

        assert_eq!(
            memberships(&db, "track-mg-1"),
            vec!["alternative metal", "heavy metal", "rock"]
        );
    }

    #[test]
    fn genre_filter_returns_tracks_from_combined_strings_too() {
        let db = Database::open_in_memory().unwrap();
        seed_multi_genre_dataset(&db);

        let params = LibraryViewParams {
            genres: Some(vec!["Heavy Metal".to_string()]),
            ..Default::default()
        };
        let mut ids: Vec<String> = db
            .query_tracks(&params)
            .unwrap()
            .into_iter()
            .map(|t| t.jellyfin_id)
            .collect();
        ids.sort();
        // The combined-string track used to be invisible under "Heavy Metal".
        assert_eq!(ids, vec!["track-mg-1", "track-mg-2"]);
    }

    #[test]
    fn a_track_in_several_selected_genres_is_returned_once() {
        let db = Database::open_in_memory().unwrap();
        seed_multi_genre_dataset(&db);

        let params = LibraryViewParams {
            genres: Some(vec![
                "Heavy Metal".to_string(),
                "Rock".to_string(),
                "Alternative Metal".to_string(),
            ]),
            ..Default::default()
        };
        let ids: Vec<String> = db
            .query_tracks(&params)
            .unwrap()
            .into_iter()
            .map(|t| t.jellyfin_id)
            .collect();
        // track-mg-1 is in all three; the StatusBar total must not triple-count it.
        assert_eq!(ids.iter().filter(|id| *id == "track-mg-1").count(), 1);
    }

    #[test]
    fn distinct_genres_lists_normalised_genres_not_tag_strings() {
        let db = Database::open_in_memory().unwrap();
        seed_multi_genre_dataset(&db);

        let genres = db.distinct_genres().unwrap();
        assert_eq!(
            genres,
            vec!["Alternative Metal", "Heavy Metal", "Nu Metal", "Rock"]
        );
        assert!(
            !genres.iter().any(|g| g.contains(';')),
            "no raw tag string may survive as a genre"
        );
    }

    #[test]
    fn casing_variants_collapse_to_one_genre() {
        let db = Database::open_in_memory().unwrap();
        seed_multi_genre_dataset(&db);

        // "Rock" and "ROCK" are tagged on different tracks.
        let rock = db
            .distinct_genres()
            .unwrap()
            .into_iter()
            .filter(|g| g.eq_ignore_ascii_case("rock"))
            .count();
        assert_eq!(rock, 1);

        let params = LibraryViewParams {
            genres: Some(vec!["rock".to_string()]),
            ..Default::default()
        };
        let ids: Vec<String> = db
            .query_tracks(&params)
            .unwrap()
            .into_iter()
            .map(|t| t.jellyfin_id)
            .collect();
        assert_eq!(ids.len(), 2, "case-folded key must match both casings");
    }

    #[test]
    fn column_browser_genre_column_matches_the_genres_view() {
        let db = Database::open_in_memory().unwrap();
        seed_multi_genre_dataset(&db);

        let view = db
            .query_library_view(&LibraryViewParams::default())
            .unwrap();
        assert_eq!(view.column_browser.genres, db.distinct_genres().unwrap());
    }

    #[test]
    fn re_syncing_is_idempotent_and_reflects_retagging() {
        let db = Database::open_in_memory().unwrap();
        seed_multi_genre_dataset(&db);
        seed_multi_genre_dataset(&db);

        assert_eq!(
            memberships(&db, "track-mg-1"),
            vec!["alternative metal", "heavy metal", "rock"],
            "a re-sync must not duplicate memberships"
        );

        // Re-tagged upstream: the dropped genres must go with it.
        let mut retagged = CachedTrack {
            genre: Some("Rock".to_string()),
            ..{
                let mut t = test_track();
                t.jellyfin_id = "track-mg-1".to_string();
                t.album_jellyfin_id = Some("album-mg-0001".to_string());
                t
            }
        };
        retagged.name = "Combined".to_string();
        db.upsert_tracks(&[retagged], ServerKind::Jellyfin, None)
            .unwrap();
        assert_eq!(memberships(&db, "track-mg-1"), vec!["rock"]);
    }

    #[test]
    fn commas_and_slashes_are_not_split() {
        let db = Database::open_in_memory().unwrap();
        db.upsert_album(
            &CachedAlbum {
                jellyfin_id: "album-punct-01".to_string(),
                release_mbid: None,
                name: "Punctuation".to_string(),
                artist_name: Some("Artist P".to_string()),
                artist_jellyfin_id: None,
                year: None,
                genre: None,
                artwork_url: None,
                track_count: Some(1),
                duration_ms: None,
                date_created: None,
            },
            ServerKind::Jellyfin,
            None,
        )
        .unwrap();

        let mut track = test_track();
        track.jellyfin_id = "track-punct-1".to_string();
        track.album_jellyfin_id = Some("album-punct-01".to_string());
        track.genre = Some("Folk, World, & Country".to_string());
        db.upsert_tracks(&[track], ServerKind::Jellyfin, None)
            .unwrap();

        assert_eq!(
            db.distinct_genres().unwrap(),
            vec!["Folk, World, & Country"]
        );
    }

    #[test]
    fn test_upsert_and_get_track() {
        let db = Database::open_in_memory().unwrap();

        // First create the album (for foreign key constraint)
        let album = CachedAlbum {
            jellyfin_id: "album12345678901234567890123456".to_string(),
            release_mbid: None,
            name: "Test Album".to_string(),
            artist_name: Some("Test Artist".to_string()),
            artist_jellyfin_id: None,
            year: Some(2024),
            genre: None,
            artwork_url: None,
            track_count: Some(1),
            duration_ms: None,
            date_created: None,
        };
        db.upsert_album(&album, ServerKind::Jellyfin, None).unwrap();

        let track = test_track();
        db.upsert_tracks(std::slice::from_ref(&track), ServerKind::Jellyfin, None)
            .unwrap();

        let retrieved = db.get_track(&track.jellyfin_id).unwrap().unwrap();
        assert_eq!(retrieved.name, "Test Track");
        assert_eq!(retrieved.artist_name, "Test Artist");
        assert_eq!(retrieved.track_number, 1);
    }

    #[test]
    fn test_get_tracks_by_album() {
        let db = Database::open_in_memory().unwrap();

        // First create an album
        let album = CachedAlbum {
            jellyfin_id: "album12345678901234567890123456".to_string(),
            release_mbid: None,
            name: "Test Album".to_string(),
            artist_name: Some("Test Artist".to_string()),
            artist_jellyfin_id: None,
            year: Some(2024),
            genre: None,
            artwork_url: None,
            track_count: Some(2),
            duration_ms: None,
            date_created: None,
        };
        db.upsert_album(&album, ServerKind::Jellyfin, None).unwrap();

        // Add tracks
        let track1 = CachedTrack {
            jellyfin_id: "track1111111111111111111111111111".to_string(),
            track_number: Some(1),
            ..test_track()
        };
        let track2 = CachedTrack {
            jellyfin_id: "track2222222222222222222222222222".to_string(),
            track_number: Some(2),
            ..test_track()
        };

        db.upsert_tracks(&[track1, track2], ServerKind::Jellyfin, None)
            .unwrap();

        let tracks = db
            .get_tracks_by_album("album12345678901234567890123456")
            .unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].track_number, 1);
        assert_eq!(tracks[1].track_number, 2);
    }

    #[test]
    fn test_cached_file_path() {
        let db = Database::open_in_memory().unwrap();

        // First create the album (for foreign key constraint)
        let album = CachedAlbum {
            jellyfin_id: "album12345678901234567890123456".to_string(),
            release_mbid: None,
            name: "Test Album".to_string(),
            artist_name: Some("Test Artist".to_string()),
            artist_jellyfin_id: None,
            year: Some(2024),
            genre: None,
            artwork_url: None,
            track_count: Some(1),
            duration_ms: None,
            date_created: None,
        };
        db.upsert_album(&album, ServerKind::Jellyfin, None).unwrap();

        let track = test_track();
        db.upsert_tracks(std::slice::from_ref(&track), ServerKind::Jellyfin, None)
            .unwrap();

        // Initially no cached path
        let retrieved = db.get_track(&track.jellyfin_id).unwrap().unwrap();
        assert!(retrieved.cached_file_path.is_none());

        // Set cached path
        db.set_track_cached_path(&track.jellyfin_id, "/path/to/file.flac", 50_000_000)
            .unwrap();

        let retrieved = db.get_track(&track.jellyfin_id).unwrap().unwrap();
        assert_eq!(
            retrieved.cached_file_path.as_deref(),
            Some("/path/to/file.flac")
        );

        // Clear cached path
        db.clear_track_cached_path(&track.jellyfin_id).unwrap();

        let retrieved = db.get_track(&track.jellyfin_id).unwrap().unwrap();
        assert!(retrieved.cached_file_path.is_none());
    }

    // ========================================================================
    // Library view query tests
    // ========================================================================

    #[test]
    fn test_query_tracks_no_filters() {
        let db = Database::open_in_memory().unwrap();
        seed_library_dataset(&db);

        let params = LibraryViewParams::default();
        let tracks = db.query_tracks(&params).unwrap();

        assert_eq!(tracks.len(), 6, "should return all 6 tracks");
    }

    #[test]
    fn test_query_tracks_genre_filter() {
        let db = Database::open_in_memory().unwrap();
        seed_library_dataset(&db);

        let params = LibraryViewParams {
            genres: Some(vec!["Rock".to_string()]),
            ..Default::default()
        };
        let tracks = db.query_tracks(&params).unwrap();

        assert_eq!(tracks.len(), 2, "only Rock tracks");
        assert!(tracks.iter().all(|t| t.genre.as_deref() == Some("Rock")));
    }

    #[test]
    fn test_query_tracks_multi_genre_filter() {
        let db = Database::open_in_memory().unwrap();
        seed_library_dataset(&db);

        let params = LibraryViewParams {
            genres: Some(vec!["Rock".to_string(), "Jazz".to_string()]),
            ..Default::default()
        };
        let tracks = db.query_tracks(&params).unwrap();

        assert_eq!(tracks.len(), 6, "Rock + Jazz = all tracks");
    }

    #[test]
    fn test_query_tracks_genre_artist_filter() {
        let db = Database::open_in_memory().unwrap();
        seed_library_dataset(&db);

        // Jazz tracks by Artist A only (should exclude Artist B's Jazz tracks)
        let params = LibraryViewParams {
            genres: Some(vec!["Jazz".to_string()]),
            artists: Some(vec!["Artist A".to_string()]),
            ..Default::default()
        };
        let tracks = db.query_tracks(&params).unwrap();

        assert_eq!(tracks.len(), 2, "Jazz AND Artist A = Aurora + Bliss");
        assert!(tracks.iter().all(|t| t.artist_name == "Artist A"));
        assert!(tracks.iter().all(|t| t.genre.as_deref() == Some("Jazz")));
    }

    #[test]
    fn test_query_tracks_album_filter() {
        let db = Database::open_in_memory().unwrap();
        seed_library_dataset(&db);

        let params = LibraryViewParams {
            album_ids: Some(vec!["album-beta-0002".to_string()]),
            ..Default::default()
        };
        let tracks = db.query_tracks(&params).unwrap();

        assert_eq!(tracks.len(), 2, "Beta Album has 2 tracks");
        assert!(tracks
            .iter()
            .all(|t| t.album_jellyfin_id == "album-beta-0002"));
    }

    #[test]
    fn test_query_tracks_offline_only() {
        let db = Database::open_in_memory().unwrap();
        seed_library_dataset(&db);

        let params = LibraryViewParams {
            offline_only: Some(true),
            ..Default::default()
        };
        let tracks = db.query_tracks(&params).unwrap();

        assert_eq!(tracks.len(), 1, "only 1 track has cached_file_path");
        assert_eq!(tracks[0].jellyfin_id, "track-a1");
        assert!(tracks[0].cached_file_path.is_some());
    }

    #[test]
    fn test_query_tracks_sort_name_asc() {
        let db = Database::open_in_memory().unwrap();
        seed_library_dataset(&db);

        let params = LibraryViewParams {
            sort: Some("name-asc".to_string()),
            ..Default::default()
        };
        let tracks = db.query_tracks(&params).unwrap();

        let names: Vec<&str> = tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Aurora", "Bliss", "Crimson", "Daybreak", "Ember", "Fable"]
        );
    }

    #[test]
    fn test_query_tracks_sort_duration_desc() {
        let db = Database::open_in_memory().unwrap();
        seed_library_dataset(&db);

        let params = LibraryViewParams {
            sort: Some("duration-desc".to_string()),
            ..Default::default()
        };
        let tracks = db.query_tracks(&params).unwrap();

        let durations: Vec<u64> = tracks.iter().map(|t| t.duration_ms).collect();
        // Verify descending order
        for window in durations.windows(2) {
            assert!(
                window[0] >= window[1],
                "expected descending: {} >= {}",
                window[0],
                window[1]
            );
        }
    }

    /// Sorting the Songs table by Artist ignores a leading "The"/"A"/"An" and
    /// sorts embedded numbers naturally, matching Apple Music/Spotify.
    #[test]
    fn test_query_tracks_sort_artist_asc_strips_article_and_sorts_naturally() {
        let db = Database::open_in_memory().unwrap();

        let album = CachedAlbum {
            name: "Compilation".to_string(),
            ..test_album_for_tracks()
        };
        db.upsert_album(&album, ServerKind::Jellyfin, None).unwrap();

        let tracks = [
            ("t-air", "Air", "Air Track"),
            ("t-beatles", "The Beatles", "Beatles Track"),
            ("t-blink10", "Blink-10", "Blink 10 Track"),
            ("t-blink2", "Blink-2", "Blink 2 Track"),
        ]
        .map(|(id, artist, name)| CachedTrack {
            jellyfin_id: id.to_string(),
            artist_name: Some(artist.to_string()),
            name: name.to_string(),
            album_jellyfin_id: Some(album.jellyfin_id.clone()),
            ..test_track_for_tracks()
        });

        db.upsert_tracks(&tracks, ServerKind::Jellyfin, None)
            .unwrap();

        let params = LibraryViewParams {
            sort: Some("artist-asc".to_string()),
            ..Default::default()
        };
        let result = db.query_tracks(&params).unwrap();
        let artists: Vec<&str> = result.iter().map(|t| t.artist_name.as_str()).collect();
        // "The Beatles" sorts as "Beatles" (article stripped) — right after
        // "Air" and before "Blink-*" — and "Blink-2"/"Blink-10" sort by the
        // number's value (natural), not lexicographically ("Blink-10" would
        // otherwise sort before "Blink-2").
        assert_eq!(artists, vec!["Air", "The Beatles", "Blink-2", "Blink-10"]);
    }

    fn test_album_for_tracks() -> CachedAlbum {
        CachedAlbum {
            jellyfin_id: "album-for-track-sort-tests-000001".to_string(),
            release_mbid: None,
            name: "Test Album".to_string(),
            artist_name: Some("Test Artist".to_string()),
            artist_jellyfin_id: Some("artist-for-track-sort-tests".to_string()),
            year: Some(2020),
            genre: None,
            artwork_url: None,
            track_count: Some(4),
            duration_ms: Some(600000),
            date_created: None,
        }
    }

    fn test_track_for_tracks() -> CachedTrack {
        CachedTrack {
            track_number: Some(1),
            disc_number: Some(1),
            duration_ms: Some(180000),
            ..test_track()
        }
    }

    /// Two artists whose Jellyfin sort_name reverses their raw-name order:
    /// "Zzz Artist" sorts FIRST via its explicit sort_name "Aaa Artist", while
    /// "Mmm Artist" (no sort_name) sorts by its own name. Honouring sort_name
    /// puts "Zzz Artist" before "Mmm Artist"; falling back to the
    /// denormalised artist_name (article-stripping only, the pre-fix
    /// behaviour) would reverse that.
    fn seed_artist_sort_name_reversal(db: &Database) {
        use crate::db::CachedArtist;

        let artist_x = CachedArtist {
            jellyfin_id: "artist-x".to_string(),
            artist_mbid: None,
            name: "Zzz Artist".to_string(),
            artwork_url: None,
            backdrop_url: None,
            album_count: Some(1),
            sort_name: Some("Aaa Artist".to_string()),
        };
        let artist_y = CachedArtist {
            jellyfin_id: "artist-y".to_string(),
            artist_mbid: None,
            name: "Mmm Artist".to_string(),
            artwork_url: None,
            backdrop_url: None,
            album_count: Some(1),
            sort_name: None,
        };
        db.upsert_artists(&[artist_x, artist_y], ServerKind::Jellyfin, None)
            .unwrap();

        let album_x = CachedAlbum {
            jellyfin_id: "album-x".to_string(),
            release_mbid: None,
            name: "Album X".to_string(),
            artist_name: Some("Zzz Artist".to_string()),
            artist_jellyfin_id: Some("artist-x".to_string()),
            year: Some(2020),
            genre: None,
            artwork_url: None,
            track_count: Some(1),
            duration_ms: Some(180000),
            date_created: None,
        };
        let album_y = CachedAlbum {
            jellyfin_id: "album-y".to_string(),
            release_mbid: None,
            name: "Album Y".to_string(),
            artist_name: Some("Mmm Artist".to_string()),
            artist_jellyfin_id: Some("artist-y".to_string()),
            year: Some(2020),
            genre: None,
            artwork_url: None,
            track_count: Some(1),
            duration_ms: Some(180000),
            date_created: None,
        };
        db.upsert_albums(&[album_x, album_y], ServerKind::Jellyfin, None)
            .unwrap();

        let track_x = CachedTrack {
            jellyfin_id: "track-x".to_string(),
            artist_name: Some("Zzz Artist".to_string()),
            name: "Track X".to_string(),
            album_jellyfin_id: Some("album-x".to_string()),
            genre: Some("Sort Name Test Genre".to_string()),
            ..test_track_for_tracks()
        };
        let track_y = CachedTrack {
            jellyfin_id: "track-y".to_string(),
            artist_name: Some("Mmm Artist".to_string()),
            name: "Track Y".to_string(),
            album_jellyfin_id: Some("album-y".to_string()),
            genre: Some("Sort Name Test Genre".to_string()),
            ..test_track_for_tracks()
        };
        db.upsert_tracks(&[track_x, track_y], ServerKind::Jellyfin, None)
            .unwrap();
    }

    #[test]
    fn test_get_all_tracks_orders_artists_by_sort_name() {
        let db = Database::open_in_memory().unwrap();
        seed_artist_sort_name_reversal(&db);

        let tracks = db.get_all_tracks().unwrap();
        let names: Vec<&str> = tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["Track X", "Track Y"]);
    }

    #[test]
    fn test_query_tracks_sort_artist_asc_orders_by_sort_name() {
        let db = Database::open_in_memory().unwrap();
        seed_artist_sort_name_reversal(&db);

        let params = LibraryViewParams {
            sort: Some("artist-asc".to_string()),
            ..Default::default()
        };
        let result = db.query_tracks(&params).unwrap();
        let names: Vec<&str> = result.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["Track X", "Track Y"]);
    }

    #[test]
    fn test_distinct_artists_orders_by_sort_name() {
        let db = Database::open_in_memory().unwrap();
        seed_artist_sort_name_reversal(&db);

        let artists = db.distinct_artists().unwrap();
        assert_eq!(artists, vec!["Zzz Artist", "Mmm Artist"]);
    }

    #[test]
    fn test_query_library_view_artists_orders_by_sort_name() {
        let db = Database::open_in_memory().unwrap();
        seed_artist_sort_name_reversal(&db);

        let result = db
            .query_library_view(&LibraryViewParams::default())
            .unwrap();
        assert_eq!(
            result.column_browser.artists,
            vec!["Zzz Artist", "Mmm Artist"]
        );
    }

    #[test]
    fn test_query_library_view_artists_with_genre_filter_orders_by_sort_name() {
        let db = Database::open_in_memory().unwrap();
        seed_artist_sort_name_reversal(&db);

        let params = LibraryViewParams {
            genres: Some(vec!["Sort Name Test Genre".to_string()]),
            ..Default::default()
        };
        let result = db.query_library_view(&params).unwrap();
        assert_eq!(
            result.column_browser.artists,
            vec!["Zzz Artist", "Mmm Artist"]
        );
    }

    #[test]
    fn test_query_library_view_genres_always_unfiltered() {
        let db = Database::open_in_memory().unwrap();
        seed_library_dataset(&db);

        // Even when filtering by a specific genre, the genre list should be unfiltered
        let params = LibraryViewParams {
            genres: Some(vec!["Rock".to_string()]),
            ..Default::default()
        };
        let result = db.query_library_view(&params).unwrap();

        assert_eq!(
            result.column_browser.genres,
            vec!["Jazz", "Rock"],
            "genres should always show all distinct genres"
        );
    }

    #[test]
    fn test_query_library_view_artist_cascade() {
        let db = Database::open_in_memory().unwrap();
        seed_library_dataset(&db);

        // When filtering by Rock genre, only Artist A should appear
        // (Artist B only has Jazz tracks)
        let params = LibraryViewParams {
            genres: Some(vec!["Rock".to_string()]),
            ..Default::default()
        };
        let result = db.query_library_view(&params).unwrap();

        assert_eq!(
            result.column_browser.artists,
            vec!["Artist A"],
            "only Artist A has Rock tracks"
        );

        // When filtering by Jazz, both artists should appear
        let params = LibraryViewParams {
            genres: Some(vec!["Jazz".to_string()]),
            ..Default::default()
        };
        let result = db.query_library_view(&params).unwrap();

        assert_eq!(
            result.column_browser.artists,
            vec!["Artist A", "Artist B"],
            "both artists have Jazz tracks"
        );
    }

    #[test]
    fn test_query_library_view_album_cascade() {
        let db = Database::open_in_memory().unwrap();
        seed_library_dataset(&db);

        // Filter by Jazz + Artist A: should only show Gamma Album
        let params = LibraryViewParams {
            genres: Some(vec!["Jazz".to_string()]),
            artists: Some(vec!["Artist A".to_string()]),
            ..Default::default()
        };
        let result = db.query_library_view(&params).unwrap();

        assert_eq!(result.column_browser.albums.len(), 1);
        assert_eq!(result.column_browser.albums[0].name, "Gamma Album");
    }

    #[test]
    fn test_query_library_view_total_track_count() {
        let db = Database::open_in_memory().unwrap();
        seed_library_dataset(&db);

        // Total count should always be unfiltered
        let params = LibraryViewParams {
            genres: Some(vec!["Rock".to_string()]),
            ..Default::default()
        };
        let result = db.query_library_view(&params).unwrap();

        assert_eq!(
            result.column_browser.total_track_count, 6,
            "total_track_count should be unfiltered"
        );
    }

    #[test]
    fn test_schema_v5_indexes_exist() {
        let db = Database::open_in_memory().unwrap();

        // Verify both covering indexes were created
        let indexes: Vec<String> = db
            .with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT name FROM sqlite_master WHERE type='index' AND name LIKE 'idx_tracks_%'",
                )?;
                let rows = stmt.query_map([], |row| row.get(0))?;
                rows.collect::<Result<Vec<_>, _>>()
            })
            .unwrap();

        // idx_tracks_genre_artist_album is deliberately NOT here: nothing reads
        // cached_tracks.genre any more (genre filtering goes through the
        // track_genres join table), so v16 drops it rather than keep
        // maintaining it on every upsert.
        assert!(
            !indexes.contains(&"idx_tracks_genre_artist_album".to_string()),
            "the dead genre index should have been dropped, found: {:?}",
            indexes
        );
        assert!(
            indexes.contains(&"idx_tracks_artist_album".to_string()),
            "idx_tracks_artist_album should exist, found: {:?}",
            indexes
        );
    }

    /// Dual-backend sandbox: rows written under one active source are
    /// invisible to reads under the other, and a per-source stale delete never
    /// touches the other backend's rows.
    #[test]
    fn source_sandboxing_isolates_backends() {
        let db = Database::open_in_memory().unwrap();

        // album_jellyfin_id = None avoids the cached_albums FK for this test.
        let mk = |id: &str, name: &str| CachedTrack {
            jellyfin_id: id.to_string(),
            track_mbid: None,
            artist_mbid: None,
            release_mbid: None,
            name: name.to_string(),
            artist_name: Some("Artist".to_string()),
            album_jellyfin_id: None,
            track_number: Some(1),
            disc_number: Some(1),
            duration_ms: Some(1000),
            genre: Some("Rock".to_string()),
            artwork_url: None,
            cached_file_path: None,
            file_size_bytes: None,
            normalization_gain_db: None,
            server_play_count: None,
        };

        // Default active source = Jellyfin.
        db.upsert_tracks(
            std::slice::from_ref(&mk("jf-1", "Jelly Song")),
            ServerKind::Jellyfin,
            None,
        )
        .unwrap();

        // Switch to OpenSubsonic and seed a different track.
        db.set_active_source(ServerKind::OpenSubsonic);
        db.upsert_tracks(
            std::slice::from_ref(&mk("ss-1", "Sonic Song")),
            ServerKind::OpenSubsonic,
            None,
        )
        .unwrap();

        // Active = OpenSubsonic: only the OpenSubsonic row is visible.
        let ss = db.get_all_tracks().unwrap();
        assert_eq!(ss.len(), 1);
        assert_eq!(ss[0].jellyfin_id, "ss-1");
        assert!(
            db.get_track("jf-1").unwrap().is_none(),
            "jellyfin row hidden while opensubsonic is active"
        );
        assert_eq!(
            db.search_tracks("Song").unwrap().len(),
            1,
            "search is scoped to the active backend"
        );

        // Switch back to Jellyfin: only the Jellyfin row is visible.
        db.set_active_source(ServerKind::Jellyfin);
        let jf = db.get_all_tracks().unwrap();
        assert_eq!(jf.len(), 1);
        assert_eq!(jf[0].jellyfin_id, "jf-1");

        // A per-source stale delete of Jellyfin must NOT touch the OpenSubsonic row.
        let removed = db.delete_stale_tracks(i64::MAX, "jellyfin").unwrap();
        assert_eq!(removed, 1, "only the jellyfin row is stale-deleted");
        db.set_active_source(ServerKind::OpenSubsonic);
        assert_eq!(
            db.get_all_tracks().unwrap().len(),
            1,
            "opensubsonic row survives a jellyfin-scoped stale delete"
        );
    }

    /// Mid-sync safety: an upsert stamps the SOURCE PASSED IN, not the live
    /// active source. A Jellyfin sync that writes while the user has toggled the
    /// active backend to OpenSubsonic must still land its rows under Jellyfin, so
    /// a later OpenSubsonic stale-delete can never remove them (the data-loss path).
    #[test]
    fn upsert_stamps_passed_source_not_active() {
        let db = Database::open_in_memory().unwrap();
        let track = CachedTrack {
            jellyfin_id: "jf-row".to_string(),
            track_mbid: None,
            artist_mbid: None,
            release_mbid: None,
            name: "Sync Row".to_string(),
            artist_name: Some("Artist".to_string()),
            album_jellyfin_id: None,
            track_number: Some(1),
            disc_number: Some(1),
            duration_ms: Some(1000),
            genre: None,
            artwork_url: None,
            cached_file_path: None,
            file_size_bytes: None,
            normalization_gain_db: None,
            server_play_count: None,
        };

        // Active backend is OpenSubsonic, but a Jellyfin sync writes source=Jellyfin.
        db.set_active_source(ServerKind::OpenSubsonic);
        db.upsert_tracks(
            std::slice::from_ref(&track),
            ServerKind::Jellyfin,
            Some(1000),
        )
        .unwrap();

        // Invisible while OpenSubsonic is active…
        assert!(
            db.get_track("jf-row").unwrap().is_none(),
            "a Jellyfin-stamped row must not appear under the OpenSubsonic view"
        );
        // …and an OpenSubsonic-scoped stale delete must NOT touch it.
        let removed = db.delete_stale_tracks(i64::MAX, "opensubsonic").unwrap();
        assert_eq!(
            removed, 0,
            "opensubsonic stale-delete must not remove a jellyfin row"
        );

        // Visible (and intact) under Jellyfin.
        db.set_active_source(ServerKind::Jellyfin);
        assert!(
            db.get_track("jf-row").unwrap().is_some(),
            "the row lands under Jellyfin (the source passed), not the active backend"
        );
    }

    /// `get_server_play_counts` persists via upsert and reads back scoped
    /// to the active source — a Subsonic row's count is invisible under Jellyfin
    /// and vice-versa; a zero/None count is omitted (frontend falls back).
    #[test]
    fn get_server_play_counts_persists_and_scopes_by_source() {
        let db = Database::open_in_memory().unwrap();

        let mk = |id: &str, count: Option<u32>| CachedTrack {
            jellyfin_id: id.to_string(),
            track_mbid: None,
            artist_mbid: None,
            release_mbid: None,
            name: "Song".to_string(),
            artist_name: Some("Artist".to_string()),
            album_jellyfin_id: None,
            track_number: Some(1),
            disc_number: Some(1),
            duration_ms: Some(1000),
            genre: None,
            artwork_url: None,
            cached_file_path: None,
            file_size_bytes: None,
            normalization_gain_db: None,
            server_play_count: count,
        };

        // Jellyfin row with a count, plus a zero-count row (must be omitted).
        db.upsert_tracks(
            std::slice::from_ref(&mk("jf-1", Some(9))),
            ServerKind::Jellyfin,
            None,
        )
        .unwrap();
        db.upsert_tracks(
            std::slice::from_ref(&mk("jf-z", Some(0))),
            ServerKind::Jellyfin,
            None,
        )
        .unwrap();
        // OpenSubsonic row with a different count.
        db.upsert_tracks(
            std::slice::from_ref(&mk("ss-1", Some(4))),
            ServerKind::OpenSubsonic,
            None,
        )
        .unwrap();

        // Active = Jellyfin: only the Jellyfin non-zero count is returned.
        let jf = db.get_server_play_counts().unwrap();
        assert_eq!(jf.get("jf-1"), Some(&9));
        assert_eq!(jf.get("jf-z"), None, "zero counts are omitted");
        assert_eq!(
            jf.get("ss-1"),
            None,
            "opensubsonic row hidden under jellyfin"
        );

        // Active = OpenSubsonic: only the Subsonic count is visible.
        db.set_active_source(ServerKind::OpenSubsonic);
        let ss = db.get_server_play_counts().unwrap();
        assert_eq!(ss.get("ss-1"), Some(&4));
        assert_eq!(
            ss.get("jf-1"),
            None,
            "jellyfin row hidden under opensubsonic"
        );

        // A later upsert without a count (None) must PRESERVE the stored value
        // (COALESCE), not wipe it — e.g. the download path re-upserting a row.
        db.upsert_tracks(
            std::slice::from_ref(&mk("ss-1", None)),
            ServerKind::OpenSubsonic,
            None,
        )
        .unwrap();
        let ss_after = db.get_server_play_counts().unwrap();
        assert_eq!(
            ss_after.get("ss-1"),
            Some(&4),
            "a None upsert must not erase the captured server play count"
        );
    }

    /// Re-resolves a held scrobble/skip queued with no usable Jellyfin id
    /// by matching what it DOES carry — artist, album, title — case
    /// insensitively, against the library.
    #[test]
    fn find_track_by_metadata_matches_case_insensitively() {
        let db = Database::open_in_memory().unwrap();
        let album = CachedAlbum {
            jellyfin_id: "album-hard-off".to_string(),
            release_mbid: None,
            name: "Hard-Off".to_string(),
            artist_name: Some("Bloodhound Gang".to_string()),
            artist_jellyfin_id: None,
            year: Some(1999),
            genre: None,
            artwork_url: None,
            track_count: Some(1),
            duration_ms: None,
            date_created: None,
        };
        db.upsert_album(&album, ServerKind::Jellyfin, None).unwrap();

        let track = CachedTrack {
            jellyfin_id: "45751bb32ad69b7b887dc7500a6653b7".to_string(),
            track_mbid: Some("ee959f12-e731-43c0-8a6f-5859c7238587".to_string()),
            artist_mbid: None,
            release_mbid: None,
            name: "Dimes".to_string(),
            artist_name: Some("Bloodhound Gang".to_string()),
            album_jellyfin_id: Some("album-hard-off".to_string()),
            track_number: Some(1),
            disc_number: Some(1),
            duration_ms: Some(200_000),
            genre: None,
            artwork_url: None,
            cached_file_path: None,
            file_size_bytes: None,
            normalization_gain_db: None,
            server_play_count: None,
        };
        db.upsert_tracks(std::slice::from_ref(&track), ServerKind::Jellyfin, None)
            .unwrap();

        let found = db
            .find_track_by_metadata("bloodhound gang", Some("hard-off"), "dimes")
            .unwrap()
            .expect("a case-differing match must still be found");
        assert_eq!(found.jellyfin_id, "45751bb32ad69b7b887dc7500a6653b7");
        assert_eq!(
            found.mbid,
            Some("ee959f12-e731-43c0-8a6f-5859c7238587".to_string())
        );

        // Without an album hint, artist + title alone must still match.
        let found_no_album = db
            .find_track_by_metadata("Bloodhound Gang", None, "Dimes")
            .unwrap();
        assert!(found_no_album.is_some(), "artist + title alone must match");

        assert!(
            db.find_track_by_metadata("Nobody", None, "Nothing")
                .unwrap()
                .is_none(),
            "no match must return None, never a wrong track"
        );
    }
}
