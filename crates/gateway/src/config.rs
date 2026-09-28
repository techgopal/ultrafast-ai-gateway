//! Data directory layout, the master key, and provider URL validation.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::secrets::Cipher;

const MASTER_KEY_FILE: &str = "master.key";

pub fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("gateway.db")
}

/// Returns the master key as hex. Uses `from_env` when given. Otherwise reads
/// `master.key` in the data directory, creating it on first use.
pub fn load_master_key(data_dir: &Path, from_env: Option<&str>) -> Result<String> {
    if let Some(v) = from_env {
        Cipher::from_hex(v).context("UF_MASTER_KEY is not valid")?;
        return Ok(v.trim().to_string());
    }
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("could not create {}", data_dir.display()))?;
    let path = data_dir.join(MASTER_KEY_FILE);
    if path.exists() {
        let v = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read {}", path.display()))?;
        Cipher::from_hex(&v)
            .with_context(|| format!("{} is not a valid master key", path.display()))?;
        return Ok(v.trim().to_string());
    }
    let v = Cipher::generate_master_hex();
    write_owner_only(&path, &v)?;
    Ok(v)
}

/// Checks a provider base URL. It must be `http://` or `https://` with a host,
/// and must not carry credentials, a query string or a fragment. Error
/// messages never echo the URL, since it may contain a secret.
pub fn validate_base_url(url: &str) -> Result<()> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"));
    let Some(rest) = rest else {
        bail!("base URL must start with http:// or https://");
    };
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        bail!("base URL must not contain whitespace");
    }
    if rest.contains('?') {
        bail!("base URL must not contain a query string");
    }
    if rest.contains('#') {
        bail!("base URL must not contain a fragment");
    }
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.contains('@') {
        bail!("base URL must not contain credentials; give the key with --api-key or UF_PROVIDER_API_KEY");
    }
    if authority.is_empty() || authority.starts_with(':') {
        bail!("base URL must include a host");
    }
    Ok(())
}

#[cfg(unix)]
fn write_owner_only(path: &Path, contents: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("could not create {}", path.display()))?;
    f.write_all(contents.as_bytes())?;
    Ok(())
}

#[cfg(not(unix))]
fn write_owner_only(path: &Path, contents: &str) -> Result<()> {
    std::fs::write(path, contents).with_context(|| format!("could not create {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::Cipher;

    #[test]
    fn env_value_wins_and_nothing_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let master = Cipher::generate_master_hex();
        assert_eq!(load_master_key(dir.path(), Some(&master)).unwrap(), master);
        assert!(!dir.path().join("master.key").exists());
    }

    #[test]
    fn invalid_env_value_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_master_key(dir.path(), Some("too-short")).is_err());
    }

    #[test]
    fn generates_once_then_reuses() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a/b");
        let first = load_master_key(&nested, None).unwrap();
        assert!(Cipher::from_hex(&first).is_ok());
        assert_eq!(load_master_key(&nested, None).unwrap(), first);
    }

    #[cfg(unix)]
    #[test]
    fn key_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        load_master_key(dir.path(), None).unwrap();
        let mode = std::fs::metadata(dir.path().join("master.key"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn corrupt_key_file_is_an_error_not_a_silent_regenerate() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("master.key"), "garbage").unwrap();
        assert!(load_master_key(dir.path(), None).is_err());
    }

    #[test]
    fn db_path_is_inside_the_data_directory() {
        assert_eq!(
            db_path(Path::new("/x")),
            PathBuf::from("/x").join("gateway.db")
        );
    }

    #[test]
    fn base_url_accepts_http_and_https() {
        for url in [
            "https://api.openai.com/v1",
            "http://127.0.0.1:9",
            "http://localhost:11434/v1/",
            "https://[::1]:8443/v1",
        ] {
            assert!(validate_base_url(url).is_ok(), "{url}");
        }
    }

    #[test]
    fn base_url_requires_scheme_and_host() {
        for url in ["", "api.openai.com", "ftp://host", "https://", "http:///v1"] {
            assert!(validate_base_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn base_url_rejects_userinfo() {
        for url in [
            "https://user:pass@api.example.com/v1",
            "https://token@api.example.com",
        ] {
            let err = validate_base_url(url).unwrap_err().to_string();
            assert!(err.contains("--api-key"), "{err}");
            assert!(!err.contains("pass") && !err.contains("token"), "{err}");
        }
    }

    #[test]
    fn base_url_rejects_query_and_fragment() {
        for url in [
            "https://api.example.com/v1?key=abc",
            "https://api.example.com/v1#frag",
            "https://api.example.com?x",
        ] {
            assert!(validate_base_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn base_url_rejects_whitespace() {
        assert!(validate_base_url("https://api.example.com/v1 ").is_err());
        assert!(validate_base_url("https://api exa.com").is_err());
    }

    #[test]
    fn at_sign_in_the_path_is_not_userinfo() {
        assert!(validate_base_url("https://api.example.com/a@b").is_ok());
    }
}
