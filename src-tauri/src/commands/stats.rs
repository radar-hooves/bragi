//! Track stats commands.
//!
//! Fetches and caches listening stats from the scrobble server.

use std::collections::HashMap;

use crate::db::CachedTrackStats;
use crate::error::{CommandError, CommandResult};
use crate::services::MusicServiceClient;
use crate::state::AppState;

/// Refresh track stats from the scrobble server and return cached stats.
///
/// Fetches bulk stats for all tracks with MBIDs, caches in SQLite,
/// and returns the full stats map. On network failure or service
/// not configured, returns whatever is cached.
#[tauri::command]
pub async fn refresh_track_stats(
    state: tauri::State<'_, AppState>,
) -> CommandResult<HashMap<String, CachedTrackStats>> {
    // Get all MBIDs from the library cache
    let mbids = state
        .db
        .get_all_track_mbids()
        .map_err(|e| format!("Failed to get track MBIDs: {}", e))?;

    if mbids.is_empty() {
        return Ok(HashMap::new());
    }

    // Try to fetch from the scrobble server
    let service_guard = state.music_service.read().await;
    if let Some(client) = service_guard.as_ref() {
        let (http, base_url) = client.clone_for_request();
        drop(service_guard); // Release lock before HTTP

        match MusicServiceClient::fetch_bulk_track_stats_with_client(&http, &base_url, &mbids).await
        {
            Ok(api_stats) => {
                // Convert API response to cached format (ISO 8601 -> Unix seconds)
                let cached: HashMap<String, CachedTrackStats> = api_stats
                    .into_iter()
                    .map(|(mbid, entry)| {
                        let last_played_at = entry
                            .last_played_at
                            .as_deref()
                            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                            .map(|dt| dt.timestamp())
                            .unwrap_or(0);
                        let last_skipped_at = entry
                            .last_skipped_at
                            .as_deref()
                            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                            .map(|dt| dt.timestamp())
                            .unwrap_or(0);

                        (
                            mbid,
                            CachedTrackStats {
                                play_count: entry.play_count,
                                skip_count: entry.skip_count,
                                last_played_at,
                                last_skipped_at,
                            },
                        )
                    })
                    .collect();

                // Upsert with MAX semantics
                if let Err(e) = state.db.upsert_track_stats(&cached) {
                    tracing::warn!("Failed to cache track stats: {}", e);
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to fetch track stats from the scrobble server: {}",
                    e
                );
                // Fall through to return cached data
            }
        }
    } else {
        drop(service_guard);
        tracing::debug!("Music service not configured, returning cached stats");
    }

    // Always return cached stats (may be empty on first run)
    state
        .db
        .get_all_track_stats()
        .map_err(|e| CommandError::from(format!("Failed to read cached track stats: {}", e)))
}

/// Server-side play counts for the active backend, keyed by server (Jellyfin)
/// id.
///
/// Read-only: these are the counts the media server itself reports (captured
/// for free during library sync into `cached_tracks.server_play_count`,
/// source-scoped). Lets the visible "Plays" column work for a backend-only user
/// with no scrobble service configured. The frontend treats these as the
/// media-server tier and falls back to the scrobble server where a count is absent.
#[tauri::command]
pub async fn get_server_play_counts(
    state: tauri::State<'_, AppState>,
) -> CommandResult<HashMap<String, u32>> {
    state
        .db
        .get_server_play_counts()
        .map_err(|e| CommandError::from(format!("Failed to read server play counts: {}", e)))
}
