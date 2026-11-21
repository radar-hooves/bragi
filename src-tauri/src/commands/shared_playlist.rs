//! Shared/imported playlist commands.
//!
//! Two ways a playlist gets imported:
//! - A Bragi "share code" (base64url-encoded JSON — see
//!   `src/lib/utils/playlistShare.ts`) — by the time a track list reaches
//!   `import_playlist` it has already been decoded on the frontend, so that
//!   path never talks to a media server.
//! - A plain Jellyfin item link from someone who doesn't use Bragi at all —
//!   `import_playlist_from_jellyfin_link` authenticates to THEIR server with
//!   credentials they gave the user for exactly this purpose, fetches the
//!   playlist once, logs the session out, and discards the credentials:
//!   nothing here is written to Bragi's own credential store, and it never
//!   touches the user's own configured Jellyfin connection (`state.media`).
//!
//! Either way, the result only ever gets persisted locally and resolved
//! against THIS instance's own library — nothing an import does is written
//! back to any media server (read-only by construction).

use crate::error::{CommandError, CommandResult};
use crate::jellyfin::{authenticate_by_name, JellyfinClient};
use crate::models::{ImportPlaylistTrackInput, ImportedPlaylist, ImportedPlaylistDetail};
use crate::state::AppState;

/// Validate and persist a decoded track list under a new imported-playlist id.
/// Shared by both import paths below.
fn insert_imported_playlist(
    state: &AppState,
    name: &str,
    tracks: &[ImportPlaylistTrackInput],
) -> CommandResult<ImportedPlaylistDetail> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Playlist name cannot be empty".into());
    }
    if tracks.is_empty() {
        return Err("Shared playlist has no tracks".into());
    }

    let id = uuid::Uuid::new_v4().to_string();

    tracing::info!(
        "Importing shared playlist '{}' ({} tracks)",
        name,
        tracks.len()
    );

    state
        .db
        .insert_imported_playlist(&id, name, tracks)
        .map_err(|e| CommandError::from(format!("Failed to import playlist: {}", e)))
}

/// Import a shared playlist from an already-decoded Bragi share code:
/// persist the given tracks locally and return the resolved detail (which
/// tracks matched THIS instance's own library).
#[tauri::command]
pub async fn import_playlist(
    state: tauri::State<'_, AppState>,
    name: String,
    tracks: Vec<ImportPlaylistTrackInput>,
) -> CommandResult<ImportedPlaylistDetail> {
    insert_imported_playlist(&state, &name, &tracks)
}

/// Import a playlist directly from a friend's own, separate Jellyfin server —
/// for when the sender just sent Jellyfin's native "copy link" for a
/// playlist rather than a Bragi share code. They need to have given the user
/// a login on their server first (a guest account or their own); this
/// authenticates with it ONCE to fetch the one playlist, then the credentials
/// are dropped — never written to disk, never used for anything else.
#[tauri::command]
pub async fn import_playlist_from_jellyfin_link(
    state: tauri::State<'_, AppState>,
    server_url: String,
    username: String,
    password: String,
    item_id: String,
) -> CommandResult<ImportedPlaylistDetail> {
    tracing::info!(
        "Fetching shared playlist '{}' from a foreign Jellyfin server",
        item_id
    );

    let credentials = authenticate_by_name(&server_url, &username, &password)
        .await
        .map_err(|e| format!("Couldn't sign in to that Jellyfin server: {}", e))?;

    let client = JellyfinClient::from_credentials(&credentials)
        .map_err(|e| format!("Invalid Jellyfin server address: {}", e))?;

    let (name, item_type) = client
        .get_item_summary(&item_id)
        .await
        .map_err(|e| format!("Couldn't fetch that item: {}", e))?;

    if item_type != "Playlist" {
        return Err(format!("That link points to a {}, not a playlist.", item_type).into());
    }

    let detail = client
        .get_playlist_tracks(&item_id)
        .await
        .map_err(|e| format!("Couldn't fetch the playlist: {}", e))?;

    // Best-effort: end the session on their server now that we have what we
    // need, rather than leaving it authenticated until the token expires on
    // its own. Never fails the import over this.
    if let Err(e) = client.logout().await {
        tracing::debug!("Couldn't log out of foreign Jellyfin session: {}", e);
    }

    let tracks: Vec<ImportPlaylistTrackInput> = detail
        .tracks
        .into_iter()
        .map(|t| ImportPlaylistTrackInput {
            mbid: t.mbid,
            title: t.name,
            artist_name: t.artist_name,
            album_name: if t.album_name.is_empty() {
                None
            } else {
                Some(t.album_name)
            },
            duration_ms: Some(t.duration_ms),
        })
        .collect();

    insert_imported_playlist(&state, &name, &tracks)
}

/// List every locally-imported playlist (sidebar summaries).
#[tauri::command]
pub async fn get_imported_playlists(
    state: tauri::State<'_, AppState>,
) -> CommandResult<Vec<ImportedPlaylist>> {
    state
        .db
        .get_imported_playlists()
        .map_err(|e| CommandError::from(format!("Failed to list imported playlists: {}", e)))
}

/// Get one imported playlist, with tracks re-resolved against the current
/// local library (never stale — see `Database::get_imported_playlist`).
#[tauri::command]
pub async fn get_imported_playlist(
    state: tauri::State<'_, AppState>,
    playlist_id: String,
) -> CommandResult<Option<ImportedPlaylistDetail>> {
    state
        .db
        .get_imported_playlist(&playlist_id)
        .map_err(|e| CommandError::from(format!("Failed to read imported playlist: {}", e)))
}

/// Delete a locally-imported playlist.
#[tauri::command]
pub async fn delete_imported_playlist(
    state: tauri::State<'_, AppState>,
    playlist_id: String,
) -> CommandResult<()> {
    state
        .db
        .delete_imported_playlist(&playlist_id)
        .map_err(|e| CommandError::from(format!("Failed to delete imported playlist: {}", e)))
}
