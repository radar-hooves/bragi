//! The one music sort order, shared by every `ORDER BY` in `db/*.rs` and (via
//! the mirrored `src/lib/utils/ordering.ts`) by every frontend comparator.
//!
//! Two SQLite collations are registered per connection ([`register_collations`]):
//! `BRAGI_FOLD` for a plain name/title, `BRAGI_ARTIST` for an artist name (also
//! strips a leading "The"/"A"/"An"). Both fold case and accents and sort digit
//! runs numerically, matching Apple Music/Spotify convention.

use std::cmp::Ordering;

use rusqlite::Connection;
use unicode_normalization::UnicodeNormalization;

/// Case- and accent-fold a string for comparison: NFD-decompose, drop the
/// combining marks that decomposition split off (the standard accent-folding
/// idiom — matches the frontend's `s.normalize('NFD').replace(/[̀-ͯ]/g, '')`),
/// then lowercase.
pub fn fold(s: &str) -> String {
    s.nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_combining_mark(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF)
}

/// Drop a leading "The "/"An "/"A " (case-insensitive) for sort purposes only
/// — the display name is never touched. Only "The" is directly confirmed by
/// an Apple source; "A"/"An" are a documented community-answer extension, not
/// an Apple one (unconfirmed beyond that).
pub fn strip_leading_article(s: &str) -> &str {
    let trimmed = s.trim_start();
    for article in ["the ", "an ", "a "] {
        if trimmed.len() > article.len()
            && trimmed
                .get(..article.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(article))
        {
            return &trimmed[article.len()..];
        }
    }
    s
}

/// Strip leading zeros off a digit run, keeping the last one for an all-zero
/// run ("000" -> "0") so the result is never empty.
fn strip_leading_zeros(digits: &str) -> &str {
    let trimmed = digits.trim_start_matches('0');
    if trimmed.is_empty() {
        &digits[digits.len() - 1..]
    } else {
        trimmed
    }
}

/// Compare two already-folded strings, treating a run of ASCII digits as one
/// number rather than comparing digit-by-digit (so "2" sorts before "10", and
/// "07" sorts by its magnitude — 7 — not by having one more character than "7").
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        return match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(ca), Some(cb)) if ca.is_ascii_digit() && cb.is_ascii_digit() => {
                let na = take_digits(&mut ai);
                let nb = take_digits(&mut bi);
                let ma = strip_leading_zeros(&na);
                let mb = strip_leading_zeros(&nb);
                // Compares as arbitrary-precision-ish via string length first,
                // since a track/year/track-number digit run never approaches
                // u128::MAX: a music library has no use for a number that long.
                match ma
                    .len()
                    .cmp(&mb.len())
                    .then_with(|| ma.cmp(mb))
                    // Equal magnitude ("7" vs "07"): fall back to the zero
                    // count so the comparison is still a total order, fewer
                    // zeros sorting first.
                    .then_with(|| (na.len() - ma.len()).cmp(&(nb.len() - mb.len())))
                {
                    Ordering::Equal => continue,
                    other => other,
                }
            }
            (Some(ca), Some(cb)) if ca == cb => {
                ai.next();
                bi.next();
                continue;
            }
            (Some(ca), Some(cb)) => ca.cmp(&cb),
        };
    }
}

fn take_digits(iter: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut out = String::new();
    while let Some(&c) = iter.peek() {
        if c.is_ascii_digit() {
            out.push(c);
            iter.next();
        } else {
            break;
        }
    }
    out
}

/// Compare two names (album/track titles, genre names): fold, then natural.
pub fn compare_folded(a: &str, b: &str) -> Ordering {
    natural_cmp(&fold(a), &fold(b))
}

/// Compare two artist names: drop a leading article, then [`compare_folded`].
/// Safe to call on an already sort-adjusted string (Jellyfin's own `SortName`,
/// or a value already run through this function) — stripping an article that
/// isn't there is a no-op.
pub fn compare_artist_names(a: &str, b: &str) -> Ordering {
    compare_folded(strip_leading_article(a), strip_leading_article(b))
}

/// Register the `BRAGI_FOLD` and `BRAGI_ARTIST` collations on a connection.
/// Must run on every connection that opens the database (collations are a
/// per-connection registration, not schema) — called from both `Database::open`
/// and `Database::open_in_memory`.
pub fn register_collations(conn: &Connection) -> rusqlite::Result<()> {
    conn.create_collation("BRAGI_FOLD", compare_folded)?;
    conn.create_collation("BRAGI_ARTIST", compare_artist_names)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_case() {
        assert_eq!(compare_folded("Abba", "abba"), Ordering::Equal);
    }

    #[test]
    fn folds_accents() {
        assert_eq!(compare_folded("Beyonce", "Beyoncé"), Ordering::Equal);
        assert_eq!(compare_folded("Sigur Ros", "Sigur Rós"), Ordering::Equal);
    }

    #[test]
    fn sorts_numbers_naturally() {
        assert_eq!(compare_folded("Track 2", "Track 10"), Ordering::Less);
        assert_eq!(compare_folded("Track 10", "Track 2"), Ordering::Greater);
        // Lexicographic would put "10" before "2".
        let mut v = vec!["Track 10", "Track 2", "Track 1"];
        v.sort_by(|a, b| compare_folded(a, b));
        assert_eq!(v, vec!["Track 1", "Track 2", "Track 10"]);
    }

    #[test]
    fn compares_digit_runs_by_magnitude_ignoring_leading_zeros() {
        // A raw-length compare puts "Track 07" (2 chars) after "Track 8" (1
        // char) even though 7 < 8 — the bug this pins.
        assert_eq!(compare_folded("Track 07", "Track 8"), Ordering::Less);
        let mut v = vec!["Track 8", "Track 07"];
        v.sort_by(|a, b| compare_folded(a, b));
        assert_eq!(v, vec!["Track 07", "Track 8"]);
    }

    #[test]
    fn breaks_an_equal_magnitude_tie_by_zero_count() {
        assert_eq!(compare_folded("Track 7", "Track 07"), Ordering::Less);
        assert_eq!(compare_folded("Track 07", "Track 007"), Ordering::Less);
        assert_eq!(compare_folded("Track 07", "Track 07"), Ordering::Equal);
    }

    #[test]
    fn strips_leading_article_for_artists() {
        assert_eq!(strip_leading_article("The Beatles"), "Beatles");
        assert_eq!(
            strip_leading_article("A Tribe Called Quest"),
            "Tribe Called Quest"
        );
        assert_eq!(strip_leading_article("An Cafe"), "Cafe");
        assert_eq!(strip_leading_article("Air"), "Air");
        assert_eq!(strip_leading_article("A"), "A");
    }

    #[test]
    fn artist_compare_ignores_article() {
        assert_eq!(
            compare_artist_names("The Beatles", "Beatles"),
            Ordering::Equal
        );
        let mut v = vec!["The Black Keys", "Air", "A Tribe Called Quest"];
        v.sort_by(|a, b| compare_artist_names(a, b));
        assert_eq!(v, vec!["Air", "The Black Keys", "A Tribe Called Quest"]);
    }

    #[test]
    fn plain_name_compare_keeps_article() {
        // compare_folded (albums/tracks/genres) never strips an article.
        assert_ne!(compare_folded("The Wall", "Wall"), Ordering::Equal);
    }

    #[test]
    fn artist_compare_is_idempotent_on_a_jellyfin_sort_name() {
        // A SortName Jellyfin already computed ("Beatles, The") must not be
        // mangled further — no leading article means the strip is a no-op.
        assert_eq!(strip_leading_article("Beatles, The"), "Beatles, The");
    }

    /// Pins this module's comparators against the same fixture
    /// `ordering.test.ts` sorts with `compareFolded`/`compareArtistNames`, so
    /// the SQLite collation and the TypeScript comparator can no longer drift
    /// silently — the settings-defaults.json fixture's pattern.
    #[test]
    fn matches_the_shared_frontend_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/ordering-names.json")).unwrap();

        let titles: Vec<String> = serde_json::from_value(fixture["titles"].clone()).unwrap();
        let titles_sorted: Vec<String> =
            serde_json::from_value(fixture["titlesSorted"].clone()).unwrap();
        let mut sorted_titles = titles;
        sorted_titles.sort_by(|a, b| compare_folded(a, b));
        assert_eq!(
            sorted_titles, titles_sorted,
            "compare_folded no longer matches src-tauri/fixtures/ordering-names.json"
        );

        let artists: Vec<String> = serde_json::from_value(fixture["artists"].clone()).unwrap();
        let artists_sorted: Vec<String> =
            serde_json::from_value(fixture["artistsSorted"].clone()).unwrap();
        let mut sorted_artists = artists;
        sorted_artists.sort_by(|a, b| compare_artist_names(a, b));
        assert_eq!(
            sorted_artists, artists_sorted,
            "compare_artist_names no longer matches src-tauri/fixtures/ordering-names.json"
        );
    }
}
