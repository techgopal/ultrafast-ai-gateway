//! Data directory layout, the master key, and provider URL validation.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::secrets::Cipher;

const MASTER_KEY_FILE: &str = "master.key";

pub fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("gateway.db")
}

/// Returns the master key as hex. Uses `from_env` when given. Otherwise reads
/// `master.key` in the data directory, creating it on first use. The data
/// directory is created in both cases, since the database lives there too.
pub fn load_master_key(data_dir: &Path, from_env: Option<&str>) -> Result<String> {
    if let Some(v) = from_env {
        Cipher::from_hex(v).context("UF_MASTER_KEY is not valid")?;
        create_data_dir(data_dir)?;
        return Ok(v.trim().to_string());
    }
    create_data_dir(data_dir)?;
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

/// Checks the API version of an Azure OpenAI provider: `2024-10-21` or
/// `2025-03-01-preview`.
pub fn validate_api_version(version: &str) -> Result<()> {
    let date = version.strip_suffix("-preview").unwrap_or(version);
    let b = date.as_bytes();
    let shaped = b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        });
    if !shaped {
        bail!("API version must look like 2024-10-21 or 2025-03-01-preview");
    }
    Ok(())
}

/// Longest accepted provider name, in characters.
const MAX_PROVIDER_NAME_CHARS: usize = 40;

/// Checks a provider name. Models are called as `NAME/MODEL`, so the name
/// is kept to 1 to 40 of `a-z`, `0-9`, `-` and `_`, starting with a letter
/// or a digit.
pub fn validate_provider_name(name: &str) -> Result<()> {
    let allowed = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_';
    let starts_well = name
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    if !starts_well || name.len() > MAX_PROVIDER_NAME_CHARS || !name.bytes().all(allowed) {
        bail!(
            "provider name must be 1 to 40 characters of a-z, 0-9, '-' and '_', \
             starting with a letter or a digit"
        );
    }
    Ok(())
}

/// Makes the database and its WAL side files readable by the owner only.
/// Files that do not exist are skipped. Does nothing on non-Unix systems.
#[cfg(unix)]
pub fn restrict_permissions(data_dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let db = db_path(data_dir);
    for suffix in ["", "-wal", "-shm"] {
        let mut name = db.clone().into_os_string();
        name.push(suffix);
        let path = PathBuf::from(name);
        match std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(e).with_context(|| format!("could not restrict {}", path.display()));
            }
        }
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn restrict_permissions(_data_dir: &Path) -> Result<()> {
    Ok(())
}

/// Creates the data directory if it is missing. Directories the gateway
/// creates are owner-only; an existing directory is left as it is.
fn create_data_dir(data_dir: &Path) -> Result<()> {
    if data_dir.is_dir() {
        return Ok(());
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(data_dir)
        .with_context(|| format!("could not create {}", data_dir.display()))
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

/// The networks of `--trusted-proxy` / `UF_TRUSTED_PROXIES`, in CIDR
/// notation; a bare address counts as a network of one. Blank entries are
/// skipped. The error names the value that is not valid.
pub fn parse_trusted_proxies(values: &[String]) -> Result<Vec<ipnet::IpNet>> {
    values
        .iter()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(|v| {
            v.parse::<ipnet::IpNet>()
                .or_else(|_| v.parse::<std::net::IpAddr>().map(ipnet::IpNet::from))
                .map_err(|_| {
                    anyhow::anyhow!(
                        "trusted proxy '{v}' is not a network in CIDR notation, such as 10.0.0.0/8"
                    )
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::Cipher;

    #[test]
    fn trusted_proxies_are_networks_and_errors_name_the_value() {
        let list =
            |items: &[&str]| -> Vec<String> { items.iter().map(|s| s.to_string()).collect() };
        let nets =
            parse_trusted_proxies(&list(&["10.0.0.0/8", " fd00::/8 ", "", "192.0.2.1"])).unwrap();
        let nets: Vec<String> = nets.iter().map(|n| n.to_string()).collect();
        assert_eq!(nets, ["10.0.0.0/8", "fd00::/8", "192.0.2.1/32"]);
        assert!(parse_trusted_proxies(&[]).unwrap().is_empty());
        for bad in ["10.0.0.0/33", "nope", "10.0.0/8", "10.0.0.0/8/9"] {
            let err = parse_trusted_proxies(&list(&["10.0.0.0/8", bad])).unwrap_err();
            assert!(err.to_string().contains(&format!("'{bad}'")), "{err}");
        }
    }

    #[test]
    fn env_value_wins_and_nothing_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let master = Cipher::generate_master_hex();
        assert_eq!(load_master_key(dir.path(), Some(&master)).unwrap(), master);
        assert!(!dir.path().join("master.key").exists());
    }

    #[tokio::test]
    async fn env_value_still_creates_a_missing_data_directory() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a/b");
        let master = Cipher::generate_master_hex();
        assert_eq!(load_master_key(&nested, Some(&master)).unwrap(), master);
        assert!(nested.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&nested).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        assert!(!nested.join("master.key").exists());
        crate::store::Store::open(&db_path(&nested)).await.unwrap();
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

    #[cfg(unix)]
    #[test]
    fn restrict_permissions_makes_database_files_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let names = ["gateway.db", "gateway.db-wal", "gateway.db-shm"];
        for n in names {
            let p = dir.path().join(n);
            std::fs::write(&p, "x").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o666)).unwrap();
        }
        restrict_permissions(dir.path()).unwrap();
        for n in names {
            let mode = std::fs::metadata(dir.path().join(n))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "{n}");
        }
    }

    #[test]
    fn restrict_permissions_ignores_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("gateway.db"), "x").unwrap();
        restrict_permissions(dir.path()).unwrap();
        let empty = tempfile::tempdir().unwrap();
        restrict_permissions(empty.path()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn created_data_directory_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a/b");
        load_master_key(&nested, None).unwrap();
        let mode = std::fs::metadata(&nested).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
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
    fn provider_names_are_short_lowercase_slugs() {
        for good in ["a", "9", "openai", "open-ai_2", "0x", &"a".repeat(40)] {
            assert!(validate_provider_name(good).is_ok(), "rejected {good:?}");
        }
        for bad in [
            "",
            " ",
            "Open AI",
            "OpenAI",
            "a/b",
            "-x",
            "_x",
            "a b",
            "a.b",
            " a",
            "a\n",
            "\u{e9}",
            &"a".repeat(41),
        ] {
            assert!(validate_provider_name(bad).is_err(), "accepted {bad:?}");
        }
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
