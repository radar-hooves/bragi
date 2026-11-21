//! Data models for the music service API.
//!
//! Request and response types for communicating with the scrobble server's native client API.
//! Includes conversion to ListenBrainz wire format as fallback.

use serde::{Deserialize, Serialize};

/// Request payload for recording a scrobble.
///
/// Submitted via the scrobble server's `POST /client/sync`; converted to
/// ListenBrainz `import` wire format via `to_lb_import()` for the
/// `/1/submit-listens` path on any other ListenBrainz-compatible service.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScrobbleRequest {
    /// MusicBrainz track ID (canonical identifier).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_mbid: Option<String>,

    /// Artist name (fallback display).
    pub artist_name: String,

    /// Track name (fallback display).
    pub track_name: String,

    /// Album name (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album_name: Option<String>,

    /// Timestamp when the track was listened to (ISO 8601).
    pub listened_at: String,

    /// Duration of playback in milliseconds.
    pub duration_ms: u64,

    /// Device identifier for this installation.
    pub device_id: String,

    /// Jellyfin item ID for correlation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jellyfin_id: Option<String>,

    /// MusicBrainz artist ID (for enrichment).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist_mbid: Option<String>,

    /// MusicBrainz release ID (for album art on dashboard).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_mbid: Option<String>,
}

impl ScrobbleRequest {
    /// Convert a batch of scrobbles to a ListenBrainz `submit-listens` payload
    /// with `listen_type: "import"`.
    ///
    /// Batch conversion for `POST /1/submit-listens` with `listen_type: "import"`.
    pub fn to_lb_import(scrobbles: &[ScrobbleRequest]) -> serde_json::Value {
        let payload: Vec<serde_json::Value> = scrobbles
            .iter()
            .map(|s| {
                let listened_at_ts = chrono::DateTime::parse_from_rfc3339(&s.listened_at)
                    .map(|dt| dt.timestamp())
                    .unwrap_or(0);

                let mut additional_info = serde_json::json!({
                    "submission_client": "Bragi",
                    "media_player": "Bragi",
                    "duration_ms": s.duration_ms,
                    "device_id": s.device_id,
                });

                if let Some(ref mbid) = s.track_mbid {
                    additional_info["recording_mbid"] = serde_json::Value::String(mbid.clone());
                }
                if let Some(ref mbid) = s.artist_mbid {
                    additional_info["artist_mbid"] = serde_json::Value::String(mbid.clone());
                }
                if let Some(ref mbid) = s.release_mbid {
                    additional_info["release_mbid"] = serde_json::Value::String(mbid.clone());
                }

                serde_json::json!({
                    "listened_at": listened_at_ts,
                    "track_metadata": {
                        "artist_name": s.artist_name,
                        "track_name": s.track_name,
                        "release_name": s.album_name,
                        "additional_info": additional_info,
                    }
                })
            })
            .collect();

        serde_json::json!({
            "listen_type": "import",
            "payload": payload,
        })
    }
}

/// Response from the bulk sync endpoint (`POST /client/sync`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResponse {
    /// Number of scrobbles successfully synced.
    pub synced: u32,

    /// Number of duplicate scrobbles skipped.
    pub duplicates: u32,

    /// Number of skips successfully synced.
    pub skips_synced: u32,

    /// Number of duplicate skips skipped.
    pub skips_duplicates: u32,

    /// Number of scrobbles the scrobble server refused to store because they carried no
    /// MusicBrainz recording ID. `#[serde(default)]` so an older scrobble server
    /// (pre-honest-reject) that omits the field deserialises to 0.
    ///
    /// Skips have no equivalent field here: Bragi always sends `"skips": []`
    /// to `/client/sync` (skips go via the separate `/client/skips` endpoint,
    /// one at a time), so a `skips_rejected_no_mbid` the server might report
    /// on this response is always meaningless on Bragi's side. Serde ignores
    /// the extra field if the server sends it.
    #[serde(default)]
    pub rejected_no_mbid: u32,
}

/// Request payload for recording a skip.
///
/// Sent to `POST /client/skips`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SkipRequest {
    /// MusicBrainz track ID (canonical identifier).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_mbid: Option<String>,

    /// Artist name (fallback display).
    pub artist_name: String,

    /// Track name (fallback display).
    pub track_name: String,

    /// Album name (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album_name: Option<String>,

    /// Timestamp when the track was skipped (ISO 8601).
    pub skipped_at: String,

    /// Duration played before skip in milliseconds.
    pub played_ms: u64,

    /// Total track duration in milliseconds.
    pub track_duration_ms: u64,

    /// Device identifier for this installation.
    pub device_id: String,

    /// Jellyfin item ID for correlation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jellyfin_id: Option<String>,

    /// MusicBrainz artist ID (for enrichment).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist_mbid: Option<String>,

    /// MusicBrainz release ID (for album art on dashboard).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_mbid: Option<String>,
}

/// Response from the skip endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SkipResponse {
    /// Whether the skip was accepted.
    pub success: bool,

    /// Optional message (e.g., "duplicate" if already recorded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// A single entry in listening history.
///
/// Serialised to the frontend as camelCase. Can be populated from either
/// the ListenBrainz `/1/user/{user_name}/listens` response or the scrobble server's
/// client API. Uses aliases to accept snake_case from APIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenHistoryEntry {
    /// MusicBrainz track ID.
    #[serde(alias = "track_mbid", skip_serializing_if = "Option::is_none")]
    pub track_mbid: Option<String>,

    /// Artist name.
    #[serde(alias = "artist_name")]
    pub artist_name: String,

    /// Track name.
    #[serde(alias = "track_name")]
    pub track_name: String,

    /// Album name.
    #[serde(alias = "album_name", skip_serializing_if = "Option::is_none")]
    pub album_name: Option<String>,

    /// When the track was listened to (ISO 8601).
    #[serde(alias = "listened_at")]
    pub listened_at: String,

    /// Duration played in milliseconds.
    #[serde(alias = "duration_ms")]
    pub duration_ms: u64,

    /// Jellyfin ID for artwork/correlation.
    #[serde(alias = "jellyfin_id", skip_serializing_if = "Option::is_none")]
    pub jellyfin_id: Option<String>,
}

/// Response from the listening history endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListenHistoryResponse {
    /// List of recent listens.
    pub items: Vec<ListenHistoryEntry>,

    /// Total number of listens (for pagination).
    pub total: u64,

    /// Number of items requested.
    pub limit: u32,

    /// Offset from start.
    pub offset: u32,
}

/// Sync status for the UI.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    /// Whether the music service is configured.
    pub is_configured: bool,

    /// Whether we can reach the service.
    pub is_online: bool,

    /// Number of pending scrobbles in the queue.
    pub pending_count: u32,

    /// Timestamp of last successful sync (Unix seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync_at: Option<i64>,

    /// Error message if last sync failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

// ============================================================================
// Track Stats Models
// ============================================================================

/// Request payload for fetching bulk track stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkTrackStatsRequest {
    /// MusicBrainz track IDs to fetch stats for.
    pub track_mbids: Vec<String>,
}

/// A single track's stats from the bulk stats endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TrackStatsEntry {
    /// Total number of listens.
    pub play_count: u32,
    /// Total number of skips.
    pub skip_count: u32,
    /// When the track was last played (ISO 8601).
    pub last_played_at: Option<String>,
    /// When the track was last skipped (ISO 8601).
    pub last_skipped_at: Option<String>,
}

/// Response from the bulk track stats endpoint (`POST /api/stats/tracks/bulk`).
#[derive(Debug, Clone, Deserialize)]
pub struct BulkTrackStatsResponse {
    /// Map of track MBID to stats.
    pub tracks: std::collections::HashMap<String, TrackStatsEntry>,
}
