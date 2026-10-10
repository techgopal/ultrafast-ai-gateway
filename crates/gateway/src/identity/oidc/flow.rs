//! The state of one sign-in attempt, kept in a cookie on the browser.

use std::fmt;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::identity::external::ExternalError;
use crate::secrets::{fill_random, secrets_equal, Cipher};

/// How long after `issued_at` a callback is still accepted.
pub const FLOW_MAX_AGE_SECS: i64 = 600;
/// How far `issued_at` may lie ahead of this machine's clock.
const CLOCK_SKEW_SECS: i64 = 60;
/// Starts every cookie plaintext. The master key also protects stored
/// secrets; the label keeps a ciphertext made for another purpose from ever
/// being read as a flow.
const LABEL: &[u8] = b"uf-oidc-flow-v1\n";
/// A cookie longer than this is refused unread (a cookie is at most 4 KiB).
const MAX_COOKIE_BYTES: usize = 4096;

#[derive(Clone, Serialize, Deserialize)]
pub struct FlowState {
    pub state: String,
    pub nonce: String,
    pub verifier: String,
    pub return_to: String,
    pub issued_at: i64,
}

/// Never prints the nonce or the verifier.
impl fmt::Debug for FlowState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FlowState(<redacted>)")
    }
}

fn random_token() -> String {
    let mut bytes = [0u8; 32];
    fill_random(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The S256 code challenge of a PKCE verifier (RFC 7636, section 4.2).
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

impl FlowState {
    /// A new attempt with 256 random bits each for state, nonce and verifier
    /// (a 43 character verifier, the shortest RFC 7636 allows).
    pub fn new(return_to: &str, now: i64) -> Self {
        Self {
            state: random_token(),
            nonce: random_token(),
            verifier: random_token(),
            return_to: return_to.to_string(),
            issued_at: now,
        }
    }

    /// The cookie value: the JSON, encrypted, as base64url.
    pub fn seal(&self, cipher: &Cipher) -> String {
        let mut plain = LABEL.to_vec();
        serde_json::to_writer(&mut plain, self).expect("a flow state serializes");
        URL_SAFE_NO_PAD.encode(cipher.encrypt(&plain))
    }

    /// Reads a cookie value. Anything that is not exactly what `seal` made
    /// (missing, altered, made under another key) is `BadState`; one that is
    /// older than ten minutes, counted from `issued_at`, is `Expired`.
    pub fn open(cipher: &Cipher, cookie: &str, now: i64) -> Result<Self, ExternalError> {
        let flow = Self::read(cipher, cookie)?;
        if flow.issued_at > now + CLOCK_SKEW_SECS {
            return Err(ExternalError::BadState);
        }
        if now - flow.issued_at > FLOW_MAX_AGE_SECS {
            return Err(ExternalError::Expired);
        }
        Ok(flow)
    }

    /// The page an attempt was for, whatever its age: the cookie is
    /// encrypted and authenticated, so only this gateway can have made it.
    pub fn return_to_of(cipher: &Cipher, cookie: &str) -> Option<String> {
        Self::read(cipher, cookie).ok().map(|flow| flow.return_to)
    }

    /// Decrypts and parses; the age is not judged.
    fn read(cipher: &Cipher, cookie: &str) -> Result<Self, ExternalError> {
        if cookie.is_empty() || cookie.len() > MAX_COOKIE_BYTES {
            return Err(ExternalError::BadState);
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(cookie)
            .map_err(|_| ExternalError::BadState)?;
        let plain = cipher
            .decrypt(&bytes)
            .map_err(|_| ExternalError::BadState)?;
        let json = plain.strip_prefix(LABEL).ok_or(ExternalError::BadState)?;
        serde_json::from_slice(json).map_err(|_| ExternalError::BadState)
    }

    /// Whether the `state` the browser brought back is this attempt's.
    pub fn state_matches(&self, returned: Option<&str>) -> bool {
        returned.is_some_and(|s| !s.is_empty() && secrets_equal(s, &self.state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher() -> Cipher {
        Cipher::from_hex(&"ab".repeat(32)).unwrap()
    }

    #[test]
    fn pkce_challenge_matches_rfc_7636_appendix_b() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn a_new_flow_has_long_distinct_random_values() {
        let a = FlowState::new("/keys", 1000);
        let b = FlowState::new("/keys", 1000);
        assert_ne!(a.state, b.state);
        assert_ne!(a.state, a.nonce);
        assert_ne!(a.nonce, a.verifier);
        assert_eq!(a.verifier.len(), 43);
        assert!(a.state.len() >= 22, "at least 128 bits");
    }

    #[test]
    fn the_cookie_round_trips() {
        let flow = FlowState::new("/keys?x=1", 5000);
        let cookie = flow.seal(&cipher());
        assert!(cookie
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
        let back = FlowState::open(&cipher(), &cookie, 5001).unwrap();
        assert_eq!(back.state, flow.state);
        assert_eq!(back.nonce, flow.nonce);
        assert_eq!(back.verifier, flow.verifier);
        assert_eq!(back.return_to, "/keys?x=1");
        assert_eq!(back.issued_at, 5000);
    }

    #[test]
    fn a_cookie_keeps_nothing_readable() {
        let flow = FlowState::new("/secret-looking-path", 5000);
        let cookie = flow.seal(&cipher());
        assert!(!cookie.contains(&flow.state));
        let raw = URL_SAFE_NO_PAD.decode(&cookie).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("secret-looking"));
    }

    #[test]
    fn a_tampered_or_foreign_cookie_is_bad_state() {
        let cookie = FlowState::new("/", 5000).seal(&cipher());
        // Flip one character in the middle.
        let mut chars: Vec<char> = cookie.chars().collect();
        let mid = chars.len() / 2;
        chars[mid] = if chars[mid] == 'A' { 'B' } else { 'A' };
        let tampered: String = chars.into_iter().collect();
        let other = Cipher::from_hex(&"cd".repeat(32)).unwrap();
        for bad in [
            tampered.as_str(),
            &cookie[..cookie.len() - 4],
            "",
            "not base64 !!",
            "AAAA",
            &"A".repeat(5000),
        ] {
            assert_eq!(
                FlowState::open(&cipher(), bad, 5001).unwrap_err(),
                ExternalError::BadState,
                "{bad}"
            );
        }
        assert_eq!(
            FlowState::open(&other, &cookie, 5001).unwrap_err(),
            ExternalError::BadState
        );
    }

    #[test]
    fn a_ciphertext_made_for_another_purpose_is_not_a_flow() {
        let flow = FlowState::new("/", 5000);
        let json = serde_json::to_vec(&flow).unwrap();
        let bare = URL_SAFE_NO_PAD.encode(cipher().encrypt(&json));
        assert_eq!(
            FlowState::open(&cipher(), &bare, 5001).unwrap_err(),
            ExternalError::BadState
        );
        // A stored secret (encrypted the same way) is not one either.
        let secret = URL_SAFE_NO_PAD.encode(cipher().encrypt(b"provider-api-key"));
        assert_eq!(
            FlowState::open(&cipher(), &secret, 5001).unwrap_err(),
            ExternalError::BadState
        );
    }

    #[test]
    fn the_cookie_expires_ten_minutes_after_issue() {
        let cookie = FlowState::new("/", 5000).seal(&cipher());
        assert!(FlowState::open(&cipher(), &cookie, 5000 + FLOW_MAX_AGE_SECS).is_ok());
        assert_eq!(
            FlowState::open(&cipher(), &cookie, 5000 + FLOW_MAX_AGE_SECS + 1).unwrap_err(),
            ExternalError::Expired
        );
    }

    #[test]
    fn a_cookie_from_the_future_is_bad_state() {
        let cookie = FlowState::new("/", 5000).seal(&cipher());
        assert_eq!(
            FlowState::open(&cipher(), &cookie, 5000 - CLOCK_SKEW_SECS - 1).unwrap_err(),
            ExternalError::BadState
        );
        assert!(FlowState::open(&cipher(), &cookie, 5000 - CLOCK_SKEW_SECS).is_ok());
    }

    #[test]
    fn the_returned_state_must_equal_the_cookies() {
        let flow = FlowState::new("/", 1);
        assert!(flow.state_matches(Some(&flow.state.clone())));
        assert!(!flow.state_matches(Some("other")));
        assert!(!flow.state_matches(Some("")));
        assert!(!flow.state_matches(None));
    }

    #[test]
    fn debug_hides_the_values() {
        let flow = FlowState::new("/", 1);
        assert!(!format!("{flow:?}").contains(&flow.nonce));
    }
}
