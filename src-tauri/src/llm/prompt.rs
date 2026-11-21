//! Playlist-curation prompt construction.
//!
//! The LLM is shown a NUMBERED candidate list and asked to return the indices of
//! the tracks to include — selecting from a supplied list rather than recalling
//! tracks structurally prevents hallucinated/nonexistent tracks.

use crate::models::Track;
use serde_json::{json, Value};

/// System prompt for the first pass: turn the request into a structured query.
/// The library's genres are supplied so the model only picks genres that exist.
pub fn intent_system_prompt(genres: &[String]) -> String {
    let genre_list = if genres.is_empty() {
        "(none)".to_string()
    } else {
        genres.join(", ")
    };
    format!(
        "You convert a music playlist request into a structured query over the user's library.\n\
         Extract:\n\
         - name: a short, fitting playlist name.\n\
         - artists: array of artist/band names explicitly named in the request (e.g. [\"Trivium\"]); [] if none.\n\
         - genres: array of genres, chosen ONLY from the user's library genres listed below; [] if none apply.\n\
         - yearMin / yearMax: inclusive release-year bounds if a decade or era is mentioned (e.g. \"2000s\" -> 2000 and 2009), else null.\n\
         - sort: \"most_played\" if they ask for top / most-played / favourite tracks; \"least_played\"; \"recently_played\" for recent; otherwise \"none\".\n\
         - count: the number of tracks requested (e.g. \"top 10\" -> 10), else null.\n\
         - curate: true ONLY if the request is about mood/feel/vibe needing subjective song choice (e.g. \"songs for a rainy afternoon\"); false for precise filters (e.g. \"top 10 Trivium songs\", \"20 jazz songs\").\n\
         Respond with JSON only.\n\n\
         Library genres: {genre_list}"
    )
}

/// JSON schema for the extracted [`PlaylistIntent`].
pub fn intent_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "artists": { "type": "array", "items": { "type": "string" } },
            "genres": { "type": "array", "items": { "type": "string" } },
            "yearMin": { "type": ["integer", "null"] },
            "yearMax": { "type": ["integer", "null"] },
            "sort": { "type": "string", "enum": ["most_played", "least_played", "recently_played", "none"] },
            "count": { "type": ["integer", "null"] },
            "curate": { "type": "boolean" }
        },
        "required": ["name", "artists", "genres", "yearMin", "yearMax", "sort", "count", "curate"],
        "additionalProperties": false
    })
}

/// Built-in curation system prompt (used when the user hasn't set a custom one).
pub const DEFAULT_SYSTEM_PROMPT: &str = "You are an expert music curator. From the numbered \
candidate tracks, choose the ones that best match the user's request and arrange them into a \
coherent playlist with good flow. Select ONLY from the candidates — never invent tracks. Prefer \
15-30 tracks, fewer if only a few genuinely fit. Give the playlist a short, fitting name.";

/// The output-format instruction appended to whichever system prompt is used, so
/// a custom prompt can't accidentally drop the response contract.
const FORMAT_INSTRUCTION: &str = "Respond with a JSON object only: \
{\"name\": \"<playlist name>\", \"indices\": [<candidate numbers in play order>]}. \
Use only candidate numbers that appear in the list.";

/// Build the system prompt, preferring the user's custom prompt when non-empty.
pub fn system_prompt(custom: &str) -> String {
    let base = if custom.trim().is_empty() {
        DEFAULT_SYSTEM_PROMPT
    } else {
        custom.trim()
    };
    format!("{base}\n\n{FORMAT_INSTRUCTION}")
}

/// Serialise the candidate tracks as a compact numbered list for the model.
pub fn candidate_list(tracks: &[Track]) -> String {
    let mut out = String::with_capacity(tracks.len() * 48);
    for (i, t) in tracks.iter().enumerate() {
        let year = t.year.map(|y| format!(" ({y})")).unwrap_or_default();
        let genre = t
            .genre
            .as_deref()
            .filter(|g| !g.is_empty())
            .map(|g| format!(" [{g}]"))
            .unwrap_or_default();
        let mins = t.duration_ms / 60_000;
        let secs = (t.duration_ms % 60_000) / 1000;
        out.push_str(&format!(
            "{i} | {} \u{2014} {} \u{2014} {}{year}{genre} {mins}:{secs:02}\n",
            t.name, t.artist_name, t.album_name,
        ));
    }
    out
}

/// Build the user message: the candidate list followed by the request.
pub fn user_message(request: &str, candidates: &str) -> String {
    format!(
        "Candidate tracks (index | Title \u{2014} Artist \u{2014} Album (Year) [Genre] mm:ss):\n\
         {candidates}\n\
         User request: {request}"
    )
}

/// JSON schema for the structured `{ name, indices }` response.
pub fn response_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "indices": { "type": "array", "items": { "type": "integer", "minimum": 0 } }
        },
        "required": ["name", "indices"],
        "additionalProperties": false
    })
}
