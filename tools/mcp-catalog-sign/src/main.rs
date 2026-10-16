//! Release tooling for the MCP catalog (`docs/mcp-catalog/`).
//!
//! ```text
//! mcp-catalog-sign keygen                 prints a new private key (the seed, base64) and its public key
//! mcp-catalog-sign public                 reads the private key from MCP_CATALOG_SIGNING_KEY, prints the public key
//! mcp-catalog-sign sign <file>            signs the exact bytes of <file>, writes <file>.sig (base64)
//! mcp-catalog-sign verify <file> <pubkey> checks <file> against <file>.sig and the public key in <pubkey>
//! ```
//!
//! The private key is only ever read from the environment variable
//! `MCP_CATALOG_SIGNING_KEY` (a GitHub secret in CI) and printed once by
//! `keygen`; it is never written to a file here.

use std::io::Read;
use std::process::ExitCode;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

fn signing_key() -> Result<SigningKey, String> {
    let raw = std::env::var("MCP_CATALOG_SIGNING_KEY")
        .map_err(|_| "MCP_CATALOG_SIGNING_KEY is not set".to_string())?;
    let bytes = STANDARD
        .decode(raw.trim())
        .map_err(|_| "MCP_CATALOG_SIGNING_KEY is not base64".to_string())?;
    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "MCP_CATALOG_SIGNING_KEY must be 32 bytes".to_string())?;
    Ok(SigningKey::from_bytes(&seed))
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("keygen") => {
            let mut seed = [0u8; 32];
            std::fs::File::open("/dev/urandom")
                .and_then(|mut f| f.read_exact(&mut seed))
                .map_err(|e| format!("could not read /dev/urandom: {e}"))?;
            let key = SigningKey::from_bytes(&seed);
            println!("private (secret, shown once): {}", STANDARD.encode(seed));
            println!(
                "public: {}",
                STANDARD.encode(key.verifying_key().as_bytes())
            );
            Ok(())
        }
        Some("public") => {
            println!(
                "{}",
                STANDARD.encode(signing_key()?.verifying_key().as_bytes())
            );
            Ok(())
        }
        Some("sign") => {
            let path = args.get(1).ok_or("usage: sign <file>")?;
            let data = std::fs::read(path).map_err(|e| format!("could not read {path}: {e}"))?;
            let signature = signing_key()?.sign(&data);
            std::fs::write(
                format!("{path}.sig"),
                format!("{}\n", STANDARD.encode(signature.to_bytes())),
            )
            .map_err(|e| format!("could not write {path}.sig: {e}"))?;
            Ok(())
        }
        Some("verify") => {
            let (path, key_path) = match (args.get(1), args.get(2)) {
                (Some(a), Some(b)) => (a, b),
                _ => return Err("usage: verify <file> <pubkey-file>".into()),
            };
            let data = std::fs::read(path).map_err(|e| format!("could not read {path}: {e}"))?;
            let sig = std::fs::read_to_string(format!("{path}.sig"))
                .map_err(|e| format!("could not read {path}.sig: {e}"))?;
            let key = std::fs::read_to_string(key_path)
                .map_err(|e| format!("could not read {key_path}: {e}"))?;
            let key: [u8; 32] = STANDARD
                .decode(key.trim())
                .map_err(|_| "the public key is not base64".to_string())?
                .try_into()
                .map_err(|_| "the public key must be 32 bytes".to_string())?;
            let key = VerifyingKey::from_bytes(&key).map_err(|_| "bad public key".to_string())?;
            let sig: [u8; 64] = STANDARD
                .decode(sig.trim())
                .map_err(|_| "the signature is not base64".to_string())?
                .try_into()
                .map_err(|_| "the signature must be 64 bytes".to_string())?;
            key.verify(&data, &Signature::from_bytes(&sig))
                .map_err(|_| "the signature does not match".to_string())
        }
        _ => Err(
            "usage: mcp-catalog-sign keygen | public | sign <file> | verify <file> <pubkey>".into(),
        ),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
