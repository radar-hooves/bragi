//! Common database utilities.
//!
//! Shared functions for database operations to avoid code duplication.

use std::time::{SystemTime, UNIX_EPOCH};

/// Get the current Unix timestamp in seconds.
///
/// Returns 0 if the system time cannot be determined (should never happen).
pub fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// FTS5 reserved keywords that must be filtered from queries.
const FTS_RESERVED_KEYWORDS: &[&str] = &["AND", "OR", "NOT", "NEAR"];

/// Escape special characters in FTS query for safe FTS5 matching.
///
/// This function:
/// 1. Filters out non-alphanumeric characters (except whitespace)
/// 2. Filters out FTS5 reserved keywords (AND, OR, NOT, NEAR)
/// 3. Wraps each term in double quotes for safe exact matching
///
/// # Security
///
/// This prevents SQL injection via FTS5 query syntax by ensuring
/// all search terms are properly quoted and special operators are removed.
pub fn escape_fts_query(query: &str) -> String {
    // First, filter to only alphanumeric and whitespace
    let filtered: String = query
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();

    // Split into words, filter reserved keywords, and wrap in quotes
    filtered
        .split_whitespace()
        .filter(|word| !FTS_RESERVED_KEYWORDS.contains(&word.to_uppercase().as_str()))
        .filter(|word| !word.is_empty())
        .map(|word| format!("\"{}\"", word))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_timestamp() {
        let ts = current_timestamp();
        // Should be a reasonable Unix timestamp (after 2020)
        assert!(ts > 1577836800); // 2020-01-01
    }

    #[test]
    fn test_escape_fts_query_basic() {
        assert_eq!(escape_fts_query("hello world"), "\"hello\" \"world\"");
    }

    #[test]
    fn test_escape_fts_query_special_chars() {
        // Special characters should be filtered out
        assert_eq!(escape_fts_query("test*"), "\"test\"");
        assert_eq!(escape_fts_query("hello-world"), "\"helloworld\"");
        assert_eq!(escape_fts_query("test:query"), "\"testquery\"");
    }

    #[test]
    fn test_escape_fts_query_reserved_keywords() {
        // FTS reserved keywords should be filtered
        assert_eq!(escape_fts_query("rock AND roll"), "\"rock\" \"roll\"");
        assert_eq!(escape_fts_query("NOT metal"), "\"metal\"");
        assert_eq!(escape_fts_query("jazz OR blues"), "\"jazz\" \"blues\"");
        assert_eq!(escape_fts_query("word NEAR phrase"), "\"word\" \"phrase\"");
    }

    #[test]
    fn test_escape_fts_query_case_insensitive_keywords() {
        // Keywords should be filtered case-insensitively
        assert_eq!(escape_fts_query("and or not near"), "");
        assert_eq!(escape_fts_query("And Or Not Near"), "");
    }

    #[test]
    fn test_escape_fts_query_empty() {
        assert_eq!(escape_fts_query(""), "");
        assert_eq!(escape_fts_query("   "), "");
    }

    #[test]
    fn test_escape_fts_query_single_word() {
        assert_eq!(escape_fts_query("test"), "\"test\"");
    }

    #[test]
    fn test_escape_fts_query_injection_attempt() {
        // Attempt to inject FTS operators
        assert_eq!(escape_fts_query("test OR 1=1"), "\"test\" \"11\"");
        assert_eq!(escape_fts_query("test\" OR \"hack"), "\"test\" \"hack\"");
    }
}
