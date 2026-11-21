//! Track stats fetching from the music service.
//!
//! Bulk retrieval of play counts, skip counts, and timestamps
//! from the scrobble server's `/client/stats/tracks/bulk` endpoint.

use std::collections::HashMap;
use url::Url;

use super::client::{transport_detail, MusicServiceClient};
use super::models::{BulkTrackStatsRequest, BulkTrackStatsResponse, TrackStatsEntry};
use crate::error::AppError;

/// Maximum MBIDs per API request (the server enforces 500).
const BATCH_SIZE: usize = 500;

impl MusicServiceClient {
    /// Fetch bulk track stats using a cloned client (static version).
    ///
    /// Chunks requests into batches of 500 MBIDs and merges results.
    pub async fn fetch_bulk_track_stats_with_client(
        http: &reqwest_middleware::ClientWithMiddleware,
        base_url: &Url,
        track_mbids: &[String],
    ) -> Result<HashMap<String, TrackStatsEntry>, AppError> {
        if track_mbids.is_empty() {
            return Ok(HashMap::new());
        }

        let url = Self::endpoint_from_base(base_url, "client/stats/tracks/bulk")?;
        let mut all_stats: HashMap<String, TrackStatsEntry> = HashMap::new();

        for chunk in track_mbids.chunks(BATCH_SIZE) {
            let request = BulkTrackStatsRequest {
                track_mbids: chunk.to_vec(),
            };

            let response = http
                .post(url.clone())
                .json(&request)
                .send()
                .await
                .map_err(|e| {
                    AppError::Network(format!(
                        "Failed to fetch track stats: {}",
                        transport_detail(&e)
                    ))
                })?;

            if response.status().is_success() {
                let body = response.text().await.map_err(|e| {
                    AppError::Network(format!(
                        "Failed to read track stats response: {}",
                        transport_detail(&e)
                    ))
                })?;

                let result: BulkTrackStatsResponse = serde_json::from_str(&body).map_err(|e| {
                    tracing::debug!(
                        "Track stats response body (first 500 chars): {}",
                        &body[..body.len().min(500)]
                    );
                    AppError::Network(format!("Failed to parse track stats response: {}", e))
                })?;
                all_stats.extend(result.tracks);
            } else if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                return Err(AppError::Auth("Invalid API token".to_string()));
            } else {
                return Err(AppError::Network(format!(
                    "Track stats request failed with status {}",
                    response.status()
                )));
            }
        }

        Ok(all_stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stats_endpoint_url() {
        let client = MusicServiceClient::new("https://music.example.com", "test-token").unwrap();
        let url =
            MusicServiceClient::endpoint_from_base(client.base_url(), "client/stats/tracks/bulk")
                .unwrap();
        assert_eq!(
            url.as_str(),
            "https://music.example.com/client/stats/tracks/bulk"
        );
    }
}
