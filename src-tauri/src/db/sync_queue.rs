//! Sync queue database operations.
//!
//! Manages the offline sync queue for pending operations that need
//! to be sent to the scrobble service when online.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::utils::current_timestamp;
use super::{Database, DatabaseError};

/// A sync queue operation type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncOperation {
    /// Record a track listen (scrobble).
    Scrobble,
    /// Record a skipped track.
    Skip,
}

impl std::fmt::Display for SyncOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncOperation::Scrobble => write!(f, "scrobble"),
            SyncOperation::Skip => write!(f, "skip"),
        }
    }
}

impl std::str::FromStr for SyncOperation {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "scrobble" => Ok(SyncOperation::Scrobble),
            "skip" => Ok(SyncOperation::Skip),
            _ => Err(format!("Unknown sync operation: {}", s)),
        }
    }
}

/// A queued sync operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedOperation {
    pub id: i64,
    pub operation: SyncOperation,
    pub payload: String,
    pub created_at: i64,
    pub attempts: u32,
    pub last_attempt_at: Option<i64>,
    pub synced_at: Option<i64>,
    /// Held back for lacking a MusicBrainz recording ID at queue time
    /// (scrobbles only). Exempt from the attempts/backoff gate; revisited
    /// each drain on a cooldown gated by `last_attempt_at`.
    pub held_no_mbid: bool,
}

/// Scrobble payload for recording a track listen.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrobblePayload {
    pub track_mbid: Option<String>,
    pub artist_mbid: Option<String>,
    pub release_mbid: Option<String>,
    pub jellyfin_id: String,
    pub artist: String,
    pub track: String,
    pub album: Option<String>,
    pub listened_at: i64,
    pub duration_ms: u64,
}

/// Skip payload for recording a skipped track.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkipPayload {
    pub track_mbid: Option<String>,
    pub artist_mbid: Option<String>,
    pub release_mbid: Option<String>,
    pub jellyfin_id: String,
    pub artist: String,
    pub track: String,
    pub album: Option<String>,
    pub skipped_at: i64,
    pub played_ms: u64,
    pub track_duration_ms: u64,
}

impl Database {
    /// Queue a new sync operation.
    pub fn queue_operation(
        &self,
        operation: SyncOperation,
        payload: &str,
    ) -> Result<i64, DatabaseError> {
        let now = current_timestamp();

        self.with_conn(|conn| {
            conn.execute(
                r#"
                INSERT INTO sync_queue (operation, payload, created_at, attempts)
                VALUES (?1, ?2, ?3, 0)
                "#,
                params![operation.to_string(), payload, now],
            )?;
            Ok(conn.last_insert_rowid())
        })
    }

    /// Queue a scrobble (track listen).
    pub fn queue_scrobble(&self, scrobble: &ScrobblePayload) -> Result<i64, DatabaseError> {
        let payload = serde_json::to_string(scrobble)
            .map_err(|e| DatabaseError::Serialisation(e.to_string()))?;
        self.queue_operation(SyncOperation::Scrobble, &payload)
    }

    /// Queue a skip (skipped track).
    pub fn queue_skip(&self, skip: &SkipPayload) -> Result<i64, DatabaseError> {
        let payload =
            serde_json::to_string(skip).map_err(|e| DatabaseError::Serialisation(e.to_string()))?;
        self.queue_operation(SyncOperation::Skip, &payload)
    }

    /// Get all pending (unsynced) operations.
    pub fn get_pending_operations(&self) -> Result<Vec<QueuedOperation>, DatabaseError> {
        self.query_pending("", [])
    }

    /// Get pending operations of a specific type.
    pub fn get_pending_operations_by_type(
        &self,
        operation: SyncOperation,
    ) -> Result<Vec<QueuedOperation>, DatabaseError> {
        self.query_pending("AND operation = ?1", [operation.to_string()])
    }

    /// Unsynced rows, oldest first, narrowed by an extra `AND ...` clause.
    fn query_pending<P: rusqlite::Params>(
        &self,
        extra_filter: &str,
        params: P,
    ) -> Result<Vec<QueuedOperation>, DatabaseError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                r#"
                SELECT id, operation, payload, created_at, attempts, last_attempt_at, synced_at, held_no_mbid
                FROM sync_queue
                WHERE synced_at IS NULL {extra_filter}
                ORDER BY created_at ASC
                "#
            ))?;

            let ops = stmt
                .query_map(params, |row| {
                    let op_str: String = row.get(1)?;
                    let operation = op_str.parse().map_err(|e: String| {
                        rusqlite::Error::FromSqlConversionFailure(
                            1,
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                        )
                    })?;

                    Ok(QueuedOperation {
                        id: row.get(0)?,
                        operation,
                        payload: row.get(2)?,
                        created_at: row.get(3)?,
                        attempts: row.get(4)?,
                        last_attempt_at: row.get(5)?,
                        synced_at: row.get(6)?,
                        held_no_mbid: row.get::<_, Option<i64>>(7)? == Some(1),
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(ops)
        })
    }

    /// Get the count of pending operations.
    pub fn get_pending_count(&self) -> Result<u32, DatabaseError> {
        self.with_conn(|conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sync_queue WHERE synced_at IS NULL",
                [],
                |row| row.get(0),
            )?;
            Ok(count as u32)
        })
    }

    /// Mark an operation as synced.
    pub fn mark_synced(&self, id: i64) -> Result<(), DatabaseError> {
        let now = current_timestamp();

        self.with_conn(|conn| {
            conn.execute(
                "UPDATE sync_queue SET synced_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
            Ok(())
        })
    }

    /// Mark multiple operations as synced in a transaction.
    pub fn mark_synced_batch(&self, ids: &[i64]) -> Result<(), DatabaseError> {
        if ids.is_empty() {
            return Ok(());
        }

        let now = current_timestamp();

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            {
                let mut stmt = tx.prepare("UPDATE sync_queue SET synced_at = ?1 WHERE id = ?2")?;

                for id in ids {
                    stmt.execute(params![now, id])?;
                }
            }

            tx.commit()?;
            Ok(())
        })
    }

    /// Record a failed sync attempt.
    pub fn record_attempt(&self, id: i64) -> Result<(), DatabaseError> {
        let now = current_timestamp();

        self.with_conn(|conn| {
            conn.execute(
                "UPDATE sync_queue SET attempts = attempts + 1, last_attempt_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
            Ok(())
        })
    }

    /// Delete synced operations older than the given timestamp.
    ///
    /// Called by the sync scheduler; synced rows are otherwise only kept for debugging.
    pub fn cleanup_synced(&self, older_than: i64) -> Result<u32, DatabaseError> {
        self.with_conn(|conn| {
            let rows = conn.execute(
                "DELETE FROM sync_queue WHERE synced_at IS NOT NULL AND synced_at < ?1",
                [older_than],
            )?;
            Ok(rows as u32)
        })
    }

    /// Delete operations that have failed too many times.
    pub fn cleanup_failed(&self, max_attempts: u32) -> Result<u32, DatabaseError> {
        self.with_conn(|conn| {
            let rows = conn.execute(
                "DELETE FROM sync_queue WHERE attempts >= ?1 AND synced_at IS NULL",
                [max_attempts],
            )?;
            Ok(rows as u32)
        })
    }

    /// Mark queued scrobbles as held pending Jellyfin MBID re-resolution.
    ///
    /// Sets `held_no_mbid = 1` and stamps `last_attempt_at` — reused here as
    /// the re-resolve cooldown stamp, NOT a backoff signal. Deliberately does
    /// NOT touch `attempts`: held rows must never accrue backoff attempts, or
    /// they would eventually hit `MAX_RETRY_ATTEMPTS` and vanish from drains
    /// while still genuinely unsynced.
    pub fn mark_held_no_mbid(&self, ids: &[i64]) -> Result<(), DatabaseError> {
        if ids.is_empty() {
            return Ok(());
        }

        let now = current_timestamp();

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            {
                let mut stmt = tx.prepare(
                    "UPDATE sync_queue SET held_no_mbid = 1, last_attempt_at = ?1 WHERE id = ?2",
                )?;

                for id in ids {
                    stmt.execute(params![now, id])?;
                }
            }

            tx.commit()?;
            Ok(())
        })
    }

    /// Patch a held scrobble OR skip's payload with a newly re-resolved
    /// MusicBrainz recording ID and clear `held_no_mbid`, so it rides the
    /// CURRENT sync drain instead of waiting for the next cooldown window.
    ///
    /// Patches generically via `serde_json::Value` rather than deserialising
    /// into `ScrobblePayload` — `ScrobblePayload` and `SkipPayload` both carry
    /// a `track_mbid` field under the same `#[serde(rename_all =
    /// "camelCase")]` shape (`trackMbid`), so one JSON-level patch works for
    /// either row's payload without the caller needing to know which
    /// operation type it is.
    pub fn update_scrobble_payload_mbid(&self, id: i64, mbid: &str) -> Result<(), DatabaseError> {
        let payload: String = self.with_conn(|conn| {
            conn.query_row(
                "SELECT payload FROM sync_queue WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
        })?;

        let mut value: serde_json::Value = serde_json::from_str(&payload)
            .map_err(|e| DatabaseError::Serialisation(e.to_string()))?;
        value["trackMbid"] = serde_json::Value::String(mbid.to_string());
        let updated = serde_json::to_string(&value)
            .map_err(|e| DatabaseError::Serialisation(e.to_string()))?;

        self.with_conn(|conn| {
            conn.execute(
                "UPDATE sync_queue SET payload = ?1, held_no_mbid = 0 WHERE id = ?2",
                params![updated, id],
            )?;
            Ok(())
        })
    }

    /// Patch a held scrobble OR skip's payload with a Jellyfin id found by
    /// matching its artist/album/title against the library, rather than a
    /// stored id — the row was queued with none, so it can never be looked
    /// up by id again. Leaves `held_no_mbid` untouched: the matched track may
    /// still lack a MusicBrainz recording ID, in which case the ordinary
    /// by-id re-resolve path picks it up on ITS OWN normal cadence, now that
    /// it has a real id to query with.
    pub fn patch_held_jellyfin_id(&self, id: i64, jellyfin_id: &str) -> Result<(), DatabaseError> {
        let payload: String = self.with_conn(|conn| {
            conn.query_row(
                "SELECT payload FROM sync_queue WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
        })?;

        let mut value: serde_json::Value = serde_json::from_str(&payload)
            .map_err(|e| DatabaseError::Serialisation(e.to_string()))?;
        value["jellyfinId"] = serde_json::Value::String(jellyfin_id.to_string());
        let updated = serde_json::to_string(&value)
            .map_err(|e| DatabaseError::Serialisation(e.to_string()))?;

        self.with_conn(|conn| {
            conn.execute(
                "UPDATE sync_queue SET payload = ?1 WHERE id = ?2",
                params![updated, id],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queue_and_get_operations() {
        let db = Database::open_in_memory().unwrap();

        let payload = r#"{"track_mbid": "abc", "listened_at": 1234567890}"#;
        let id = db
            .queue_operation(SyncOperation::Scrobble, payload)
            .unwrap();

        let pending = db.get_pending_operations().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, id);
        assert_eq!(pending[0].operation, SyncOperation::Scrobble);
        assert_eq!(pending[0].attempts, 0);
    }

    #[test]
    fn test_queue_scrobble() {
        let db = Database::open_in_memory().unwrap();

        let scrobble = ScrobblePayload {
            track_mbid: Some("mb-123".to_string()),
            artist_mbid: Some("mb-artist-123".to_string()),
            release_mbid: Some("mb-release-123".to_string()),
            jellyfin_id: "jf-456".to_string(),
            artist: "Test Artist".to_string(),
            track: "Test Track".to_string(),
            album: Some("Test Album".to_string()),
            listened_at: 1234567890,
            duration_ms: 180000,
        };

        let id = db.queue_scrobble(&scrobble).unwrap();
        assert!(id > 0);

        let pending = db.get_pending_operations().unwrap();
        assert_eq!(pending.len(), 1);

        // Verify payload can be deserialised
        let parsed: ScrobblePayload = serde_json::from_str(&pending[0].payload).unwrap();
        assert_eq!(parsed.track_mbid, Some("mb-123".to_string()));
    }

    #[test]
    fn test_mark_synced() {
        let db = Database::open_in_memory().unwrap();

        let id = db.queue_operation(SyncOperation::Scrobble, "{}").unwrap();

        assert_eq!(db.get_pending_count().unwrap(), 1);

        db.mark_synced(id).unwrap();

        assert_eq!(db.get_pending_count().unwrap(), 0);
    }

    #[test]
    fn test_record_attempt() {
        let db = Database::open_in_memory().unwrap();

        let id = db.queue_operation(SyncOperation::Scrobble, "{}").unwrap();

        let pending = db.get_pending_operations().unwrap();
        assert_eq!(pending[0].attempts, 0);

        db.record_attempt(id).unwrap();

        let pending = db.get_pending_operations().unwrap();
        assert_eq!(pending[0].attempts, 1);
        assert!(pending[0].last_attempt_at.is_some());
    }

    #[test]
    fn test_cleanup_failed() {
        let db = Database::open_in_memory().unwrap();

        let id = db.queue_operation(SyncOperation::Scrobble, "{}").unwrap();

        // Simulate 5 failed attempts
        for _ in 0..5 {
            db.record_attempt(id).unwrap();
        }

        // Should still be pending
        assert_eq!(db.get_pending_count().unwrap(), 1);

        // Cleanup operations with 5+ attempts
        let cleaned = db.cleanup_failed(5).unwrap();
        assert_eq!(cleaned, 1);

        assert_eq!(db.get_pending_count().unwrap(), 0);
    }

    #[test]
    fn test_batch_sync() {
        let db = Database::open_in_memory().unwrap();

        let id1 = db.queue_operation(SyncOperation::Scrobble, "{}").unwrap();
        let id2 = db.queue_operation(SyncOperation::Scrobble, "{}").unwrap();
        let _id3 = db.queue_operation(SyncOperation::Scrobble, "{}").unwrap();

        assert_eq!(db.get_pending_count().unwrap(), 3);

        db.mark_synced_batch(&[id1, id2]).unwrap();

        assert_eq!(db.get_pending_count().unwrap(), 1);
    }

    fn sample_scrobble(track_mbid: Option<&str>) -> ScrobblePayload {
        ScrobblePayload {
            track_mbid: track_mbid.map(|s| s.to_string()),
            artist_mbid: None,
            release_mbid: None,
            jellyfin_id: "jf-1".to_string(),
            artist: "Test Artist".to_string(),
            track: "Test Track".to_string(),
            album: Some("Test Album".to_string()),
            listened_at: 1234567890,
            duration_ms: 180000,
        }
    }

    fn sample_skip(track_mbid: Option<&str>) -> SkipPayload {
        SkipPayload {
            track_mbid: track_mbid.map(|s| s.to_string()),
            artist_mbid: None,
            release_mbid: None,
            jellyfin_id: "jf-skip-1".to_string(),
            artist: "Skip Artist".to_string(),
            track: "Skip Track".to_string(),
            album: Some("Skip Album".to_string()),
            skipped_at: 1234567890,
            played_ms: 1000,
            track_duration_ms: 200000,
        }
    }

    #[test]
    fn queued_operations_default_to_not_held() {
        let db = Database::open_in_memory().unwrap();
        db.queue_scrobble(&sample_scrobble(Some("mb-1"))).unwrap();

        let pending = db.get_pending_operations().unwrap();
        assert!(
            !pending[0].held_no_mbid,
            "a normal queued row must not be held"
        );
    }

    #[test]
    fn mark_held_no_mbid_sets_flag_and_cooldown_stamp_without_touching_attempts() {
        let db = Database::open_in_memory().unwrap();
        let id = db.queue_scrobble(&sample_scrobble(None)).unwrap();

        db.mark_held_no_mbid(&[id]).unwrap();

        let pending = db.get_pending_operations().unwrap();
        assert!(pending[0].held_no_mbid, "row must be flagged held");
        assert!(
            pending[0].last_attempt_at.is_some(),
            "held marking must stamp the re-resolve cooldown"
        );
        assert_eq!(
            pending[0].attempts, 0,
            "held rows must never accrue backoff attempts"
        );
    }

    #[test]
    fn update_scrobble_payload_mbid_patches_payload_and_clears_held() {
        let db = Database::open_in_memory().unwrap();
        let id = db.queue_scrobble(&sample_scrobble(None)).unwrap();
        db.mark_held_no_mbid(&[id]).unwrap();

        db.update_scrobble_payload_mbid(id, "resolved-mbid-123")
            .unwrap();

        let pending = db.get_pending_operations().unwrap();
        assert!(
            !pending[0].held_no_mbid,
            "resolving the mbid must clear the held flag"
        );
        let payload: ScrobblePayload = serde_json::from_str(&pending[0].payload).unwrap();
        assert_eq!(payload.track_mbid, Some("resolved-mbid-123".to_string()));
    }

    /// The generic JSON patch in `update_scrobble_payload_mbid` must resolve
    /// a held SKIP's `trackMbid` too, not just a scrobble's — the scrobble server's
    /// `/client/skips` also requires a track_mbid, so a mbid-less skip is
    /// held and re-resolved the same way as a mbid-less scrobble.
    #[test]
    fn update_scrobble_payload_mbid_patches_skip_payload_too() {
        let db = Database::open_in_memory().unwrap();
        let id = db.queue_skip(&sample_skip(None)).unwrap();
        db.mark_held_no_mbid(&[id]).unwrap();

        db.update_scrobble_payload_mbid(id, "resolved-skip-mbid-456")
            .unwrap();

        let pending = db.get_pending_operations().unwrap();
        assert!(
            !pending[0].held_no_mbid,
            "resolving the mbid must clear the held flag on a skip row too"
        );
        let payload: SkipPayload = serde_json::from_str(&pending[0].payload).unwrap();
        assert_eq!(
            payload.track_mbid,
            Some("resolved-skip-mbid-456".to_string())
        );
        // Every other field must survive the generic JSON patch untouched.
        assert_eq!(payload.artist, "Skip Artist");
        assert_eq!(payload.jellyfin_id, "jf-skip-1");
        assert_eq!(payload.played_ms, 1000);
    }

    /// Patches ONLY the id, leaving the row held — the matched track may
    /// still lack a MusicBrainz tag, in which case the ordinary by-id path
    /// picks it up later now that it has a real id to query with.
    #[test]
    fn patch_held_jellyfin_id_updates_payload_and_leaves_held_flag() {
        let db = Database::open_in_memory().unwrap();
        let mut no_id = sample_scrobble(None);
        no_id.jellyfin_id = String::new();
        let id = db.queue_scrobble(&no_id).unwrap();
        db.mark_held_no_mbid(&[id]).unwrap();

        db.patch_held_jellyfin_id(id, "45751bb32ad69b7b887dc7500a6653b7")
            .unwrap();

        let pending = db.get_pending_operations().unwrap();
        assert!(
            pending[0].held_no_mbid,
            "patching the id alone must not clear held_no_mbid"
        );
        let payload: ScrobblePayload = serde_json::from_str(&pending[0].payload).unwrap();
        assert_eq!(
            payload.jellyfin_id,
            "45751bb32ad69b7b887dc7500a6653b7".to_string()
        );
        // Every other field must survive the generic JSON patch untouched.
        assert_eq!(payload.artist, "Test Artist");
        assert_eq!(payload.track, "Test Track");
    }

    #[test]
    fn test_cleanup_synced_keeps_recent_and_pending() {
        let db = Database::open_in_memory().unwrap();
        let old = db.queue_operation(SyncOperation::Scrobble, "{}").unwrap();
        let recent = db.queue_operation(SyncOperation::Scrobble, "{}").unwrap();
        db.queue_operation(SyncOperation::Scrobble, "{}").unwrap();
        db.mark_synced(old).unwrap();
        db.mark_synced(recent).unwrap();
        db.with_conn(|c| {
            c.execute("UPDATE sync_queue SET synced_at = 100 WHERE id = ?1", [old])?;
            Ok(())
        })
        .unwrap();

        assert_eq!(db.cleanup_synced(1_000).unwrap(), 1);
        assert_eq!(db.get_pending_count().unwrap(), 1);
        let left: i64 = db
            .with_conn(|c| c.query_row("SELECT COUNT(*) FROM sync_queue", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(left, 2);
    }
}
