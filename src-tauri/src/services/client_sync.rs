//! Scrobble and skip sync endpoints for the music service client.
//!
//! Bulk sync operations combining scrobble import and skip recording.
//! Also provides listening history retrieval via the ListenBrainz-compatible API.

use url::Url;

use super::client::{transport_detail, MusicServiceClient, ScrobbleSyncOutcome};
use super::models::{ListenHistoryEntry, ListenHistoryResponse, ScrobbleRequest, SkipRequest};
use crate::error::AppError;

impl MusicServiceClient {
    /// Bulk sync scrobbles and skips using a cloned client (static version).
    ///
    /// Scrobbles are synced via the native client API (`POST /client/sync`).
    /// Skips are synced via `POST /client/skips` and will be silently
    /// skipped if the service doesn't support them.
    pub async fn sync_with_skips_with_client(
        http: &reqwest_middleware::ClientWithMiddleware,
        base_url: &Url,
        scrobbles: &[ScrobbleRequest],
        skips: &[SkipRequest],
    ) -> Result<(ScrobbleSyncOutcome, u32), AppError> {
        if scrobbles.is_empty() && skips.is_empty() {
            return Ok((ScrobbleSyncOutcome::default(), 0));
        }

        // Sync scrobbles via LB import
        let mut scrobble_outcome =
            Self::sync_scrobbles_with_client(http, base_url, scrobbles).await;
        if scrobble_outcome.error.is_some() {
            // The service is down or the token is bad: skips would fail the
            // same way, so they stay queued for the next drain.
            return Ok((scrobble_outcome, 0));
        }

        // Sync skips via the scrobble server's extension (best-effort). The returned
        // count is a PREFIX length over `skips` — the caller marks exactly
        // that many, in order, as drained — so every processed skip must
        // count, and the loop must only stop early on a transient failure.
        let mut total_skips_synced: u32 = 0;
        for skip in skips {
            match Self::record_skip_with_client(http, base_url, skip).await {
                // Recorded, duplicate, or unsupported by the service — all
                // drained: none of these can succeed differently on retry.
                Ok(_) => total_skips_synced += 1,
                Err(AppError::InvalidOperation(msg)) => {
                    // Permanently rejected: retrying the same payload would
                    // wedge the drain forever. Drop it and move on.
                    tracing::warn!("Dropping rejected skip: {}", msg);
                    total_skips_synced += 1;
                }
                Err(AppError::Network(e)) => {
                    // Transient failure: leave the remainder queued for the
                    // next drain.
                    tracing::debug!(
                        "Skip sync interrupted ({}); {} remaining stay queued",
                        e,
                        skips.len() - total_skips_synced as usize
                    );
                    break;
                }
                Err(e) => {
                    // Auth or the like: the scrobbles already accepted still
                    // report as drained beside the error.
                    scrobble_outcome.error = Some(e);
                    break;
                }
            }
        }

        Ok((scrobble_outcome, total_skips_synced))
    }

    /// Get listening history using a cloned client (static version).
    ///
    /// Uses the ListenBrainz `/1/user/{user_name}/listens` endpoint.
    /// The user_name "bragi" is used as a placeholder (single-user model).
    pub async fn get_history_with_client(
        http: &reqwest_middleware::ClientWithMiddleware,
        base_url: &Url,
        limit: u32,
        _offset: u32,
    ) -> Result<ListenHistoryResponse, AppError> {
        let mut url = Self::endpoint_from_base(base_url, "1/user/bragi/listens")?;
        url.query_pairs_mut()
            .append_pair("count", &limit.to_string());

        let response = http.get(url).send().await.map_err(|e| {
            AppError::Network(format!("Failed to get history: {}", transport_detail(&e)))
        })?;

        if response.status().is_success() {
            let lb_response: serde_json::Value = response.json().await.map_err(|e| {
                AppError::Network(format!(
                    "Failed to parse response: {}",
                    transport_detail(&e)
                ))
            })?;

            let empty = vec![];
            let listens = lb_response["payload"]["listens"]
                .as_array()
                .unwrap_or(&empty);

            let items: Vec<ListenHistoryEntry> = listens
                .iter()
                .filter_map(|listen| {
                    let metadata = &listen["track_metadata"];
                    let additional = &metadata["additional_info"];
                    Some(ListenHistoryEntry {
                        track_mbid: additional["recording_mbid"].as_str().map(|s| s.to_string()),
                        artist_name: metadata["artist_name"].as_str()?.to_string(),
                        track_name: metadata["track_name"].as_str()?.to_string(),
                        album_name: metadata["release_name"].as_str().map(|s| s.to_string()),
                        listened_at: chrono::DateTime::from_timestamp(
                            listen["listened_at"].as_i64().unwrap_or(0),
                            0,
                        )
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_default(),
                        duration_ms: additional["duration_ms"].as_u64().unwrap_or(0),
                        jellyfin_id: None,
                    })
                })
                .collect();

            let count = lb_response["payload"]["count"].as_u64().unwrap_or(0);

            Ok(ListenHistoryResponse {
                total: count,
                items,
                limit,
                offset: 0,
            })
        } else if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            Err(AppError::Auth("Invalid API token".to_string()))
        } else {
            Err(AppError::Network(format!(
                "Get history failed with status {}",
                response.status()
            )))
        }
    }
}
