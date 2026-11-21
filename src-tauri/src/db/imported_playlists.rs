//! Local storage for shared/imported playlists.
//!
//! An imported playlist is a point-in-time, read-only snapshot of a playlist
//! exported from someone else's Bragi instance — it is never written back to
//! this instance's own media server. Tracks are keyed by MusicBrainz recording
//! ID (the canonical cross-instance identifier) and re-resolved against this
//! library's `cached_tracks` on every read (a live LEFT JOIN, never a stored
//! match), so resolution stays current as the local library syncs or
//! downloads more tracks after import.

use rusqlite::{params, OptionalExtension};

use super::{current_timestamp, Database, DatabaseError};
use crate::models::{
    ImportPlaylistTrackInput, ImportedPlaylist, ImportedPlaylistDetail, ImportedPlaylistTrack,
};

impl Database {
    /// Persist a newly-imported playlist and return its resolved detail.
    pub fn insert_imported_playlist(
        &self,
        id: &str,
        name: &str,
        tracks: &[ImportPlaylistTrackInput],
    ) -> Result<ImportedPlaylistDetail, DatabaseError> {
        let now = current_timestamp();

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            tx.execute(
                "INSERT INTO imported_playlists (id, name, imported_at) VALUES (?1, ?2, ?3)",
                params![id, name, now],
            )?;

            {
                let mut stmt = tx.prepare(
                    r#"
                    INSERT INTO imported_playlist_tracks (
                        playlist_id, position, mbid, title, artist_name, album_name, duration_ms
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                    "#,
                )?;

                for (position, track) in tracks.iter().enumerate() {
                    stmt.execute(params![
                        id,
                        position as i64,
                        track.mbid,
                        track.title,
                        track.artist_name,
                        track.album_name,
                        track.duration_ms.map(|d| d as i64),
                    ])?;
                }
            }

            tx.commit()?;
            Ok(())
        })?;

        self.get_imported_playlist(id)?.ok_or_else(|| {
            DatabaseError::NotFound(format!(
                "imported playlist '{id}' vanished immediately after insert"
            ))
        })
    }

    /// List every imported playlist (sidebar summaries), newest first.
    pub fn get_imported_playlists(&self) -> Result<Vec<ImportedPlaylist>, DatabaseError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, imported_at FROM imported_playlists ORDER BY imported_at DESC",
            )?;
            let playlists = stmt
                .query_map([], |row| {
                    Ok(ImportedPlaylist {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        imported_at: row.get(2)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(playlists)
        })
    }

    /// Read one imported playlist, resolving each track against THIS
    /// instance's own library. Scoped to the active backend like every other
    /// cache read. Joins against a `GROUP BY track_mbid` derived table
    /// (rather than three independent correlated subqueries) so a library
    /// with more than one cached track sharing an mbid (a rare edge case, not
    /// schema-guaranteed unique) can never fan a single imported track out
    /// into duplicate rows, and always resolves jellyfin_id/artwork_url/
    /// cached_file_path from the SAME underlying row.
    pub fn get_imported_playlist(
        &self,
        id: &str,
    ) -> Result<Option<ImportedPlaylistDetail>, DatabaseError> {
        let source = self.active_source().as_str();

        self.with_conn(|conn| {
            let playlist = conn
                .query_row(
                    "SELECT id, name, imported_at FROM imported_playlists WHERE id = ?1",
                    params![id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    },
                )
                .optional()?;

            let Some((id, name, imported_at)) = playlist else {
                return Ok(None);
            };

            let mut stmt = conn.prepare(
                r#"
                SELECT
                    ipt.mbid, ipt.title, ipt.artist_name, ipt.album_name, ipt.duration_ms,
                    ct.jellyfin_id, ct.artwork_url, ct.cached_file_path
                FROM imported_playlist_tracks ipt
                LEFT JOIN (
                    SELECT track_mbid, jellyfin_id, artwork_url, cached_file_path
                    FROM cached_tracks
                    WHERE source = ?2
                    GROUP BY track_mbid
                ) ct ON ct.track_mbid = ipt.mbid
                WHERE ipt.playlist_id = ?1
                ORDER BY ipt.position ASC
                "#,
            )?;

            let tracks = stmt
                .query_map(params![id, source], |row| {
                    Ok(ImportedPlaylistTrack {
                        mbid: row.get(0)?,
                        title: row.get(1)?,
                        artist_name: row.get(2)?,
                        album_name: row.get(3)?,
                        duration_ms: row.get::<_, Option<i64>>(4)?.map(|d| d as u64),
                        resolved_jellyfin_id: row.get(5)?,
                        artwork_url: row.get(6)?,
                        cached_file_path: row.get(7)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(Some(ImportedPlaylistDetail {
                id,
                name,
                imported_at,
                tracks,
            }))
        })
    }

    /// Delete an imported playlist. Its tracks cascade via the FK.
    pub fn delete_imported_playlist(&self, id: &str) -> Result<(), DatabaseError> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM imported_playlists WHERE id = ?1", params![id])?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(n: u32, mbid: Option<&str>) -> ImportPlaylistTrackInput {
        ImportPlaylistTrackInput {
            mbid: mbid.map(|s| s.to_string()),
            title: format!("Track {n}"),
            artist_name: "Artist".to_string(),
            album_name: Some("Album".to_string()),
            duration_ms: Some(200_000 + u64::from(n)),
        }
    }

    #[test]
    fn insert_and_list_round_trips() {
        let db = Database::open_in_memory().unwrap();
        let detail = db
            .insert_imported_playlist(
                "pl-1",
                "My Shared Mix",
                &[track(0, Some("mb-0")), track(1, None)],
            )
            .unwrap();

        assert_eq!(detail.name, "My Shared Mix");
        assert_eq!(detail.tracks.len(), 2);
        assert_eq!(detail.tracks[0].title, "Track 0");
        // Neither track exists in the (empty) local library yet.
        assert!(detail.tracks[0].resolved_jellyfin_id.is_none());
        assert!(detail.tracks[1].mbid.is_none());

        let summaries = db.get_imported_playlists().unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "pl-1");
    }

    #[test]
    fn resolves_against_local_library_by_mbid() {
        let db = Database::open_in_memory().unwrap();
        db.insert_imported_playlist(
            "pl-1",
            "Mix",
            &[track(0, Some("mb-match")), track(1, Some("mb-miss"))],
        )
        .unwrap();

        // Seed a local track sharing mb-match's mbid — resolution is live, not
        // computed at import time, so this happening AFTER import must still work.
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO cached_tracks (jellyfin_id, track_mbid, name, last_synced_at, source)
                 VALUES ('jf-track-1', 'mb-match', 'Track 0', 0, 'jellyfin')",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        let detail = db.get_imported_playlist("pl-1").unwrap().unwrap();
        assert_eq!(
            detail.tracks[0].resolved_jellyfin_id.as_deref(),
            Some("jf-track-1")
        );
        assert!(detail.tracks[1].resolved_jellyfin_id.is_none());
    }

    #[test]
    fn get_missing_playlist_returns_none() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.get_imported_playlist("nope").unwrap().is_none());
    }

    #[test]
    fn delete_removes_playlist_and_cascades_tracks() {
        let db = Database::open_in_memory().unwrap();
        db.insert_imported_playlist("pl-1", "Mix", &[track(0, Some("mb-0"))])
            .unwrap();
        db.delete_imported_playlist("pl-1").unwrap();

        assert!(db.get_imported_playlist("pl-1").unwrap().is_none());
        let orphans: i64 = db
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM imported_playlist_tracks WHERE playlist_id = 'pl-1'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(orphans, 0);
    }
}
