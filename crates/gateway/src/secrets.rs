//! Virtual key generation and hashing, and encryption of provider credentials.

use std::fmt;

use anyhow::{anyhow, bail, Result};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::rngs::SysRng;
use rand::TryRng;
use sha2::{Digest, Sha256};

pub const KEY_PREFIX: &str = "uf-sk-";
pub const TOKEN_PREFIX: &str = "uf-at-";
pub const INVITE_PREFIX: &str = "uf-inv-";
const NONCE_LEN: usize = 12;

pub struct NewKey {
    /// Shown to the user once. Never stored.
    pub full: String,
    pub hash: String,
    /// Safe to store and show, for example `uf-sk-…7d2f`.
    pub display: String,
}

/// Redacts the full key, so logging a `NewKey` cannot leak it.
impl fmt::Debug for NewKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NewKey")
            .field("full", &"<redacted>")
            .field("hash", &self.hash)
            .field("display", &self.display)
            .finish()
    }
}

/// Fills `buf` from the operating system's random number generator.
///
/// # Panics
/// If the operating system cannot supply random bytes. No secret is made
/// from anything else.
pub fn fill_random(buf: &mut [u8]) {
    SysRng
        .try_fill_bytes(buf)
        .expect("the operating system supplies random bytes");
}

/// The letters of a setup code: Crockford's base 32, which has no I, L, O
/// or U, so a code read from a log is not mistyped.
const SETUP_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A one-time code for creating the first admin, as `XXXX-XXXX-XXXX`: 60
/// random bits.
pub fn generate_setup_code() -> String {
    let mut bytes = [0u8; 12];
    fill_random(&mut bytes);
    let letters: Vec<char> = bytes
        .iter()
        // 256 is a multiple of 32: every letter is as likely.
        .map(|b| char::from(SETUP_ALPHABET[usize::from(b % 32)]))
        .collect();
    letters
        .chunks(4)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

/// Whether `given` is the setup code `expected`, read as people type it:
/// in any case, with or without dashes and spaces, an O for a zero and an I
/// or L for a one. Compared by hash, so the time taken says nothing of how
/// much of it was right.
pub fn setup_code_matches(expected: &str, given: &str) -> bool {
    let normal = |text: &str| -> String {
        text.chars()
            .filter(|c| *c != '-' && !c.is_whitespace())
            .map(|c| match c.to_ascii_uppercase() {
                'O' => '0',
                'I' | 'L' => '1',
                other => other,
            })
            .collect()
    };
    let given = normal(given);
    !given.is_empty() && Sha256::digest(normal(expected)) == Sha256::digest(given)
}

pub fn generate_key() -> NewKey {
    generate_secret(KEY_PREFIX)
}

/// A secret with the given prefix and 32 random bytes as hex.
pub fn generate_secret(prefix: &str) -> NewKey {
    let mut bytes = [0u8; 32];
    fill_random(&mut bytes);
    let full = format!("{prefix}{}", hex::encode(bytes));
    let display = format!("{prefix}\u{2026}{}", &full[full.len() - 4..]);
    NewKey {
        hash: hash_key(&full),
        full,
        display,
    }
}

pub fn hash_key(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

/// Whether two secrets are equal. It compares their SHA-256 digests, so how
/// long the comparison takes says nothing about the secrets themselves.
pub fn secrets_equal(a: &str, b: &str) -> bool {
    hash_key(a) == hash_key(b)
}

#[derive(Clone)]
pub struct Cipher(ChaCha20Poly1305);

/// Never prints key material.
impl fmt::Debug for Cipher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Cipher(<redacted>)")
    }
}

impl Cipher {
    pub fn generate_master_hex() -> String {
        let mut bytes = [0u8; 32];
        fill_random(&mut bytes);
        hex::encode(bytes)
    }

    pub fn from_hex(master: &str) -> Result<Self> {
        let bytes = hex::decode(master.trim()).map_err(|_| anyhow!("master key is not hex"))?;
        if bytes.len() != 32 {
            bail!("master key must be 32 bytes (64 hex characters)");
        }
        if bytes.iter().all(|b| *b == 0) {
            bail!("master key must not be all zeros");
        }
        let key = Key::try_from(bytes.as_slice()).expect("the length was checked to be 32 bytes");
        Ok(Self(ChaCha20Poly1305::new(&key)))
    }

    /// Output is the 12-byte nonce followed by the ciphertext.
    pub fn encrypt(&self, plain: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; NONCE_LEN];
        fill_random(&mut nonce);
        let ct = self
            .0
            .encrypt(&Nonce::from(nonce), plain)
            .expect("encryption with a valid key and nonce cannot fail");
        let mut out = nonce.to_vec();
        out.extend(ct);
        out
    }

    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() <= NONCE_LEN {
            bail!("encrypted value is too short");
        }
        let (nonce, ct) = data.split_at(NONCE_LEN);
        let nonce = Nonce::try_from(nonce).expect("the slice is 12 bytes long");
        self.0
            .decrypt(&nonce, ct)
            .map_err(|_| anyhow!("could not decrypt: wrong master key or corrupted value"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_are_unique_prefixed_and_hash_consistently() {
        let a = generate_key();
        let b = generate_key();
        assert_ne!(a.full, b.full);
        assert!(a.full.starts_with(KEY_PREFIX));
        assert_eq!(a.full.len(), KEY_PREFIX.len() + 64);
        assert_eq!(a.hash, hash_key(&a.full));
        assert_eq!(a.hash.len(), 64);
        assert!(a.display.ends_with(&a.full[a.full.len() - 4..]));
        assert!(!a
            .display
            .contains(&a.full[KEY_PREFIX.len()..a.full.len() - 4]));
    }

    #[test]
    fn generate_secret_uses_prefix() {
        for prefix in [TOKEN_PREFIX, INVITE_PREFIX, KEY_PREFIX] {
            let a = generate_secret(prefix);
            let b = generate_secret(prefix);
            assert_ne!(a.full, b.full);
            assert!(a.full.starts_with(prefix));
            assert_eq!(a.full.len(), prefix.len() + 64);
            assert_eq!(a.hash, hash_key(&a.full));
            assert!(a.display.starts_with(prefix));
            assert!(a.display.ends_with(&a.full[a.full.len() - 4..]));
            assert!(!a.display.contains(&a.full[prefix.len()..a.full.len() - 4]));
            let shown = format!("{a:?}");
            assert!(!shown.contains(&a.full));
            assert!(!shown.contains(&a.full[prefix.len()..]));
        }
        assert_eq!(TOKEN_PREFIX, "uf-at-");
        assert_eq!(INVITE_PREFIX, "uf-inv-");
        assert_eq!(generate_secret(TOKEN_PREFIX).full.len(), 6 + 64);
    }

    #[test]
    fn secrets_are_compared_by_value() {
        let a = generate_key().full;
        assert!(secrets_equal(&a, &a.clone()));
        assert!(secrets_equal("", ""));
        assert!(!secrets_equal(&a, &generate_key().full));
        assert!(!secrets_equal(&a, &a[..a.len() - 1]));
        assert!(!secrets_equal(&a, &a.to_uppercase()));
        assert!(!secrets_equal(&a, ""));
        assert!(!secrets_equal("", &a));
    }

    #[test]
    fn cipher_round_trips_and_uses_fresh_nonces() {
        let c = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
        let one = c.encrypt(b"sk-provider");
        let two = c.encrypt(b"sk-provider");
        assert_ne!(one, two);
        assert_eq!(c.decrypt(&one).unwrap(), b"sk-provider");
    }

    #[test]
    fn cipher_rejects_tampering_wrong_key_and_bad_input() {
        let c = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
        let mut data = c.encrypt(b"secret");
        let last = data.len() - 1;
        data[last] ^= 1;
        assert!(c.decrypt(&data).is_err());
        let other = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
        assert!(other.decrypt(&c.encrypt(b"secret")).is_err());
        assert!(c.decrypt(b"short").is_err());
        assert!(Cipher::from_hex("abcd").is_err());
        assert!(Cipher::from_hex("not hex").is_err());
    }

    #[test]
    fn cipher_rejects_an_all_zero_master_key() {
        let err = Cipher::from_hex(&"00".repeat(32)).expect_err("must be rejected");
        assert!(err.to_string().contains("zero"));
    }

    #[test]
    fn cipher_errors_do_not_echo_the_master_key() {
        let bad = "zz".repeat(32);
        let err = Cipher::from_hex(&bad).expect_err("must be rejected");
        assert!(!format!("{err:?}").contains(&bad));
    }

    #[test]
    fn debug_output_does_not_expose_secrets() {
        let master = Cipher::generate_master_hex();
        let c = Cipher::from_hex(&master).unwrap();
        let shown = format!("{c:?}");
        assert!(!shown.contains(&master));
        assert_eq!(shown, "Cipher(<redacted>)");

        let k = generate_key();
        let shown = format!("{k:?}");
        assert!(!shown.contains(&k.full));
        assert!(!shown.contains(&k.full[KEY_PREFIX.len()..]));
        assert!(shown.contains(&k.display));
    }

    // Known answers. The values below were produced by sha2 0.10 and
    // chacha20poly1305 0.10. Stored hashes and credentials depend on them:
    // never change the expected values.

    #[test]
    fn hash_key_matches_stored_hashes() {
        assert_eq!(
            hash_key("uf-sk-00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"),
            "44d70721051f4488e32fcf97e42380e3b2371f022c9984e45fca8530d41f16b5"
        );
        assert_eq!(
            hash_key(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hash_key("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn cipher_decrypts_a_stored_credential() {
        let c =
            Cipher::from_hex("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f")
                .unwrap();
        let stored = hex::decode(
            "ab9df8363ccae36d1c5561c3498fad8249f6c496f7333e0a19e74e8f4e245ac6\
             58ece260e7e13e1964a7a124881fb5c4b4c7aa776f7063573b41",
        )
        .unwrap();
        // 12-byte nonce, the ciphertext, a 16-byte tag.
        assert_eq!(stored.len(), 12 + 30 + 16);
        assert_eq!(
            c.decrypt(&stored).unwrap(),
            b"sk-fixture-provider-credential"
        );
    }

    #[test]
    fn cipher_output_is_nonce_then_ciphertext_and_tag() {
        let c =
            Cipher::from_hex("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f")
                .unwrap();
        let out = c.encrypt(b"sk-fixture-provider-credential");
        assert_eq!(out.len(), 12 + 30 + 16);
        assert_eq!(c.decrypt(&out).unwrap(), b"sk-fixture-provider-credential");
    }
}

#[cfg(test)]
mod setup_code_tests {
    use super::*;

    #[test]
    fn a_setup_code_is_three_groups_of_four_unambiguous_letters() {
        for _ in 0..200 {
            let code = generate_setup_code();
            assert_eq!(code.len(), 14, "{code}");
            let groups: Vec<&str> = code.split('-').collect();
            assert_eq!(groups.len(), 3, "{code}");
            for group in groups {
                assert_eq!(group.len(), 4);
                assert!(group.bytes().all(|b| SETUP_ALPHABET.contains(&b)), "{code}");
            }
        }
        assert_ne!(generate_setup_code(), generate_setup_code());
    }

    #[test]
    fn a_setup_code_is_read_as_people_type_it() {
        let code = "A0B1-CDEF-9XYZ";
        for given in [
            code,
            "a0b1-cdef-9xyz",
            "A0B1CDEF9XYZ",
            " a0b1 cdef 9xyz ",
            "AOBI-CDEF-9XYZ",
            "AOBL-CDEF-9XYZ",
        ] {
            assert!(setup_code_matches(code, given), "{given}");
        }
        for given in [
            "",
            "-",
            "A0B1-CDEF-9XY",
            "A0B1-CDEF-9XYZZ",
            "B0B1-CDEF-9XYZ",
        ] {
            assert!(!setup_code_matches(code, given), "{given}");
        }
    }
}
