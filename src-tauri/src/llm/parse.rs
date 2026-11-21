//! Defensive JSON extraction from LLM responses.
//!
//! Small local models (and Claude-backed proxies that ignore `response_format`)
//! wrap JSON in prose or ```json fences. This pulls the first well-formed JSON
//! value out of such a response — an object OR a bare array (a track-index list).

use crate::error::AppError;
use serde_json::Value;

/// Extract a JSON value from a possibly-decorated LLM response.
///
/// Order: strip code fences → try a direct parse → depth-matched substring from
/// the first `{`/`[` to its matching close. Tolerates trailing prose.
///
pub fn extract_json(raw: &str) -> Result<Value, AppError> {
    let stripped = strip_code_fences(raw.trim());

    // Fast path: the whole thing is valid JSON.
    if let Ok(value) = serde_json::from_str::<Value>(stripped) {
        return Ok(value);
    }

    // Depth-walk from the first opening delimiter to its match, so trailing
    // prose after the value does not break the parse.
    if let Some(candidate) = balanced_slice(stripped) {
        if let Ok(value) = serde_json::from_str::<Value>(candidate) {
            return Ok(value);
        }
    }

    let preview: String = raw.trim().chars().take(200).collect();
    Err(AppError::InvalidOperation(format!(
        "LLM returned no parseable JSON: {preview}"
    )))
}

/// Remove a single wrapping ```lang ... ``` fence if present.
fn strip_code_fences(s: &str) -> &str {
    let s = s.trim();
    if !s.starts_with("```") {
        return s;
    }
    // Drop the opening fence line (``` or ```json) …
    let after_open = match s.find('\n') {
        Some(nl) => &s[nl + 1..],
        None => return s,
    };
    // … and the closing fence.
    match after_open.rfind("```") {
        Some(idx) => after_open[..idx].trim(),
        None => after_open.trim(),
    }
}

/// Return the substring from the first `{` or `[` to its matching close brace,
/// honouring nesting and skipping delimiters inside strings.
fn balanced_slice(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let (open, close) = {
        let brace = s.find('{');
        let bracket = s.find('[');
        match (brace, bracket) {
            (Some(b), Some(k)) if b < k => (b, b'}'),
            (Some(_), Some(k)) => (k, b']'),
            (Some(b), None) => (b, b'}'),
            (None, Some(k)) => (k, b']'),
            (None, None) => return None,
        }
    };
    let open_byte = bytes[open];
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut i = open;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_string = false;
            }
        } else {
            match c {
                b'"' => in_string = true,
                b if b == open_byte => depth += 1,
                b if b == close => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&s[open..=i]);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_object() {
        let v = extract_json(r#"{"name":"Chill","indices":[1,2,3]}"#).unwrap();
        assert_eq!(v["name"], "Chill");
        assert_eq!(v["indices"][0], 1);
    }

    #[test]
    fn strips_json_code_fence() {
        let raw = "```json\n{\"name\":\"Focus\",\"indices\":[4,5]}\n```";
        let v = extract_json(raw).unwrap();
        assert_eq!(v["name"], "Focus");
    }

    #[test]
    fn extracts_object_from_surrounding_prose() {
        let raw = "Sure! Here is your playlist:\n{\"indices\":[7,1,9]}\nHope you enjoy it.";
        let v = extract_json(raw).unwrap();
        assert_eq!(v["indices"][2], 9);
    }

    #[test]
    fn tolerates_bare_index_array() {
        let v = extract_json("[3, 7, 1, 42]").unwrap();
        assert_eq!(v.as_array().unwrap().len(), 4);
        assert_eq!(v[3], 42);
    }

    #[test]
    fn ignores_braces_inside_strings() {
        let raw = r#"{"title":"a } b { c","indices":[0]}"#;
        let v = extract_json(raw).unwrap();
        assert_eq!(v["title"], "a } b { c");
    }

    #[test]
    fn errors_on_unparseable() {
        assert!(extract_json("I cannot help with that.").is_err());
    }
}
