//! Jellyfin playlist management Tauri commands.
//!
//! Handles CRUD operations for playlists stored in Jellyfin.
//! Changes sync bidirectionally so other Jellyfin apps see updates.

use crate::error::{CommandError, CommandResult};
use crate::mediaserver::ServerKind;
use crate::models::{JellyfinPlaylist, JellyfinPlaylistDetail};
use crate::state::AppState;

/// A playlist name as create and rename accept it: trimmed, not empty, at
/// most 255 characters (not bytes, so a non-Latin name gets the same room).
fn playlist_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Playlist name cannot be empty".into());
    }
    if name.chars().count() > 255 {
        return Err("Playlist name is too long (max 255 characters)".to_string());
    }
    Ok(name.to_string())
}

/// Refresh the offline cache for a playlist after a mutation (non-fatal).
///
/// `source` is the fetching client's own `client.kind()`, captured with the
/// fetch — never `db.active_source()`, which can flip during the awaited HTTP
/// round-trip and mislabel this backend's tracks as the other's.
fn cache_playlist(
    state: &AppState,
    playlist_id: &str,
    playlist: &JellyfinPlaylistDetail,
    source: ServerKind,
) {
    if let Err(e) = state
        .db
        .replace_playlist_tracks(playlist_id, &playlist.tracks, source)
    {
        tracing::warn!("Failed to refresh cached playlist '{}': {}", playlist_id, e);
    }
}

// ============================================================================
// Tauri Commands
// ============================================================================

/// Get all Jellyfin playlists.
#[tauri::command]
pub async fn get_jellyfin_playlists(
    state: tauri::State<'_, AppState>,
) -> CommandResult<Vec<JellyfinPlaylist>> {
    tracing::info!("Getting Jellyfin playlists");

    // Clone the cheaply-cloneable (Arc-backed) client out of the guard and
    // release the RwLock before the network request — never hold the lock
    // across an HTTP await (see .claude/rules/00-project-foundations.md).
    let client = state.get_media().await?;

    let playlists = client
        .get_playlists()
        .await
        .map_err(|e| format!("Failed to get playlists: {}", e))?;

    tracing::info!("Retrieved {} Jellyfin playlists", playlists.len());

    Ok(playlists)
}

/// Get a Jellyfin playlist with its tracks.
#[tauri::command]
pub async fn get_jellyfin_playlist(
    state: tauri::State<'_, AppState>,
    playlist_id: String,
) -> CommandResult<JellyfinPlaylistDetail> {
    tauri_plugin_telemetry::traced_with_span(
        tracing::info_span!(
            target: tauri_plugin_telemetry::COMMAND_SPAN_TARGET,
            "ipc_command",
            command = "get_jellyfin_playlist",
            outcome = tracing::field::Empty,
            %playlist_id,
        ),
        async move {
            tracing::info!("Getting Jellyfin playlist: {}", playlist_id);

            // Clone the cheaply-cloneable (Arc-backed) client out of the guard and
            // release the RwLock before the network request — never hold the lock
            // across an HTTP await (see .claude/rules/00-project-foundations.md).
            let client = state.get_media().await?;

            let playlist = client
                .get_playlist_tracks(&playlist_id)
                .await
                .map_err(|e| format!("Failed to get playlist: {}", e))?;

            // Persist to the offline cache so the next open (and offline browsing) is
            // instant. Non-fatal: a cache write failure must not break the live fetch.
            // Stamp with THIS client's backend, not the (possibly toggled) active source.
            if let Err(e) =
                state
                    .db
                    .replace_playlist_tracks(&playlist_id, &playlist.tracks, client.kind())
            {
                tracing::warn!("Failed to cache playlist '{}' tracks: {}", playlist_id, e);
            }

            tracing::info!(
                "Retrieved Jellyfin playlist '{}' with {} tracks",
                playlist_id,
                playlist.tracks.len()
            );

            Ok(playlist)
        },
    )
    .await
}

/// Get a playlist's tracks from the local SQLite cache only (no network).
///
/// Returns `None` when the playlist was never cached, `Some` (possibly with an
/// empty track list) when it was. Used by the store to hydrate instantly on the
/// first click and to browse playlists offline, then revalidate in the
/// background via [`get_jellyfin_playlist`].
#[tauri::command]
pub async fn get_cached_jellyfin_playlist(
    state: tauri::State<'_, AppState>,
    playlist_id: String,
) -> CommandResult<Option<JellyfinPlaylistDetail>> {
    state
        .db
        .get_cached_playlist(&playlist_id)
        .map_err(|e| CommandError::from(format!("Failed to read cached playlist: {}", e)))
}

/// Create a new Jellyfin playlist.
#[tauri::command]
pub async fn create_jellyfin_playlist(
    state: tauri::State<'_, AppState>,
    name: String,
) -> CommandResult<JellyfinPlaylist> {
    tauri_plugin_telemetry::traced_with_span(
        tracing::info_span!(
            target: tauri_plugin_telemetry::COMMAND_SPAN_TARGET,
            "ipc_command",
            command = "create_jellyfin_playlist",
            outcome = tracing::field::Empty,
            %name,
        ),
        async move {
            tracing::info!("Creating Jellyfin playlist: {}", name);

            let name = playlist_name(&name)?;

            let client = state.get_media().await?;

            let playlist = client
                .create_playlist(&name)
                .await
                .map_err(|e| format!("Failed to create playlist: {}", e))?;

            tracing::info!(
                "Created Jellyfin playlist '{}' with ID: {}",
                playlist.name,
                playlist.jellyfin_id
            );

            Ok(playlist)
        },
    )
    .await
}

/// Rename a Jellyfin playlist.
#[tauri::command]
pub async fn rename_jellyfin_playlist(
    state: tauri::State<'_, AppState>,
    playlist_id: String,
    name: String,
) -> CommandResult<()> {
    tauri_plugin_telemetry::traced_with_span(
        tracing::info_span!(
            target: tauri_plugin_telemetry::COMMAND_SPAN_TARGET,
            "ipc_command",
            command = "rename_jellyfin_playlist",
            outcome = tracing::field::Empty,
            %playlist_id,
        ),
        async move {
            tracing::info!("Renaming Jellyfin playlist {}: {}", playlist_id, name);

            let name = playlist_name(&name)?;

            let client = state.get_media().await?;

            client
                .rename_playlist(&playlist_id, &name)
                .await
                .map_err(|e| format!("Failed to rename playlist: {}", e))?;

            tracing::info!("Renamed Jellyfin playlist '{}' to '{}'", playlist_id, name);

            Ok(())
        },
    )
    .await
}

/// Delete a Jellyfin playlist.
#[tauri::command]
pub async fn delete_jellyfin_playlist(
    state: tauri::State<'_, AppState>,
    playlist_id: String,
) -> CommandResult<()> {
    tauri_plugin_telemetry::traced_with_span(
        tracing::info_span!(
            target: tauri_plugin_telemetry::COMMAND_SPAN_TARGET,
            "ipc_command",
            command = "delete_jellyfin_playlist",
            outcome = tracing::field::Empty,
            %playlist_id,
        ),
        async move {
            tracing::info!("Deleting Jellyfin playlist: {}", playlist_id);

            let client = state.get_media().await?;

            client
                .delete_playlist(&playlist_id)
                .await
                .map_err(|e| format!("Failed to delete playlist: {}", e))?;

            // Drop it from the offline cache too (non-fatal). Scope the eviction to this
            // client's backend so a mid-flight toggle can't delete the other backend's
            // same-id playlist.
            if let Err(e) = state.db.delete_cached_playlist(&playlist_id, client.kind()) {
                tracing::warn!("Failed to evict cached playlist '{}': {}", playlist_id, e);
            }

            tracing::info!("Deleted Jellyfin playlist: {}", playlist_id);

            Ok(())
        },
    )
    .await
}

/// Add tracks to a Jellyfin playlist.
#[tauri::command]
pub async fn add_tracks_to_jellyfin_playlist(
    state: tauri::State<'_, AppState>,
    playlist_id: String,
    track_ids: Vec<String>,
) -> CommandResult<JellyfinPlaylistDetail> {
    tauri_plugin_telemetry::traced_with_span(
        tracing::info_span!(
            target: tauri_plugin_telemetry::COMMAND_SPAN_TARGET,
            "ipc_command",
            command = "add_tracks_to_jellyfin_playlist",
            outcome = tracing::field::Empty,
            %playlist_id,
            track_count = track_ids.len(),
        ),
        async move {
            tracing::info!(
                "Adding {} tracks to Jellyfin playlist {}",
                track_ids.len(),
                playlist_id
            );

            if track_ids.is_empty() {
                return Err("No tracks provided".into());
            }

            let client = state.get_media().await?;

            client
                .add_tracks_to_playlist(&playlist_id, &track_ids)
                .await
                .map_err(|e| format!("Failed to add tracks: {}", e))?;

            // Return updated playlist
            let playlist = client
                .get_playlist_tracks(&playlist_id)
                .await
                .map_err(|e| format!("Failed to get updated playlist: {}", e))?;

            cache_playlist(&state, &playlist_id, &playlist, client.kind());

            tracing::info!(
                "Added tracks to Jellyfin playlist '{}', now has {} tracks",
                playlist_id,
                playlist.tracks.len()
            );

            Ok(playlist)
        },
    )
    .await
}

/// Remove tracks from a Jellyfin playlist.
#[tauri::command]
pub async fn remove_tracks_from_jellyfin_playlist(
    state: tauri::State<'_, AppState>,
    playlist_id: String,
    entry_ids: Vec<String>,
) -> CommandResult<JellyfinPlaylistDetail> {
    tracing::info!(
        "Removing {} tracks from Jellyfin playlist {}",
        entry_ids.len(),
        playlist_id
    );

    if entry_ids.is_empty() {
        return Err("No track entry IDs provided".into());
    }

    let client = state.get_media().await?;

    client
        .remove_tracks_from_playlist(&playlist_id, &entry_ids)
        .await
        .map_err(|e| format!("Failed to remove tracks: {}", e))?;

    // Return updated playlist
    let playlist = client
        .get_playlist_tracks(&playlist_id)
        .await
        .map_err(|e| format!("Failed to get updated playlist: {}", e))?;

    cache_playlist(&state, &playlist_id, &playlist, client.kind());

    tracing::info!(
        "Removed tracks from Jellyfin playlist '{}', now has {} tracks",
        playlist_id,
        playlist.tracks.len()
    );

    Ok(playlist)
}

/// Move a track within a Jellyfin playlist.
#[tauri::command]
pub async fn move_jellyfin_playlist_item(
    state: tauri::State<'_, AppState>,
    playlist_id: String,
    item_id: String,
    new_index: u32,
) -> CommandResult<JellyfinPlaylistDetail> {
    tracing::info!(
        "Moving track {} to index {} in Jellyfin playlist {}",
        item_id,
        new_index,
        playlist_id
    );

    let client = state.get_media().await?;

    client
        .move_playlist_item(&playlist_id, &item_id, new_index)
        .await
        .map_err(|e| format!("Failed to move track: {}", e))?;

    // Return updated playlist
    let playlist = client
        .get_playlist_tracks(&playlist_id)
        .await
        .map_err(|e| format!("Failed to get updated playlist: {}", e))?;

    cache_playlist(&state, &playlist_id, &playlist, client.kind());

    tracing::info!("Moved track in Jellyfin playlist '{}'", playlist_id);

    Ok(playlist)
}

#[cfg(test)]
mod tests {
    use super::playlist_name;

    #[test]
    fn a_name_is_trimmed_and_must_not_be_empty() {
        assert_eq!(playlist_name("  Road Trip  ").as_deref(), Ok("Road Trip"));
        assert!(playlist_name("   ").is_err());
    }

    #[test]
    fn the_limit_counts_characters_not_bytes() {
        assert!(playlist_name(&"夜".repeat(255)).is_ok());
        assert!(playlist_name(&"a".repeat(256)).is_err());
    }
}
