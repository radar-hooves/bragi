//! Local cache for playlist details (offline-first + instant reopen).
//!
//! The first click on a playlist after launch was a cold network round-trip to
//! the media server. Persisting the tracks here lets the store hydrate
//! instantly from SQLite and revalidate in the background, and makes playlists
//! browsable offline. A presence marker row in `cached_playlists` distinguishes
//! a legitimately empty playlist from one that was never cached.
//!
//! Every row is scoped to a backend, so a dual-connected user's Jellyfin and
//! OpenSubsonic playlists never collide on id. Reads use the live
//! `active_source()`; writes take the fetching client's own `client.kind()`
//! explicitly, because the caller awaits an HTTP fetch (during which a backend
//! toggle can flip `active_source()`) before writing.

use rusqlite::{params, OptionalExtension};

use super::{current_timestamp, Database, DatabaseError};
use crate::mediaserver::ServerKind;
use crate::models::{JellyfinPlaylistDetail, JellyfinPlaylistTrack};

impl Database {
    /// Replace the cached tracks for a playlist in a single transaction.
    ///
    /// Also upserts the `cached_playlists` presence marker so an empty playlist
    /// is remembered as "cached and empty" rather than "never cached".
    ///
    /// `source` is the fetching client's own backend (`client.kind()`), passed
    /// explicitly rather than read from `active_source()`: the caller clones the
    /// client and awaits the HTTP fetch, during which a backend toggle can flip
    /// `active_source()`, so stamping the write with the live active source would
    /// mislabel this backend's tracks as the other's and let a later per-source
    /// eviction delete them (mirrors `upsert_albums`).
    pub fn replace_playlist_tracks(
        &self,
        playlist_id: &str,
        tracks: &[JellyfinPlaylistTrack],
        source: ServerKind,
    ) -> Result<(), DatabaseError> {
        let now = current_timestamp();
        let source = source.as_str();

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            tx.execute(
                r#"
                INSERT INTO cached_playlists (playlist_id, source, last_synced_at)
                VALUES (?1, ?2, ?3)
                ON CONFLICT(source, playlist_id) DO UPDATE SET last_synced_at = excluded.last_synced_at
                "#,
                params![playlist_id, source, now],
            )?;

            tx.execute(
                "DELETE FROM cached_playlist_tracks WHERE playlist_id = ?1 AND source = ?2",
                params![playlist_id, source],
            )?;

            {
                let mut stmt = tx.prepare(
                    r#"
                    INSERT INTO cached_playlist_tracks (
                        playlist_id, source, position, jellyfin_id, mbid, playlist_item_id, name,
                        artist_name, album_name, album_jellyfin_id, duration_ms, artwork_url
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                    "#,
                )?;

                for (position, track) in tracks.iter().enumerate() {
                    stmt.execute(params![
                        playlist_id,
                        source,
                        position as i64,
                        track.jellyfin_id,
                        track.mbid,
                        track.playlist_item_id,
                        track.name,
                        track.artist_name,
                        track.album_name,
                        track.album_jellyfin_id,
                        track.duration_ms as i64,
                        track.artwork_url,
                    ])?;
                }
            }

            tx.commit()?;
            Ok(())
        })
    }

    /// Read a cached playlist detail, or `None` if the playlist was never cached.
    ///
    /// Returns `Some` with an empty track list for a cached-but-empty playlist,
    /// so offline browsing distinguishes it from a cache miss.
    pub fn get_cached_playlist(
        &self,
        playlist_id: &str,
    ) -> Result<Option<JellyfinPlaylistDetail>, DatabaseError> {
        let source = self.active_source().as_str();
        self.with_conn(|conn| {
            let is_cached: bool = conn
                .query_row(
                    "SELECT 1 FROM cached_playlists WHERE playlist_id = ?1 AND source = ?2",
                    params![playlist_id, source],
                    |_| Ok(true),
                )
                .optional()?
                .unwrap_or(false);

            if !is_cached {
                return Ok(None);
            }

            let mut stmt = conn.prepare(
                r#"
                SELECT jellyfin_id, mbid, playlist_item_id, name, artist_name, album_name,
                       album_jellyfin_id, duration_ms, artwork_url
                FROM cached_playlist_tracks
                WHERE playlist_id = ?1 AND source = ?2
                ORDER BY position ASC
                "#,
            )?;

            let tracks = stmt
                .query_map(params![playlist_id, source], |row| {
                    Ok(JellyfinPlaylistTrack {
                        jellyfin_id: row.get(0)?,
                        mbid: row.get(1)?,
                        playlist_item_id: row.get(2)?,
                        name: row.get(3)?,
                        artist_name: row.get(4)?,
                        album_name: row.get(5)?,
                        album_jellyfin_id: row.get(6)?,
                        duration_ms: row.get::<_, i64>(7)? as u64,
                        artwork_url: row.get(8)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(Some(JellyfinPlaylistDetail {
                jellyfin_id: playlist_id.to_string(),
                tracks,
            }))
        })
    }

    /// Drop a playlist from the cache (both the marker and its tracks).
    ///
    /// Takes the deleting client's own `source` (`client.kind()`) for the same
    /// mislabel-race reason as [`replace_playlist_tracks`]: the caller awaits the
    /// remote delete first, during which `active_source()` can flip.
    pub fn delete_cached_playlist(
        &self,
        playlist_id: &str,
        source: ServerKind,
    ) -> Result<(), DatabaseError> {
        let source = source.as_str();
        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "DELETE FROM cached_playlist_tracks WHERE playlist_id = ?1 AND source = ?2",
                params![playlist_id, source],
            )?;
            tx.execute(
                "DELETE FROM cached_playlists WHERE playlist_id = ?1 AND source = ?2",
                params![playlist_id, source],
            )?;
            tx.commit()?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JF: ServerKind = ServerKind::Jellyfin;

    fn track(n: u32) -> JellyfinPlaylistTrack {
        JellyfinPlaylistTrack {
            jellyfin_id: format!("track-{n}"),
            mbid: Some(format!("mbid-{n}")),
            playlist_item_id: format!("item-{n}"),
            name: format!("Track {n}"),
            artist_name: "Artist".to_string(),
            album_name: "Album".to_string(),
            album_jellyfin_id: "album-1".to_string(),
            duration_ms: 200_000 + u64::from(n),
            artwork_url: if n.is_multiple_of(2) {
                Some(format!("art-{n}"))
            } else {
                None
            },
        }
    }

    #[test]
    fn round_trips_ordered_tracks() {
        let db = Database::open_in_memory().unwrap();
        let tracks = vec![track(0), track(1), track(2)];
        db.replace_playlist_tracks("pl-1", &tracks, JF).unwrap();

        let got = db.get_cached_playlist("pl-1").unwrap().unwrap();
        assert_eq!(got.jellyfin_id, "pl-1");
        assert_eq!(got.tracks.len(), 3);
        // Order preserved by position.
        assert_eq!(got.tracks[0].jellyfin_id, "track-0");
        assert_eq!(got.tracks[2].jellyfin_id, "track-2");
        assert_eq!(got.tracks[1].duration_ms, 200_001);
        assert_eq!(got.tracks[1].artwork_url, None);
    }

    #[test]
    fn replace_overwrites_previous_tracks() {
        let db = Database::open_in_memory().unwrap();
        db.replace_playlist_tracks("pl-1", &[track(0), track(1)], JF)
            .unwrap();
        db.replace_playlist_tracks("pl-1", &[track(9)], JF).unwrap();

        let got = db.get_cached_playlist("pl-1").unwrap().unwrap();
        assert_eq!(got.tracks.len(), 1, "stale rows must be cleared");
        assert_eq!(got.tracks[0].jellyfin_id, "track-9");
    }

    #[test]
    fn cached_empty_is_distinct_from_never_cached() {
        let db = Database::open_in_memory().unwrap();
        // Never cached -> None.
        assert!(db.get_cached_playlist("missing").unwrap().is_none());

        // Cached but empty -> Some(empty), so it stays browsable offline.
        db.replace_playlist_tracks("empty-pl", &[], JF).unwrap();
        let got = db.get_cached_playlist("empty-pl").unwrap();
        assert!(got.is_some());
        assert!(got.unwrap().tracks.is_empty());
    }

    #[test]
    fn delete_removes_the_cache_entry() {
        let db = Database::open_in_memory().unwrap();
        db.replace_playlist_tracks("pl-1", &[track(0)], JF).unwrap();
        db.delete_cached_playlist("pl-1", JF).unwrap();
        assert!(db.get_cached_playlist("pl-1").unwrap().is_none());
    }

    #[test]
    fn same_id_across_backends_does_not_clobber_and_evicts_per_source() {
        let db = Database::open_in_memory().unwrap();

        // Two backends cache a playlist with the SAME id but different contents,
        // each stamped with its own explicit source (as the commands do).
        db.set_active_source(ServerKind::Jellyfin);
        db.replace_playlist_tracks("shared", &[track(0), track(1)], ServerKind::Jellyfin)
            .unwrap();

        db.set_active_source(ServerKind::OpenSubsonic);
        db.replace_playlist_tracks("shared", &[track(9)], ServerKind::OpenSubsonic)
            .unwrap();

        // Each source reads back its own copy — no cross-contamination.
        let ss = db.get_cached_playlist("shared").unwrap().unwrap();
        assert_eq!(ss.tracks.len(), 1);
        assert_eq!(ss.tracks[0].jellyfin_id, "track-9");

        db.set_active_source(ServerKind::Jellyfin);
        let jf = db.get_cached_playlist("shared").unwrap().unwrap();
        assert_eq!(jf.tracks.len(), 2);
        assert_eq!(jf.tracks[0].jellyfin_id, "track-0");
    }

    #[test]
    fn write_is_stamped_with_passed_source_not_active_source() {
        // Simulates the mid-fetch toggle race: a Jellyfin fetch is in flight
        // when the user switches the active backend to OpenSubsonic. The write
        // must land under Jellyfin (the fetching client's kind), NOT under the
        // now-active OpenSubsonic source.
        let db = Database::open_in_memory().unwrap();

        // Active source is flipped to OpenSubsonic before the write completes...
        db.set_active_source(ServerKind::OpenSubsonic);
        // ...but the Jellyfin fetch stamps its own kind.
        db.replace_playlist_tracks("shared", &[track(0)], ServerKind::Jellyfin)
            .unwrap();

        // Read back under Jellyfin: the tracks are there.
        db.set_active_source(ServerKind::Jellyfin);
        let jf = db.get_cached_playlist("shared").unwrap().unwrap();
        assert_eq!(jf.tracks.len(), 1);
        assert_eq!(jf.tracks[0].jellyfin_id, "track-0");

        // And OpenSubsonic never saw the mislabelled rows in the first place.
        db.set_active_source(ServerKind::OpenSubsonic);
        assert!(db.get_cached_playlist("shared").unwrap().is_none());
    }

    #[test]
    fn delete_is_stamped_with_passed_source_not_active_source() {
        // Deleting under the fetching client's own source must not touch the
        // other backend's same-id playlist even if active_source has flipped.
        let db = Database::open_in_memory().unwrap();

        db.replace_playlist_tracks("shared", &[track(0)], ServerKind::Jellyfin)
            .unwrap();
        db.replace_playlist_tracks("shared", &[track(9)], ServerKind::OpenSubsonic)
            .unwrap();

        // Active source flipped to Jellyfin, but the delete targets OpenSubsonic.
        db.set_active_source(ServerKind::Jellyfin);
        db.delete_cached_playlist("shared", ServerKind::OpenSubsonic)
            .unwrap();

        // Jellyfin copy intact; OpenSubsonic copy gone.
        let jf = db.get_cached_playlist("shared").unwrap().unwrap();
        assert_eq!(jf.tracks[0].jellyfin_id, "track-0");
        db.set_active_source(ServerKind::OpenSubsonic);
        assert!(db.get_cached_playlist("shared").unwrap().is_none());
    }
}
