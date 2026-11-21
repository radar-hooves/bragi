//! OpenSubsonic authentication.
//!
//! Every Subsonic request carries auth in the query string:
//! `u=<user>&t=md5(password+salt)&s=<salt>&v=1.16.1&c=bragi&f=json`.
//! A fresh random salt is generated per request (token auth), so the password
//! itself never travels on the wire. md5 is mandated by the Subsonic protocol.

/// The Subsonic API protocol version Bragi advertises.
pub const API_VERSION: &str = "1.16.1";
/// The client name reported to the server (the `c` parameter).
pub const CLIENT_NAME: &str = "bragi";

/// Compute the Subsonic auth token `t = md5(password + salt)` as lowercase hex.
pub fn auth_token(password: &str, salt: &str) -> String {
    let mut input = String::with_capacity(password.len() + salt.len());
    input.push_str(password);
    input.push_str(salt);
    format!("{:x}", md5::compute(input.as_bytes()))
}

/// Generate a random hex salt for a single request.
///
/// The Subsonic spec requires a per-request salt (≥6 chars); we use 16 hex
/// characters (64 bits) drawn from the process RNG.
pub fn random_salt() -> String {
    let mut s = String::with_capacity(16);
    for _ in 0..16 {
        // fastrand is already a dependency; hex digit 0-15.
        s.push(char::from_digit(fastrand::u32(0..16), 16).expect("0..16 is a valid hex digit"));
    }
    s
}

/// Append the common auth + client query params to a Subsonic request URL.
///
/// Used by both the API client and the streaming proxy so every upstream call
/// is signed identically.
pub fn append_auth(url: &mut url::Url, username: &str, password: &str) {
    let salt = random_salt();
    let token = auth_token(password, &salt);
    url.query_pairs_mut()
        .append_pair("u", username)
        .append_pair("t", &token)
        .append_pair("s", &salt)
        .append_pair("v", API_VERSION)
        .append_pair("c", CLIENT_NAME)
        .append_pair("f", "json");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_matches_canonical_vector() {
        // Canonical example from the Subsonic API docs: password "sesame",
        // salt "c19b2d" -> token 26719a1196d2a940705a59634eb18eab.
        assert_eq!(
            auth_token("sesame", "c19b2d"),
            "26719a1196d2a940705a59634eb18eab"
        );
    }

    #[test]
    fn salt_is_hex_and_long_enough() {
        let s = random_salt();
        assert!(s.len() >= 6, "salt must be at least 6 chars");
        assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn append_auth_sets_all_params_without_leaking_password() {
        let mut url = url::Url::parse("https://demo.navidrome.org/rest/ping.view").unwrap();
        append_auth(&mut url, "demo", "demo");
        let q = url.query().unwrap();
        for key in ["u=demo", "t=", "s=", "v=1.16.1", "c=bragi", "f=json"] {
            assert!(q.contains(key), "missing {key} in {q}");
        }
        // The raw password must never appear as a query value.
        assert!(!q.contains("p=demo"));
        assert!(!q.contains("password"));
    }
}
