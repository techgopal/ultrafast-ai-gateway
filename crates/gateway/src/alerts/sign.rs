//! The signature of a webhook: `x-uf-signature: t=<unix seconds>,v1=<hex>`
//! where the hex is HMAC-SHA256 over `"<t>.<raw body>"` with the channel's
//! secret.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::secrets::fill_random;

/// The header all deliveries carry.
pub const SIGNATURE_HEADER: &str = "x-uf-signature";
const SECRET_PREFIX: &str = "whsec_";

/// A new channel secret: `whsec_` and 32 random bytes as hex.
pub fn new_secret() -> String {
    let mut bytes = [0u8; 32];
    fill_random(&mut bytes);
    format!("{SECRET_PREFIX}{}", hex::encode(bytes))
}

/// The hex HMAC-SHA256 of `"<t>.<body>"`.
pub fn signature(secret: &str, t: i64, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts a key of any length");
    mac.update(t.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

/// The value of [`SIGNATURE_HEADER`].
pub fn header_value(secret: &str, t: i64, body: &[u8]) -> String {
    format!("t={t},v1={}", signature(secret, t, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_answer() {
        // Computed with Python's hmac/hashlib, independently of this code.
        let sig = signature("whsec_test-secret", 1_700_000_000, br#"{"a":1}"#);
        assert_eq!(
            sig,
            "f7c40776aa58d1eee88648e741f2488ecc8e2cc810cd3801942f335e82a1da57"
        );
        assert_eq!(
            header_value("whsec_test-secret", 1_700_000_000, br#"{"a":1}"#),
            format!("t=1700000000,v1={sig}")
        );
    }

    #[test]
    fn the_signature_covers_the_time_the_body_and_the_secret() {
        let base = signature("s", 1, b"x");
        assert_ne!(base, signature("s", 2, b"x"));
        assert_ne!(base, signature("s", 1, b"y"));
        assert_ne!(base, signature("t", 1, b"x"));
    }

    #[test]
    fn secrets_are_whsec_and_64_hex_and_differ() {
        let a = new_secret();
        let b = new_secret();
        assert_ne!(a, b);
        let hex = a.strip_prefix("whsec_").expect("prefix");
        assert_eq!(hex.len(), 64);
        assert!(hex.bytes().all(|c| c.is_ascii_hexdigit()));
    }
}
