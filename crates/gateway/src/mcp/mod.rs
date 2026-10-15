//! MCP servers the gateway proxies: the registry's helpers, tool discovery
//! (`sync`) and the client toward an upstream (`upstream`).

use std::collections::BTreeMap;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

use crate::secrets::Cipher;
use crate::store::McpServerRow;

pub mod call;
pub mod catalog;
pub mod combined;
pub mod discover;
pub mod health;
pub mod local;
pub mod project;
pub mod server;
pub mod sync;
pub mod upstream;

/// Whether the operator asked for local servers: `UF_MCP_LOCAL=on`. Whether
/// they run is the supervisor's say (`Supervisor::enabled`): never as root.
pub fn local_requested() -> bool {
    std::env::var("UF_MCP_LOCAL").is_ok_and(|v| v.trim().eq_ignore_ascii_case("on"))
}

/// The longest server name.
pub const MAX_SERVER_NAME: usize = 63;
/// The longest tool name an upstream may give, in bytes.
pub const MAX_TOOL_NAME_BYTES: usize = 128;
/// The prefix of a key allowlist entry that names a tool: `mcp:<server>/<tool>`.
pub const ALLOW_PREFIX: &str = "mcp:";

/// `^[a-z0-9][a-z0-9-]{0,62}$`. No `_` is allowed: a later endpoint
/// separates a server from a tool by it. The message does not repeat what was sent.
pub fn validate_server_name(name: &str) -> anyhow::Result<()> {
    let bytes = name.as_bytes();
    let ok = !bytes.is_empty()
        && bytes.len() <= MAX_SERVER_NAME
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-');
    if !ok {
        anyhow::bail!(
            "name must be 1 to 63 characters: lowercase letters, digits and '-', starting with a letter or digit"
        );
    }
    Ok(())
}

/// The key allowlist entry of a tool.
pub fn allow_entry(server: &str, tool: &str) -> String {
    format!("{ALLOW_PREFIX}{server}/{tool}")
}

/// `(server, tool)` of an allowlist entry that names a tool.
pub fn parse_allow_entry(entry: &str) -> Option<(&str, &str)> {
    let rest = entry.strip_prefix(ALLOW_PREFIX)?;
    let (server, tool) = rest.split_once('/')?;
    (!server.is_empty() && !tool.is_empty()).then_some((server, tool))
}

/// The stored (text) form of a secret: the cipher's output in base64.
/// Whitespace around a pasted secret is not part of it.
pub fn encrypt_secret(cipher: &Cipher, secret: &str) -> String {
    STANDARD.encode(cipher.encrypt(secret.trim().as_bytes()))
}

/// The stored form of an environment value, kept exactly as given.
pub fn encrypt_value(cipher: &Cipher, value: &str) -> String {
    STANDARD.encode(cipher.encrypt(value.as_bytes()))
}

/// The secret of a stored form, or `None` when it cannot be read (another
/// master key, damaged text).
pub fn decrypt_secret(cipher: &Cipher, stored: &str) -> Option<String> {
    let bytes = STANDARD.decode(stored).ok()?;
    let plain = cipher.decrypt(&bytes).ok()?;
    String::from_utf8(plain).ok()
}

/// Whether the server can be called: it needs no secret, or its secret is
/// stored and can be read.
pub fn server_usable(row: &McpServerRow, cipher: &Cipher) -> bool {
    row.auth_kind == "none"
        || row
            .auth_secret_enc
            .as_deref()
            .is_some_and(|s| decrypt_secret(cipher, s).is_some())
}

/// Environment variables the gateway sets itself, or that change what a
/// program loads: a local server may not be given them.
fn reserved_env(name: &str) -> bool {
    matches!(name, "PATH" | "HOME" | "LANG")
        || name.starts_with("LD_")
        || name.starts_with("DYLD_")
        || name.starts_with("UF_")
}

pub const MAX_ENV_VARS: usize = 32;
pub const MAX_ENV_VALUE_BYTES: usize = 4096;
pub const MAX_ARGS: usize = 64;
pub const MAX_ARG_BYTES: usize = 1024;

/// Checks a name of an environment variable of a local server.
pub fn check_env_name(name: &str) -> anyhow::Result<()> {
    let b = name.as_bytes();
    let ok = !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_alphabetic() || b[0] == b'_')
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_');
    if !ok {
        anyhow::bail!("names are letters, digits and '_', not starting with a digit, at most 64");
    }
    if reserved_env(name) {
        anyhow::bail!("the gateway sets this one itself (PATH, HOME, LANG), or it changes what a program loads (LD_*, DYLD_*), or it is the gateway's own (UF_*)");
    }
    Ok(())
}

/// Checks the program of a local server: what is run, never a shell line.
pub fn check_command(command: &str, args: &[String], fields: &mut BTreeMap<String, String>) {
    if command.is_empty()
        || command.len() > 512
        || command.chars().any(|c| c == '\0' || c == '\n' || c == '\r')
    {
        fields.insert(
            "command".to_string(),
            "command is the program to run: at most 512 characters, one line".to_string(),
        );
    }
    if args.len() > MAX_ARGS
        || args
            .iter()
            .any(|a| a.len() > MAX_ARG_BYTES || a.contains('\0'))
    {
        fields.insert(
            "args".to_string(),
            format!("at most {MAX_ARGS} arguments of at most {MAX_ARG_BYTES} bytes"),
        );
    }
}

/// The kinds of upstream authentication.
pub const AUTH_KINDS: [&str; 3] = ["none", "bearer", "header"];

/// Checks `auth_kind` with its header, the way the API and the import
/// both need it. Fields are named like the request's.
pub fn check_auth(kind: &str, header: Option<&str>, fields: &mut BTreeMap<String, String>) {
    if !AUTH_KINDS.contains(&kind) {
        fields.insert(
            "auth_kind".to_string(),
            "auth_kind must be none, bearer or header".to_string(),
        );
        return;
    }
    match (kind, header) {
        ("header", None) => {
            fields.insert(
                "auth_header".to_string(),
                "auth_header is required when auth_kind is header".to_string(),
            );
        }
        ("header", Some(h)) => {
            let reserved = [
                "host",
                "content-length",
                "content-type",
                "accept",
                "transfer-encoding",
                "connection",
                "mcp-protocol-version",
            ];
            let valid = h.len() <= 64
                && reqwest::header::HeaderName::from_bytes(h.as_bytes()).is_ok()
                && !reserved.contains(&h.to_ascii_lowercase().as_str());
            if !valid {
                fields.insert(
                    "auth_header".to_string(),
                    "auth_header must be a valid header name of at most 64 characters that the gateway does not set itself".to_string(),
                );
            }
        }
        (_, Some(_)) => {
            fields.insert(
                "auth_header".to_string(),
                "auth_header is only for auth_kind header".to_string(),
            );
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_names() {
        for ok in ["a", "0", "git-hub", "a-b-c", &"a".repeat(63)] {
            assert!(validate_server_name(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "-a",
            "A",
            "a_b",
            "a b",
            "a/b",
            "a.b",
            "é",
            &"a".repeat(64),
        ] {
            assert!(validate_server_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn allow_entries_split_at_the_first_slash() {
        assert_eq!(parse_allow_entry("mcp:git/search"), Some(("git", "search")));
        assert_eq!(parse_allow_entry("mcp:git/a/b"), Some(("git", "a/b")));
        assert_eq!(parse_allow_entry("mcp:git"), None);
        assert_eq!(parse_allow_entry("mcp:/x"), None);
        assert_eq!(parse_allow_entry("openai/gpt-4o"), None);
        assert_eq!(allow_entry("git", "a/b"), "mcp:git/a/b");
    }

    #[test]
    fn a_secret_round_trips_and_is_not_in_the_stored_text() {
        let cipher = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
        let stored = encrypt_secret(&cipher, "  tok-123 ");
        assert!(!stored.contains("tok-123"));
        assert_eq!(decrypt_secret(&cipher, &stored).as_deref(), Some("tok-123"));
        let other = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
        assert_eq!(decrypt_secret(&other, &stored), None);
        assert_eq!(decrypt_secret(&cipher, "not base64!"), None);
    }

    #[test]
    fn auth_checks() {
        let run = |kind, header| {
            let mut f = BTreeMap::new();
            check_auth(kind, header, &mut f);
            f
        };
        assert!(run("none", None).is_empty());
        assert!(run("bearer", None).is_empty());
        assert!(run("header", Some("x-api-key")).is_empty());
        assert!(run("header", None).contains_key("auth_header"));
        assert!(run("header", Some("bad header")).contains_key("auth_header"));
        assert!(run("header", Some("Host")).contains_key("auth_header"));
        assert!(run("bearer", Some("x")).contains_key("auth_header"));
        assert!(run("basic", None).contains_key("auth_kind"));
    }
}
