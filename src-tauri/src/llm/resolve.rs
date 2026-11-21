//! Resolve LLM-selected candidate indices back to real library tracks.
//!
//! The model returns indices into the candidate list, so resolution is a direct,
//! hallucination-proof lookup — out-of-range or duplicate indices are simply
//! dropped, and the counts drive the UI's "matched N of M" figure.

use std::collections::HashSet;

use crate::models::Track;

/// The result of resolving an LLM response against the candidate list.
pub struct Resolution {
    /// The resolved tracks, in the order the model chose, de-duplicated.
    pub tracks: Vec<Track>,
    /// How many indices the model proposed (before dropping invalid/duplicate).
    pub suggested_count: usize,
}

/// Map the model's chosen indices onto candidate tracks.
pub fn resolve_indices(candidates: &[Track], indices: &[i64]) -> Resolution {
    let mut tracks = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for &idx in indices {
        if idx < 0 {
            continue;
        }
        if let Some(track) = candidates.get(idx as usize) {
            if seen.insert(track.jellyfin_id.as_str()) {
                tracks.push(track.clone());
            }
        }
    }
    Resolution {
        tracks,
        suggested_count: indices.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            jellyfin_id: id.to_string(),
            mbid: None,
            artist_mbid: None,
            release_mbid: None,
            name: id.to_string(),
            artist_name: "A".to_string(),
            album_name: "Al".to_string(),
            album_jellyfin_id: "alb".to_string(),
            track_number: 1,
            disc_number: 1,
            duration_ms: 1000,
            genre: None,
            year: None,
            artwork_url: None,
            cached_file_path: None,
            file_size_bytes: None,
            normalization_gain_db: None,
        }
    }

    #[test]
    fn resolves_in_order_dropping_invalid_and_dupes() {
        let candidates = vec![track("a"), track("b"), track("c")];
        // 2 valid (order preserved), an out-of-range 9, a duplicate 0, a negative.
        let r = resolve_indices(&candidates, &[2, 0, 9, 0, -1]);
        assert_eq!(
            r.tracks
                .iter()
                .map(|t| t.jellyfin_id.as_str())
                .collect::<Vec<_>>(),
            vec!["c", "a"]
        );
        assert_eq!(r.suggested_count, 5);
    }
}
