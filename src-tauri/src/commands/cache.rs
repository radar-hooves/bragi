//! Cache-related Tauri commands.
//!
//! Commands for managing the local SQLite cache, including
//! sync queue operations and cache statistics.

use crate::db::{CacheStats, LibraryViewParams, LibraryViewResult, QueuedOperation};
use crate::error::{CommandError, CommandResult};
use crate::models::{Album, Artist, SearchResults, Track};
use crate::state::AppState;

/// Get all cached albums from the local database.
#[tauri::command]
pub async fn get_cached_albums(state: tauri::State<'_, AppState>) -> CommandResult<Vec<Album>> {
    tracing::debug!("Fetching all cached albums");

    let mut albums = state
        .db
        .get_all_albums()
        .map_err(|e| format!("Failed to get cached albums: {}", e))?;
    // A cached OpenSubsonic row stores the port-free cover sentinel; rewrite it to
    // a live `/cover` proxy URL when the proxy is up. Best-effort: if it isn't
    // (genuinely offline), the disk-cache primary `<img src>` still renders.
    state.resolve_albums_artwork(&mut albums).await;
    Ok(albums)
}

/// Get recently added albums from the local database.
///
/// Returns albums that were added to the cache within the specified number of days.
#[tauri::command]
pub async fn get_recently_added_albums(
    state: tauri::State<'_, AppState>,
    days: u32,
) -> CommandResult<Vec<Album>> {
    tracing::debug!("Fetching recently added albums (last {} days)", days);

    let mut albums = state
        .db
        .get_recently_added_albums(days)
        .map_err(|e| format!("Failed to get recently added albums: {}", e))?;
    state.resolve_albums_artwork(&mut albums).await;
    Ok(albums)
}

/// Get cached tracks for a specific album.
#[tauri::command]
pub async fn get_cached_tracks(
    state: tauri::State<'_, AppState>,
    album_id: String,
) -> CommandResult<Vec<Track>> {
    tracing::debug!("Fetching cached tracks for album: {}", album_id);

    let mut tracks = state
        .db
        .get_tracks_by_album(&album_id)
        .map_err(|e| format!("Failed to get cached tracks: {}", e))?;
    state.resolve_tracks_artwork(&mut tracks).await;
    Ok(tracks)
}

/// Get all cached tracks from the local database.
#[tauri::command]
pub async fn get_all_cached_tracks(state: tauri::State<'_, AppState>) -> CommandResult<Vec<Track>> {
    tracing::debug!("Fetching all cached tracks");

    let mut tracks = state
        .db
        .get_all_tracks()
        .map_err(|e| format!("Failed to get cached tracks: {}", e))?;
    state.resolve_tracks_artwork(&mut tracks).await;
    Ok(tracks)
}

/// Get cached albums for a specific artist from the local database.
#[tauri::command]
pub async fn get_cached_artist_albums(
    state: tauri::State<'_, AppState>,
    artist_id: String,
) -> CommandResult<Vec<Album>> {
    tracing::debug!("Fetching cached albums for artist: {}", artist_id);

    let mut albums = state
        .db
        .get_albums_by_artist(&artist_id)
        .map_err(|e| format!("Failed to get cached artist albums: {}", e))?;
    state.resolve_albums_artwork(&mut albums).await;
    Ok(albums)
}

/// Get all cached artists from the local database.
#[tauri::command]
pub async fn get_cached_artists(state: tauri::State<'_, AppState>) -> CommandResult<Vec<Artist>> {
    tracing::debug!("Fetching all cached artists");

    let mut artists = state
        .db
        .get_all_artists()
        .map_err(|e| format!("Failed to get cached artists: {}", e))?;
    state.resolve_artists_artwork(&mut artists).await;
    Ok(artists)
}

/// Search the local cache for tracks, albums, and artists.
#[tauri::command]
pub async fn search_cached_library(
    state: tauri::State<'_, AppState>,
    query: String,
) -> CommandResult<SearchResults> {
    tauri_plugin_telemetry::traced_with_span(
        tracing::info_span!(
            target: tauri_plugin_telemetry::COMMAND_SPAN_TARGET,
            "ipc_command",
            command = "search_cached_library",
            outcome = tracing::field::Empty,
            %query,
        ),
        async move {
            tracing::debug!("Searching cached library for: {}", query);

            if query.trim().is_empty() {
                return Ok(SearchResults::default());
            }

            let mut tracks = state
                .db
                .search_tracks(&query)
                .map_err(|e| format!("Failed to search tracks: {}", e))?;

            let mut albums = state
                .db
                .search_albums(&query)
                .map_err(|e| format!("Failed to search albums: {}", e))?;

            let mut artists = state
                .db
                .search_artists(&query)
                .map_err(|e| format!("Failed to search artists: {}", e))?;

            state.resolve_tracks_artwork(&mut tracks).await;
            state.resolve_albums_artwork(&mut albums).await;
            state.resolve_artists_artwork(&mut artists).await;

            Ok(SearchResults {
                tracks,
                albums,
                artists,
            })
        },
    )
    .await
}

/// Get cache statistics.
#[tauri::command]
pub async fn get_cache_stats(state: tauri::State<'_, AppState>) -> CommandResult<CacheStats> {
    tracing::debug!("Getting cache statistics");

    state
        .db
        .get_cache_stats()
        .map_err(|e| CommandError::from(format!("Failed to get cache stats: {}", e)))
}

/// Get all pending sync operations.
#[tauri::command]
pub async fn get_pending_sync_operations(
    state: tauri::State<'_, AppState>,
) -> CommandResult<Vec<QueuedOperation>> {
    tracing::debug!("Getting pending sync operations");

    state
        .db
        .get_pending_operations()
        .map_err(|e| CommandError::from(format!("Failed to get pending operations: {}", e)))
}

/// Query the library view with column browser data and filtered/sorted tracks.
#[tauri::command]
pub async fn query_library_view(
    state: tauri::State<'_, AppState>,
    params: LibraryViewParams,
) -> CommandResult<LibraryViewResult> {
    tracing::debug!("Querying library view");

    let mut result = state
        .db
        .query_library_view(&params)
        .map_err(|e| format!("Failed to query library view: {}", e))?;
    state.resolve_tracks_artwork(&mut result.tracks).await;
    Ok(result)
}

/// Per-genre counts for the Genres view, aggregated in SQL.
///
/// Replaces a client-side walk over every cached track, which both mis-counted
/// multi-value genre tags and hit Svelte 5's O(n^2) dependency-tracking cliff.
#[tauri::command]
pub async fn get_genre_stats(
    state: tauri::State<'_, AppState>,
) -> CommandResult<Vec<crate::db::GenreStat>> {
    tracing::debug!("Fetching genre stats");
    state
        .db
        .genre_stats()
        .map_err(|e| CommandError::from(format!("Failed to fetch genre stats: {}", e)))
}

/// Query tracks with filtering and sorting (without column browser data).
#[tauri::command]
pub async fn query_tracks(
    state: tauri::State<'_, AppState>,
    params: LibraryViewParams,
) -> CommandResult<Vec<Track>> {
    tracing::debug!("Querying tracks");

    let mut tracks = state
        .db
        .query_tracks(&params)
        .map_err(|e| format!("Failed to query tracks: {}", e))?;
    state.resolve_tracks_artwork(&mut tracks).await;
    Ok(tracks)
}
