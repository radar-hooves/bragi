//! Track stats persistence.
//!
//! Caches the scrobble server's listening stats (play count, skip count, timestamps)
//! in SQLite for offline access.

use std::collections::HashMap;

use crate::db::{Database, DatabaseError};

/// A cached track stats entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedTrackStats {
    pub play_count: u32,
    pub skip_count: u32,
    /// Unix seconds, 0 if never played.
    pub last_played_at: i64,
    /// Unix seconds, 0 if never skipped.
    pub last_skipped_at: i64,
}

impl Database {
    /// Upsert track stats from the scrobble server.
    ///
    /// Uses MAX(existing, new) for counts to avoid regression from
    /// optimistic local increments that the scrobble server hasn't processed yet.
    pub fn upsert_track_stats(
        &self,
        stats: &HashMap<String, CachedTrackStats>,
    ) -> Result<(), DatabaseError> {
        if stats.is_empty() {
            return Ok(());
        }

        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "INSERT INTO track_stats (track_mbid, play_count, skip_count, last_played_at, last_skipped_at, fetched_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, strftime('%s', 'now'))
                 ON CONFLICT(track_mbid) DO UPDATE SET
                     play_count = MAX(track_stats.play_count, excluded.play_count),
                     skip_count = MAX(track_stats.skip_count, excluded.skip_count),
                     last_played_at = MAX(track_stats.last_played_at, excluded.last_played_at),
                     last_skipped_at = MAX(track_stats.last_skipped_at, excluded.last_skipped_at),
                     fetched_at = excluded.fetched_at",
            )?;

            for (mbid, entry) in stats {
                stmt.execute(rusqlite::params![
                    mbid,
                    entry.play_count,
                    entry.skip_count,
                    entry.last_played_at,
                    entry.last_skipped_at,
                ])?;
            }
            Ok(())
        })
    }

    /// Read all cached track stats.
    pub fn get_all_track_stats(&self) -> Result<HashMap<String, CachedTrackStats>, DatabaseError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT track_mbid, play_count, skip_count, last_played_at, last_skipped_at FROM track_stats",
            )?;

            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    CachedTrackStats {
                        play_count: row.get::<_, u32>(1)?,
                        skip_count: row.get::<_, u32>(2)?,
                        last_played_at: row.get::<_, i64>(3).unwrap_or(0),
                        last_skipped_at: row.get::<_, i64>(4).unwrap_or(0),
                    },
                ))
            })?;

            let mut map = HashMap::new();
            for row in rows {
                let (mbid, stats) = row?;
                map.insert(mbid, stats);
            }
            Ok(map)
        })
    }

    /// Increment play count for a single track (optimistic local update).
    #[allow(dead_code)] // Optimistic local count, never wired to the scrobble path since dd12a8e
    pub fn increment_play_count(
        &self,
        track_mbid: &str,
        played_at: i64,
    ) -> Result<(), DatabaseError> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO track_stats (track_mbid, play_count, skip_count, last_played_at, last_skipped_at, fetched_at)
                 VALUES (?1, 1, 0, ?2, 0, strftime('%s', 'now'))
                 ON CONFLICT(track_mbid) DO UPDATE SET
                     play_count = track_stats.play_count + 1,
                     last_played_at = MAX(track_stats.last_played_at, ?2),
                     fetched_at = strftime('%s', 'now')",
                rusqlite::params![track_mbid, played_at],
            )?;
            Ok(())
        })
    }

    /// Increment skip count for a single track (optimistic local update).
    #[allow(dead_code)] // As increment_play_count: never wired to the skip path
    pub fn increment_skip_count(
        &self,
        track_mbid: &str,
        skipped_at: i64,
    ) -> Result<(), DatabaseError> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO track_stats (track_mbid, play_count, skip_count, last_played_at, last_skipped_at, fetched_at)
                 VALUES (?1, 0, 1, 0, ?2, strftime('%s', 'now'))
                 ON CONFLICT(track_mbid) DO UPDATE SET
                     skip_count = track_stats.skip_count + 1,
                     last_skipped_at = MAX(track_stats.last_skipped_at, ?2),
                     fetched_at = strftime('%s', 'now')",
                rusqlite::params![track_mbid, skipped_at],
            )?;
            Ok(())
        })
    }

    /// Get all distinct track MBIDs from the active backend's cached library.
    ///
    /// Scoped to the active source so the scrobble-server stats prefetch only
    /// fetches stats for the library currently in view. `track_stats` itself
    /// stays backend-agnostic (keyed by canonical recording MBID).
    pub fn get_all_track_mbids(&self) -> Result<Vec<String>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT DISTINCT track_mbid FROM cached_tracks \
                 WHERE source = ?1 AND track_mbid IS NOT NULL",
            )?;
            let mbids = stmt
                .query_map([source], |row| row.get(0))?
                .collect::<Result<Vec<String>, _>>()?;
            Ok(mbids)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upsert_and_read_stats() {
        let db = Database::open_in_memory().unwrap();

        let mut stats = HashMap::new();
        stats.insert(
            "mbid-1".to_string(),
            CachedTrackStats {
                play_count: 5,
                skip_count: 2,
                last_played_at: 1000,
                last_skipped_at: 900,
            },
        );
        stats.insert(
            "mbid-2".to_string(),
            CachedTrackStats {
                play_count: 10,
                skip_count: 0,
                last_played_at: 2000,
                last_skipped_at: 0,
            },
        );

        db.upsert_track_stats(&stats).unwrap();

        let result = db.get_all_track_stats().unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result["mbid-1"].play_count, 5);
        assert_eq!(result["mbid-2"].play_count, 10);
    }

    #[test]
    fn test_upsert_max_semantics() {
        let db = Database::open_in_memory().unwrap();

        // Insert initial stats
        let mut stats = HashMap::new();
        stats.insert(
            "mbid-1".to_string(),
            CachedTrackStats {
                play_count: 10,
                skip_count: 3,
                last_played_at: 1000,
                last_skipped_at: 500,
            },
        );
        db.upsert_track_stats(&stats).unwrap();

        // Increment locally (now play_count=11)
        db.increment_play_count("mbid-1", 1100).unwrap();

        // The scrobble server still returns 10 (hasn't processed the local scrobble yet)
        let mut stale_stats = HashMap::new();
        stale_stats.insert(
            "mbid-1".to_string(),
            CachedTrackStats {
                play_count: 10,
                skip_count: 3,
                last_played_at: 1000,
                last_skipped_at: 500,
            },
        );
        db.upsert_track_stats(&stale_stats).unwrap();

        // MAX should keep 11, not regress to 10
        let result = db.get_all_track_stats().unwrap();
        assert_eq!(result["mbid-1"].play_count, 11);
        assert_eq!(result["mbid-1"].last_played_at, 1100);
    }

    #[test]
    fn test_increment_play_count() {
        let db = Database::open_in_memory().unwrap();

        db.increment_play_count("mbid-1", 1000).unwrap();
        db.increment_play_count("mbid-1", 2000).unwrap();

        let result = db.get_all_track_stats().unwrap();
        assert_eq!(result["mbid-1"].play_count, 2);
        assert_eq!(result["mbid-1"].last_played_at, 2000);
        assert_eq!(result["mbid-1"].skip_count, 0);
    }

    #[test]
    fn test_increment_skip_count() {
        let db = Database::open_in_memory().unwrap();

        db.increment_skip_count("mbid-1", 500).unwrap();

        let result = db.get_all_track_stats().unwrap();
        assert_eq!(result["mbid-1"].skip_count, 1);
        assert_eq!(result["mbid-1"].last_skipped_at, 500);
        assert_eq!(result["mbid-1"].play_count, 0);
    }

    #[test]
    fn test_empty_upsert() {
        let db = Database::open_in_memory().unwrap();
        db.upsert_track_stats(&HashMap::new()).unwrap();
        let result = db.get_all_track_stats().unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_all_track_mbids() {
        let db = Database::open_in_memory().unwrap();

        // Insert some tracks with and without MBIDs
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO cached_albums (jellyfin_id, name, last_synced_at) VALUES ('album-1', 'Album', 1000)",
                [],
            )?;
            conn.execute(
                "INSERT INTO cached_tracks (jellyfin_id, track_mbid, name, album_jellyfin_id, track_number, disc_number, duration_ms, last_synced_at)
                 VALUES ('t1', 'mbid-a', 'Track 1', 'album-1', 1, 1, 180000, 1000)",
                [],
            )?;
            conn.execute(
                "INSERT INTO cached_tracks (jellyfin_id, track_mbid, name, album_jellyfin_id, track_number, disc_number, duration_ms, last_synced_at)
                 VALUES ('t2', NULL, 'Track 2', 'album-1', 2, 1, 200000, 1000)",
                [],
            )?;
            conn.execute(
                "INSERT INTO cached_tracks (jellyfin_id, track_mbid, name, album_jellyfin_id, track_number, disc_number, duration_ms, last_synced_at)
                 VALUES ('t3', 'mbid-b', 'Track 3', 'album-1', 3, 1, 220000, 1000)",
                [],
            )?;
            Ok(())
        }).unwrap();

        let mbids = db.get_all_track_mbids().unwrap();
        assert_eq!(mbids.len(), 2);
        assert!(mbids.contains(&"mbid-a".to_string()));
        assert!(mbids.contains(&"mbid-b".to_string()));
    }
}
