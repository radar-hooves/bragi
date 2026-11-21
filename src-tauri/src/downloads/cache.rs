//! Cache management for downloaded audio files.
//!
//! Every row in `cached_tracks` with a `cached_file_path` is a track the user
//! explicitly downloaded (nothing else writes that column — streaming plays
//! through mpv's own in-memory network cache, never through this table). Apple
//! Music and Spotify never silently delete a download to make room for
//! another; only a streaming cache is trimmed automatically, and a download
//! that would exceed the limit is refused with a message instead. Bragi has
//! no streaming cache of its own to trim, so the only automatic policy left is
//! [`cache_has_room`]: full clearing (an explicit user action, mirroring
//! Spotify's "Remove all downloads") is the rest.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use crate::audio::{AudioCommand, AudioEngineHandle};
use crate::db::Database;
use crate::error::AppError;

use super::types::DownloadEvent;
use super::worker::delete_cached_file;

/// Shared slot holding the audio engine handle (mirrors `AppState.audio`).
pub(super) type AudioSlot = Arc<tokio::sync::RwLock<Option<AudioEngineHandle>>>;

/// Track ids the cache must never delete right now: the current track plus
/// the gapless lookahead window mpv may already have open.
pub(super) async fn protected_track_ids(audio: &AudioSlot) -> HashSet<String> {
    let mut protected = HashSet::new();
    let Ok(guard) = audio.try_read() else {
        return protected;
    };
    let Some(handle) = guard.as_ref() else {
        return protected;
    };
    let (tx, rx) = std::sync::mpsc::channel();
    if handle.send(AudioCommand::QuerySnapshot(tx)).is_err() {
        return protected;
    }
    let Ok(Ok((playback, queue))) =
        tokio::task::spawn_blocking(move || rx.recv_timeout(std::time::Duration::from_secs(2)))
            .await
    else {
        return protected;
    };
    if let Some(track) = playback.current_track {
        protected.insert(track.jellyfin_id);
    }
    if let Ok(current) = usize::try_from(queue.current_index) {
        for entry in queue.tracks.iter().skip(current + 1).take(2) {
            protected.insert(entry.jellyfin_id.clone());
        }
    }
    protected
}

/// Whether the cache has room for another download under `cache_limit_bytes`.
///
/// A download is the user's explicit choice to keep a track offline, so a
/// full cache refuses the new download rather than silently deleting an
/// older one — the same contract Spotify's desktop client documents ("If you
/// don't have enough storage space, you need to free up space by manually
/// removing downloaded tracks").
pub(super) fn cache_has_room(db: &Database, cache_limit_bytes: u64) -> Result<bool, AppError> {
    Ok(db.get_cached_files_size()? < cache_limit_bytes)
}

/// Clear all cached files.
///
/// A track whose file cannot be deleted keeps its DB row (the file is still
/// on disk, so it is still cached) and is not counted as freed — the DB and
/// the disk must never disagree about what is cached. The currently playing
/// or pre-buffered tracks are skipped for the same reason eviction skips
/// them; they clear on the next attempt once playback moves on.
pub(super) async fn clear_all_cache<F>(
    db: &Arc<Database>,
    protected: &HashSet<String>,
    event_callback: &Arc<F>,
) -> Result<(), AppError>
where
    F: Fn(DownloadEvent) + Send + Sync + 'static,
{
    let cached_tracks = db.get_cached_tracks_for_eviction()?;

    let mut freed_bytes: u64 = 0;
    let mut evicted_track_ids: Vec<String> = Vec::new();
    let mut skipped: u32 = 0;

    for (track_id, file_path, file_size) in cached_tracks {
        if protected.contains(&track_id) {
            skipped += 1;
            continue;
        }

        let path = PathBuf::from(&file_path);
        if let Err(e) = delete_cached_file(&path).await {
            tracing::warn!("Failed to delete cached file {}: {}", file_path, e);
            skipped += 1;
            continue;
        }

        if let Err(e) = db.clear_track_cached_path(&track_id) {
            tracing::warn!("Failed to clear track cache path in database: {}", e);
        }

        freed_bytes += file_size;
        evicted_track_ids.push(track_id);
    }

    if skipped > 0 {
        tracing::info!(
            "Cache clear skipped {} in-use or undeletable tracks",
            skipped
        );
    }

    let evicted_count = evicted_track_ids.len() as u32;
    event_callback(DownloadEvent::CacheEvicted {
        evicted_count,
        freed_bytes,
        evicted_track_ids,
    });

    tracing::info!(
        "Cache cleared: freed {} bytes, removed {} tracks",
        freed_bytes,
        evicted_count
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{CachedAlbum, CachedTrack};
    use crate::mediaserver::ServerKind;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    const ALBUM_ID: &str = "album123456789012345678901234567";

    /// Seed `n` cached tracks (100 bytes each) under one album, so
    /// `get_cached_tracks_for_eviction` / `get_cached_files_size` see them.
    fn seed_cached_tracks(db: &Database, n: usize) -> Vec<String> {
        db.upsert_album(
            &CachedAlbum {
                jellyfin_id: ALBUM_ID.to_string(),
                release_mbid: None,
                name: "Test Album".to_string(),
                artist_name: Some("Test Artist".to_string()),
                artist_jellyfin_id: None,
                year: None,
                genre: None,
                artwork_url: None,
                track_count: Some(n as u32),
                duration_ms: None,
                date_created: None,
            },
            ServerKind::Jellyfin,
            None,
        )
        .unwrap();

        let ids: Vec<String> = (0..n).map(|i| format!("track{:029}{:03}", 0, i)).collect();
        let tracks: Vec<CachedTrack> = ids
            .iter()
            .map(|id| CachedTrack {
                jellyfin_id: id.clone(),
                track_mbid: None,
                artist_mbid: None,
                release_mbid: None,
                name: format!("Track {}", id),
                artist_name: Some("Test Artist".to_string()),
                album_jellyfin_id: Some(ALBUM_ID.to_string()),
                track_number: Some(1),
                disc_number: Some(1),
                duration_ms: Some(200_000),
                genre: None,
                artwork_url: None,
                cached_file_path: Some(format!("/tmp/does-not-exist/{}.flac", id)),
                file_size_bytes: Some(100),
                normalization_gain_db: None,
                server_play_count: None,
            })
            .collect();
        db.upsert_tracks(&tracks, ServerKind::Jellyfin, None)
            .unwrap();
        ids
    }

    /// Collects every `CacheEvicted` event's `evicted_track_ids` into one Vec.
    fn collecting_callback() -> (
        impl Fn(DownloadEvent) + Send + Sync + 'static,
        Arc<Mutex<Vec<String>>>,
    ) {
        let collected = Arc::new(Mutex::new(Vec::new()));
        let collected_for_cb = collected.clone();
        let callback = move |event: DownloadEvent| {
            if let DownloadEvent::CacheEvicted {
                evicted_track_ids, ..
            } = event
            {
                collected_for_cb.lock().unwrap().extend(evicted_track_ids);
            }
        };
        (callback, collected)
    }

    /// A downloaded track is never evictable to make room for another: the
    /// cache is full (2 tracks x 100 bytes = 200 bytes, at a 200-byte limit),
    /// so there is no room left, matching Apple Music/Spotify's "never evict
    /// a download" policy.
    #[test]
    fn cache_has_room_false_when_at_or_over_limit() {
        let db = Database::open_in_memory().unwrap();
        seed_cached_tracks(&db, 2);

        assert!(!cache_has_room(&db, 200).unwrap());
        assert!(!cache_has_room(&db, 150).unwrap());
    }

    /// Below the limit there is room for another download.
    #[test]
    fn cache_has_room_true_when_under_limit() {
        let db = Database::open_in_memory().unwrap();
        seed_cached_tracks(&db, 2);

        assert!(cache_has_room(&db, 201).unwrap());
    }

    /// `clear_all_cache` names exactly what it cleared, excluding a protected
    /// (currently playing/pre-buffered) track — never every cached track.
    /// This is what lets the frontend clear only those tracks' downloaded
    /// badges instead of wiping every badge in the library.
    #[tokio::test]
    async fn clear_all_cache_names_exactly_the_cleared_tracks_excluding_protected() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let ids = seed_cached_tracks(&db, 3);
        let protected: HashSet<String> = [ids[1].clone()].into_iter().collect();

        let (callback, collected) = collecting_callback();
        clear_all_cache(&db, &protected, &Arc::new(callback))
            .await
            .unwrap();

        let mut evicted = collected.lock().unwrap().clone();
        evicted.sort();
        let mut expected = vec![ids[0].clone(), ids[2].clone()];
        expected.sort();
        assert_eq!(evicted, expected);
        assert!(db
            .get_track(&ids[1])
            .unwrap()
            .unwrap()
            .cached_file_path
            .is_some());
    }

    /// Guards the count field against drifting from the id list it is derived
    /// from (`evicted_count = evicted_track_ids.len()`).
    #[tokio::test]
    async fn clear_all_cache_evicted_count_matches_evicted_track_ids_length() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        seed_cached_tracks(&db, 3);
        let counted = Arc::new(AtomicUsize::new(0));
        let counted_for_cb = counted.clone();
        let callback = move |event: DownloadEvent| {
            if let DownloadEvent::CacheEvicted {
                evicted_count,
                evicted_track_ids,
                ..
            } = event
            {
                assert_eq!(evicted_count as usize, evicted_track_ids.len());
                counted_for_cb.fetch_add(1, Ordering::SeqCst);
            }
        };
        clear_all_cache(&db, &HashSet::new(), &Arc::new(callback))
            .await
            .unwrap();
        assert_eq!(
            counted.load(Ordering::SeqCst),
            1,
            "callback must fire exactly once"
        );
    }
}
