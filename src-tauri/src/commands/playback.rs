//! Playback-related Tauri commands.
//!
//! Handles audio playback, queue management, and transport controls.
//! All commands communicate with the audio engine via the AudioEngineHandle.

use crate::audio::{query_mpv_devices, AudioCommand, AudioDevice, QueueInsertPosition, RepeatMode};
use crate::error::{CommandError, CommandResult};
use crate::models::{PlaybackState, QueueState, Track};
use crate::state::AppState;

/// Play a specific track.
///
/// Uses the cached file path if available (offline playback), otherwise
/// generates a proxy URL to stream from Jellyfin.
/// If `clear_queue` is true, clears the queue first.
#[tauri::command]
pub async fn play_track(
    state: tauri::State<'_, AppState>,
    track: Track,
    clear_queue: Option<bool>,
) -> CommandResult<()> {
    tauri_plugin_telemetry::traced_with_span(
        tracing::info_span!(
            target: tauri_plugin_telemetry::COMMAND_SPAN_TARGET,
            "ipc_command",
            command = "play_track",
            outcome = tracing::field::Empty,
            track_id = %track.jellyfin_id,
            track_name = %track.name,
        ),
        async move {
            tracing::info!("play_track command called for: {}", track.name);

            let stream_url = if let Some(ref path) = track.cached_file_path {
                tracing::debug!("Using cached file: {}", path);
                format!("file://{}", path)
            } else {
                let (proxy_port, proxy_token) = state.proxy_endpoint().await?;
                let jellyfin = state.media.read().await;
                let client = jellyfin
                    .as_ref()
                    .ok_or_else(|| "Jellyfin not configured and track is not cached".to_string())?;
                let url = client.get_stream_url(&track.jellyfin_id, proxy_port, &proxy_token);
                // Redact the `?tk=<token>` secret before logging.
                tracing::debug!("Stream URL: {}", crate::proxy::redact_stream_url(&url));
                url
            };

            state
                .get_audio()
                .await?
                .send(AudioCommand::Play {
                    track,
                    stream_url,
                    clear_queue: clear_queue.unwrap_or(true),
                })
                .map_err(CommandError::msg)
        },
    )
    .await
}

/// Play a full context (a Songs table, an album, a playlist, ...), replacing
/// the queue with `tracks` in order and starting playback at `start_index`.
///
/// The one mechanism every "play this row" entry point goes through: unlike
/// the old `play_track(clear_queue: true)` + `add_to_queue` two-call dance,
/// this resolves every stream URL up front and sends a single
/// `AudioCommand::PlayContext`, so the engine either has the whole context or
/// none of it — no window where the queue holds only the track that was
/// double-clicked — and tracks before `start_index` are kept too, so Previous
/// can navigate back into the context. `shuffle` is the caller's current
/// shuffle preference; the engine applies it while building the queue rather
/// than the caller issuing a separate `set_shuffle` afterwards, which would
/// broadcast an unshuffled queue and then a second, shuffled one (see
/// `AudioCommand::PlayContext`).
#[tauri::command]
pub async fn play_context(
    state: tauri::State<'_, AppState>,
    tracks: Vec<Track>,
    start_index: usize,
    shuffle: bool,
) -> CommandResult<()> {
    tauri_plugin_telemetry::traced_with_span(
        tracing::info_span!(
            target: tauri_plugin_telemetry::COMMAND_SPAN_TARGET,
            "ipc_command",
            command = "play_context",
            outcome = tracing::field::Empty,
            track_count = tracks.len(),
            start_index = start_index,
        ),
        async move {
            let needs_jellyfin = tracks.iter().any(|t| t.cached_file_path.is_none());
            let proxy_endpoint = if needs_jellyfin {
                Some(state.proxy_endpoint().await?)
            } else {
                None
            };
            let jellyfin = state.media.read().await;
            let client = if needs_jellyfin {
                Some(jellyfin.as_ref().ok_or_else(|| {
                    "Jellyfin not configured and some tracks are not cached".to_string()
                })?)
            } else {
                None
            };

            let tracks_with_urls: Vec<(Track, String)> = tracks
                .into_iter()
                .map(|track| -> Result<(Track, String), String> {
                    let url = if let Some(ref path) = track.cached_file_path {
                        format!("file://{}", path)
                    } else {
                        // needs_jellyfin (checked above) guarantees both are
                        // Some for any track reaching this branch, but the
                        // lookup itself stays fallible rather than trusting
                        // that invariant with an expect().
                        let (port, token) = proxy_endpoint
                            .as_ref()
                            .ok_or_else(|| "Local proxy not yet started".to_string())?;
                        client
                            .as_ref()
                            .ok_or_else(|| "Jellyfin not configured".to_string())?
                            .get_stream_url(&track.jellyfin_id, *port, token)
                    };
                    Ok((track, url))
                })
                .collect::<Result<Vec<_>, String>>()?;

            state
                .get_audio()
                .await?
                .send(AudioCommand::PlayContext {
                    tracks: tracks_with_urls,
                    start_index,
                    shuffle,
                })
                .map_err(CommandError::msg)
        },
    )
    .await
}

/// Pause playback.
#[tauri::command]
pub async fn pause(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    state
        .get_audio()
        .await?
        .send(AudioCommand::Pause)
        .map_err(CommandError::msg)
}

/// Resume playback.
#[tauri::command]
pub async fn resume(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    state
        .get_audio()
        .await?
        .send(AudioCommand::Resume)
        .map_err(CommandError::msg)
}

/// Seek to a position in the current track.
#[tauri::command]
pub async fn seek(state: tauri::State<'_, AppState>, position_ms: u64) -> CommandResult<()> {
    state
        .get_audio()
        .await?
        .send(AudioCommand::Seek(position_ms))
        .map_err(CommandError::msg)
}

/// Set the playback volume.
#[tauri::command]
pub async fn set_volume(state: tauri::State<'_, AppState>, volume: f32) -> CommandResult<()> {
    state
        .get_audio()
        .await?
        .send(AudioCommand::SetVolume(volume))
        .map_err(CommandError::msg)
}

/// Read a synchronous snapshot of playback and queue state from the engine.
///
/// The engine loop cycles at least every 50 ms, so a short timeout is ample.
/// Returns defaults when the audio engine is not yet initialised, so the
/// frontend can poll safely during startup.
async fn query_snapshot(state: &AppState) -> CommandResult<(PlaybackState, QueueState)> {
    let Ok(audio) = state.get_audio().await else {
        return Ok((PlaybackState::default(), QueueState::default()));
    };

    let (tx, rx) = std::sync::mpsc::channel();
    audio.send(AudioCommand::QuerySnapshot(tx))?;

    tokio::task::spawn_blocking(move || rx.recv_timeout(std::time::Duration::from_secs(2)))
        .await
        .map_err(|e| format!("Snapshot task failed: {}", e))?
        .map_err(|e| {
            CommandError::from(format!("Audio engine did not answer snapshot query: {}", e))
        })
}

/// Get the current playback state.
#[tauri::command]
pub async fn get_playback_state(state: tauri::State<'_, AppState>) -> CommandResult<PlaybackState> {
    let (playback, _) = query_snapshot(&state).await?;
    Ok(playback)
}

/// Get the current queue.
#[tauri::command]
pub async fn get_queue(state: tauri::State<'_, AppState>) -> CommandResult<QueueState> {
    let (_, queue) = query_snapshot(&state).await?;
    Ok(queue)
}

/// Add tracks to the queue.
#[tauri::command]
pub async fn add_to_queue(
    state: tauri::State<'_, AppState>,
    tracks: Vec<Track>,
    position: QueueInsertPosition,
) -> CommandResult<()> {
    // Resolve stream URLs: use cached file if available, otherwise proxy to Jellyfin.
    // We only acquire the Jellyfin client if at least one track needs it.
    let needs_jellyfin = tracks.iter().any(|t| t.cached_file_path.is_none());
    let proxy_endpoint = if needs_jellyfin {
        Some(state.proxy_endpoint().await?)
    } else {
        None
    };
    let jellyfin = state.media.read().await;
    let client =
        if needs_jellyfin {
            Some(jellyfin.as_ref().ok_or_else(|| {
                "Jellyfin not configured and some tracks are not cached".to_string()
            })?)
        } else {
            None
        };

    let tracks_with_urls: Vec<(Track, String)> = tracks
        .into_iter()
        .map(|track| {
            let url = if let Some(ref path) = track.cached_file_path {
                format!("file://{}", path)
            } else {
                let (port, token) = proxy_endpoint.as_ref().expect("endpoint checked above");
                client
                    .as_ref()
                    .expect("client checked above")
                    .get_stream_url(&track.jellyfin_id, *port, token)
            };
            (track, url)
        })
        .collect();

    state
        .get_audio()
        .await?
        .send(AudioCommand::AddToQueue {
            tracks: tracks_with_urls,
            position,
        })
        .map_err(CommandError::msg)
}

/// Clear the queue (keeps current track).
#[tauri::command]
pub async fn clear_queue(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    state
        .get_audio()
        .await?
        .send(AudioCommand::ClearQueue)
        .map_err(CommandError::msg)
}

/// Remove a track from the queue by index.
#[tauri::command]
pub async fn remove_from_queue(
    state: tauri::State<'_, AppState>,
    index: usize,
) -> CommandResult<()> {
    state
        .get_audio()
        .await?
        .send(AudioCommand::RemoveFromQueue(index))
        .map_err(CommandError::msg)
}

/// Reorder a track in the queue.
#[tauri::command]
pub async fn reorder_queue(
    state: tauri::State<'_, AppState>,
    from_index: usize,
    to_index: usize,
) -> CommandResult<()> {
    state
        .get_audio()
        .await?
        .send(AudioCommand::ReorderQueue {
            from: from_index,
            to: to_index,
        })
        .map_err(CommandError::msg)
}

/// Jump directly to a queue entry by index.
///
/// The Up Next panel's double-click/Return activation — the one
/// queue-navigation path alongside `skip_next`/`skip_previous`.
#[tauri::command]
pub async fn jump_to_queue_index(
    state: tauri::State<'_, AppState>,
    index: usize,
) -> CommandResult<()> {
    state
        .get_audio()
        .await?
        .send(AudioCommand::JumpToQueueIndex(index))
        .map_err(CommandError::msg)
}

/// Skip to the next track.
#[tauri::command]
pub async fn skip_next(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    state
        .get_audio()
        .await?
        .send(AudioCommand::SkipNext)
        .map_err(CommandError::msg)
}

/// Skip to the previous track.
#[tauri::command]
pub async fn skip_previous(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    state
        .get_audio()
        .await?
        .send(AudioCommand::SkipPrevious)
        .map_err(CommandError::msg)
}

/// Set shuffle mode.
#[tauri::command]
pub async fn set_shuffle(state: tauri::State<'_, AppState>, shuffle: bool) -> CommandResult<()> {
    state
        .get_audio()
        .await?
        .send(AudioCommand::SetShuffle(shuffle))
        .map_err(CommandError::msg)
}

/// Set repeat mode.
#[tauri::command]
pub async fn set_repeat(
    state: tauri::State<'_, AppState>,
    repeat: RepeatMode,
) -> CommandResult<()> {
    state
        .get_audio()
        .await?
        .send(AudioCommand::SetRepeat(repeat))
        .map_err(CommandError::msg)
}

/// Get available audio output devices.
///
/// Queries the running mpv instance for its device list, which returns
/// device identifiers compatible with `--audio-device=`.
#[tauri::command]
pub async fn get_output_devices() -> CommandResult<Vec<AudioDevice>> {
    let socket_path = crate::audio::mpv_process::get_socket_path();
    query_mpv_devices(&socket_path).map_err(CommandError::msg)
}

/// Set the audio output device.
///
/// Pass `None` to use the system default device.
#[tauri::command]
pub async fn set_output_device(
    state: tauri::State<'_, AppState>,
    device_name: Option<String>,
) -> CommandResult<()> {
    state
        .get_audio()
        .await?
        .send(AudioCommand::SetOutputDevice(device_name))
        .map_err(CommandError::msg)
}
