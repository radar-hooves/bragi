//! Intent-driven track selection for AI playlists.
//!
//! The LLM first extracts a structured [`PlaylistIntent`] from the natural-
//! language prompt (artists, genres, year range, sort, count, and whether the
//! request is a subjective "vibe"). This module then executes that intent as a
//! real query over the library — filtering by artist/genre/year, joining
//! the scrobble server's play counts, and sorting — so "Trivium's top 10 most played" actually returns
//! Trivium's tracks by play-count, not a random sample. Only genuinely
//! subjective prompts fall through to LLM curation of the filtered pool.

use crate::db::{Database, LibraryViewParams};
use crate::error::AppError;
use crate::models::Track;

/// Hard cap on tracks handed to the LLM for subjective curation (token budget).
pub const MAX_CANDIDATES: usize = 500;

/// A structured playlist request extracted from the prompt by the LLM.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlaylistIntent {
    /// A short playlist name the model suggested.
    pub name: String,
    /// Artist names mentioned in the request (resolved against the library).
    pub artists: Vec<String>,
    /// Genres mentioned (matched against the library's genres).
    pub genres: Vec<String>,
    /// Inclusive release-year range (e.g. a decade), if any.
    #[serde(deserialize_with = "de_opt_u32", default)]
    pub year_min: Option<u32>,
    #[serde(deserialize_with = "de_opt_u32", default)]
    pub year_max: Option<u32>,
    /// Ordering: `most_played` | `least_played` | `recently_played` | `none`.
    pub sort: String,
    /// Number of tracks requested, if the user specified one.
    #[serde(deserialize_with = "de_opt_u32", default)]
    pub count: Option<u32>,
    /// True when the request is about mood/feel and needs subjective song
    /// selection (curation); false for precise filters like "top N by X".
    pub curate: bool,
}

/// Deserialize an optional `u32` tolerantly: accept null, an integer, a
/// whole-ish float, or a numeric string — so a model that emits `10.0` or `"10"`
/// for `count` doesn't fail the whole struct and collapse the intent to defaults.
fn de_opt_u32<'de, D>(d: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let v = Option::<serde_json::Value>::deserialize(d)?;
    Ok(v.and_then(|v| match v {
        serde_json::Value::Number(n) => n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u32),
        serde_json::Value::String(s) => s
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|f| *f >= 0.0)
            .map(|f| f as u32),
        _ => None,
    }))
}

/// Select and order the library tracks that satisfy an intent. Returns the full
/// ordered pool (the caller applies the count / curation). An empty result means
/// nothing in the library matched (e.g. an artist the user doesn't own).
pub fn select_tracks(db: &Database, intent: &PlaylistIntent) -> Result<Vec<Track>, AppError> {
    // Resolve artist mentions to exact library artist names.
    let resolved_artists = if intent.artists.is_empty() {
        None
    } else {
        let library = db.distinct_artists().map_err(AppError::from)?;
        let matched = resolve_artists(&intent.artists, &library);
        if matched.is_empty() {
            // The user named artists we don't have — return nothing rather than
            // silently widening to the whole library.
            return Ok(Vec::new());
        }
        Some(matched)
    };

    // Resolve genre mentions to exact library genres.
    let resolved_genres = if intent.genres.is_empty() {
        None
    } else {
        let library = db.distinct_genres().map_err(AppError::from)?;
        let matched = resolve_genres(&intent.genres, &library);
        if matched.is_empty() {
            None
        } else {
            Some(matched)
        }
    };

    let params = LibraryViewParams {
        artists: resolved_artists,
        genres: resolved_genres,
        ..Default::default()
    };
    let mut pool = db.query_tracks(&params).map_err(AppError::from)?;

    // Year range (the query layer has no year filter).
    if intent.year_min.is_some() || intent.year_max.is_some() {
        let lo = intent.year_min.unwrap_or(0);
        let hi = intent.year_max.unwrap_or(u32::MAX);
        pool.retain(|t| t.year.map(|y| y >= lo && y <= hi).unwrap_or(false));
    }

    // Sorting (which needs play-count data whose source is a user setting) is
    // applied by the caller via [`order_pool`]; here we only filter.
    Ok(pool)
}

/// A resolved play metric for a track, from whichever source(s) the caller chose.
#[derive(Clone, Copy, Default)]
pub struct PlayMetric {
    pub play_count: i64,
    pub last_played: i64,
}

/// Order the pool in place per the sort key, using a per-track metric supplied
/// by the caller (so the play-count source stays a caller concern). `none`
/// shuffles so repeat generations vary.
pub fn order_pool<F: Fn(&Track) -> PlayMetric>(pool: &mut [Track], sort: &str, metric: F) {
    match sort {
        "most_played" => pool.sort_by_key(|t| std::cmp::Reverse(metric(t).play_count)),
        "least_played" => pool.sort_by_key(|t| metric(t).play_count),
        "recently_played" => pool.sort_by_key(|t| std::cmp::Reverse(metric(t).last_played)),
        _ => shuffle(pool),
    }
}

/// Whether a sort key needs play-count data (vs. a shuffle).
pub fn sort_needs_play_data(sort: &str) -> bool {
    matches!(sort, "most_played" | "least_played" | "recently_played")
}

/// Normalise a name for comparison: lowercase, trimmed, leading "the " dropped
/// (so "The Beatles" == "Beatles").
fn normalize(s: &str) -> String {
    let s = s.trim().to_lowercase();
    s.strip_prefix("the ").map(str::to_string).unwrap_or(s)
}

/// Resolve requested artist names to owned library artists by **normalised exact
/// match** only — a substring match would let a short owned name ("Air") swallow
/// a longer unowned request ("Air Supply") and defeat the "you don't own that
/// artist" guard. Collects every match.
fn resolve_artists(requested: &[String], library: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for want in requested {
        let wl = normalize(want);
        if wl.is_empty() {
            continue;
        }
        for lib in library {
            if normalize(lib) == wl && !out.contains(lib) {
                out.push(lib.clone());
            }
        }
    }
    out
}

/// Resolve requested genres to owned library genres by substring match either
/// way, so "metal" pulls in "Heavy Metal", "Metalcore", etc. Collects every
/// match (a broad net is desired for genres, unlike artists).
fn resolve_genres(requested: &[String], library: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for want in requested {
        let wl = want.trim().to_lowercase();
        if wl.is_empty() {
            continue;
        }
        for lib in library {
            let ll = lib.to_lowercase();
            if (ll.contains(&wl) || wl.contains(&ll)) && !out.contains(lib) {
                out.push(lib.clone());
            }
        }
    }
    out
}

/// In-place time-seeded Fisher–Yates shuffle (no `rand` dependency).
fn shuffle(pool: &mut [Track]) {
    if pool.len() < 2 {
        return;
    }
    let mut state = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9e37_79b9_7f4a_7c15)
        | 1;
    for i in (1..pool.len()).rev() {
        // xorshift64
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let j = (state % (i as u64 + 1)) as usize;
        pool.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_artists_by_exact_match_only() {
        let library = vec![
            "Trivium".to_string(),
            "The Beatles".to_string(),
            "Air".to_string(),
        ];
        // Case-insensitive exact.
        assert_eq!(
            resolve_artists(&["trivium".to_string()], &library),
            vec!["Trivium"]
        );
        // Leading "the" is normalised on both sides.
        assert_eq!(
            resolve_artists(&["beatles".to_string()], &library),
            vec!["The Beatles"]
        );
        // The over-match guard: "Air Supply" (unowned) must NOT resolve to "Air".
        assert!(resolve_artists(&["Air Supply".to_string()], &library).is_empty());
        assert!(resolve_artists(&["Nonexistent".to_string()], &library).is_empty());
    }

    #[test]
    fn resolves_genres_by_substring_collecting_all() {
        let library = vec![
            "Heavy Metal".to_string(),
            "Metalcore".to_string(),
            "Jazz".to_string(),
        ];
        // "metal" pulls in every metal-ish genre, not just the first.
        assert_eq!(
            resolve_genres(&["metal".to_string()], &library),
            vec!["Heavy Metal", "Metalcore"]
        );
        assert!(resolve_genres(&["classical".to_string()], &library).is_empty());
    }
}
