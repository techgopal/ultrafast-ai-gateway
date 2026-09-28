//! Password policy and Argon2id hashing.

use std::sync::{LazyLock, OnceLock};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::rngs::OsRng;

pub const MIN_PASSWORD_LEN: usize = 12;
pub const MAX_PASSWORD_LEN: usize = 256;

/// Argon2 memory cost in KiB.
const MEMORY_KIB: u32 = 19456;
/// Argon2 iteration count.
const ITERATIONS: u32 = 2;
/// Argon2 degree of parallelism.
const PARALLELISM: u32 = 1;

/// A hash of a fixed password, computed once, for `verify_dummy`.
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| {
    hash_password("dummy-password-for-timing").expect("hashing the dummy password")
});

/// Computes the dummy hash now instead of during the first sign-in for an
/// unknown account. Call it at startup: a hashing failure is returned here
/// rather than raised inside a request.
pub fn warm_up() -> anyhow::Result<()> {
    static DONE: OnceLock<()> = OnceLock::new();
    if DONE.get().is_some() {
        return Ok(());
    }
    // Hashing fails here, as an error, before the dummy hash is forced.
    hash_password("warm-up-probe")?;
    LazyLock::force(&DUMMY_HASH);
    let _ = DONE.set(());
    Ok(())
}

fn argon2() -> anyhow::Result<Argon2<'static>> {
    let params = Params::new(MEMORY_KIB, ITERATIONS, PARALLELISM, None)
        .map_err(|e| anyhow::anyhow!("invalid argon2 parameters: {e}"))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// Checks the length of a password, counted in characters.
pub fn check_password_policy(password: &str) -> Result<(), &'static str> {
    let len = password.chars().count();
    if len < MIN_PASSWORD_LEN {
        return Err("password must be at least 12 characters");
    }
    if len > MAX_PASSWORD_LEN {
        return Err("password must be at most 256 characters");
    }
    Ok(())
}

/// Hashes a password with Argon2id and a fresh salt, returning the PHC
/// string. Does not check the password policy.
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = argon2()?
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("password hashing failed: {e}"))?;
    Ok(hash.to_string())
}

/// Whether `password` matches the PHC string `phc`. A malformed hash is a
/// mismatch.
pub fn verify_password(password: &str, phc: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(phc) else {
        return false;
    };
    // The parameters used are the ones recorded in the hash.
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Spends the same effort as a real verification. Used when the account
/// does not exist, so timing does not reveal that.
pub fn verify_dummy(password: &str) {
    let _ = verify_password(password, &DUMMY_HASH);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_policy() {
        assert_eq!(
            check_password_policy(&"a".repeat(11)),
            Err("password must be at least 12 characters")
        );
        assert_eq!(check_password_policy(&"a".repeat(12)), Ok(()));
        assert_eq!(check_password_policy(&"a".repeat(256)), Ok(()));
        assert_eq!(
            check_password_policy(&"a".repeat(257)),
            Err("password must be at most 256 characters")
        );
        // 12 characters, 24 bytes.
        assert_eq!(check_password_policy(&"é".repeat(12)), Ok(()));
        // 11 characters, 22 bytes: long enough in bytes, too short in characters.
        assert_eq!(
            check_password_policy(&"é".repeat(11)),
            Err("password must be at least 12 characters")
        );
        // 256 characters, 512 bytes.
        assert_eq!(check_password_policy(&"é".repeat(256)), Ok(()));
    }

    #[test]
    fn hash_and_verify() {
        let first = hash_password("correct horse battery").unwrap();
        let second = hash_password("correct horse battery").unwrap();
        assert!(first.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
        assert_ne!(first, second);
        assert!(verify_password("correct horse battery", &first));
        assert!(!verify_password("wrong horse battery", &first));
    }

    #[test]
    fn verify_handles_bad_hashes() {
        assert!(!verify_password("x", ""));
        assert!(!verify_password("x", "not-a-hash"));
        assert!(!verify_password("x", "$argon2id$broken"));
    }

    #[test]
    fn dummy_verification_runs() {
        verify_dummy("anything");
    }

    #[test]
    fn warm_up_can_be_repeated() {
        warm_up().unwrap();
        warm_up().unwrap();
        verify_dummy("anything");
    }

    #[test]
    fn dummy_hash_is_a_real_argon2id_hash() {
        assert!(DUMMY_HASH.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
        assert!(verify_password("dummy-password-for-timing", &DUMMY_HASH));
    }
}
