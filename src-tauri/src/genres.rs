//! Multi-value genre tag normalisation.
//!
//! Jellyfin only splits a multi-value genre frame on delimiters the library
//! options declare, so an unconfigured server returns whole tag strings —
//! `["Alternative Metal;Heavy Metal;Rock"]` — as a single array element. Bragi
//! owns its view of the library, so it splits client-side rather than asking
//! anyone to reconfigure their server.

/// One genre membership: the key everything matches on, and the casing shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Genre {
    /// Case-folded, whitespace-collapsed matching key.
    pub key: String,
    /// The casing first seen for this key, for display.
    pub display: String,
}

/// The only delimiter split by default.
///
/// `,` and `/` are deliberately NOT split: they produce false splits on
/// legitimate genre names — `Folk, World, & Country`, `R&B/Soul`.
const DELIMITER: char = ';';

/// Collapse internal whitespace and trim.
fn tidy(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a token is an ID3v1 numeric genre code rather than a genre name.
///
/// Some taggers leave the raw code behind (`"17"` for Rock). It carries no
/// meaning to a reader, so it is dropped rather than listed as a genre.
fn is_numeric_code(token: &str) -> bool {
    !token.is_empty() && token.chars().all(|c| c.is_ascii_digit())
}

/// Normalise a genre name into its matching key.
pub fn genre_key(name: &str) -> String {
    tidy(name).to_lowercase()
}

/// Split a media server's whole `Genres` array into distinct genre memberships.
///
/// Consumes every element (not just the first), splits each on `;`, drops empty
/// and purely-numeric tokens, and dedupes case-insensitively while keeping the
/// casing first seen. Order is the order of first appearance, so a re-sync of
/// unchanged tags produces an identical result.
pub fn split_genres(raw: &[String]) -> Vec<Genre> {
    let mut out: Vec<Genre> = Vec::new();

    for element in raw {
        for token in element.split(DELIMITER) {
            let name = tidy(token);
            if name.is_empty() {
                continue;
            }
            if is_numeric_code(&name) {
                tracing::debug!(genre = %name, "Dropping numeric genre code");
                continue;
            }
            let key = name.to_lowercase();
            if out.iter().any(|g| g.key == key) {
                continue;
            }
            out.push(Genre { key, display: name });
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(raw: &[&str]) -> Vec<String> {
        split_genres(&raw.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            .into_iter()
            .map(|g| g.key)
            .collect()
    }

    fn displays(raw: &[&str]) -> Vec<String> {
        split_genres(&raw.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            .into_iter()
            .map(|g| g.display)
            .collect()
    }

    #[test]
    fn splits_a_semicolon_delimited_tag_string() {
        assert_eq!(
            displays(&["Alternative Metal;Heavy Metal;Rock"]),
            vec!["Alternative Metal", "Heavy Metal", "Rock"]
        );
    }

    #[test]
    fn consumes_every_array_element_not_just_the_first() {
        assert_eq!(
            displays(&["Death Metal;Rock", "Ambient"]),
            vec!["Death Metal", "Rock", "Ambient"]
        );
    }

    #[test]
    fn does_not_split_commas_or_slashes() {
        assert_eq!(
            displays(&["Folk, World, & Country"]),
            vec!["Folk, World, & Country"]
        );
        assert_eq!(displays(&["R&B/Soul"]), vec!["R&B/Soul"]);
    }

    #[test]
    fn trims_and_collapses_whitespace() {
        assert_eq!(
            displays(&["  Heavy   Metal ; Rock  "]),
            vec!["Heavy Metal", "Rock"]
        );
    }

    #[test]
    fn drops_empty_tokens() {
        assert_eq!(displays(&[";;Rock;;"]), vec!["Rock"]);
        assert!(displays(&["   "]).is_empty());
    }

    #[test]
    fn drops_purely_numeric_id3v1_codes() {
        assert_eq!(displays(&["17;Rock"]), vec!["Rock"]);
        // A genre whose name merely contains digits survives.
        assert_eq!(displays(&["2 Step"]), vec!["2 Step"]);
    }

    #[test]
    fn case_folds_for_matching_and_keeps_the_first_casing_for_display() {
        let g = split_genres(&["Nu Metal;nu metal;NU METAL".to_string()]);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].key, "nu metal");
        assert_eq!(g[0].display, "Nu Metal");
    }

    #[test]
    fn dedupes_within_a_track_across_array_elements() {
        assert_eq!(keys(&["Rock;Metal", "rock"]), vec!["rock", "metal"]);
    }

    #[test]
    fn is_stable_across_repeated_calls() {
        let raw = vec!["Death Metal;Melodic Death Metal;Rock".to_string()];
        assert_eq!(split_genres(&raw), split_genres(&raw));
    }

    #[test]
    fn genre_key_matches_the_split_key() {
        assert_eq!(genre_key("  Heavy   METAL "), "heavy metal");
    }

    #[test]
    fn empty_input_yields_no_genres() {
        assert!(split_genres(&[]).is_empty());
    }
}
