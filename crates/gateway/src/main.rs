use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use ultrafast_gateway::api::auth::bootstrap_admin;
use ultrafast_gateway::api::openapi::spec;
use ultrafast_gateway::api::trimmed_name;
use ultrafast_gateway::app::{router, shutdown_signal, spawn_refresher, AppState};
use ultrafast_gateway::config::{
    db_path, load_master_key, parse_trusted_proxies, restrict_permissions, validate_api_version,
    validate_base_url, validate_provider_name,
};
use ultrafast_gateway::identity::password;
use ultrafast_gateway::secrets::{generate_key, Cipher};
use ultrafast_gateway::store::Store;
use ultrafast_translate::provider::{ProviderKind, DEFAULT_AZURE_API_VERSION};

#[derive(Parser)]
#[command(name = "ultrafast", version, about = "Ultrafast AI gateway")]
struct Cli {
    /// Directory for the database and master key.
    #[arg(long, env = "UF_DATA_DIR", default_value = "./data", global = true)]
    data_dir: PathBuf,
    /// 64 hex characters. Generated into the data directory when unset.
    /// Prefer the UF_MASTER_KEY environment variable: a flag value is visible
    /// in the process list and shell history.
    #[arg(long, env = "UF_MASTER_KEY", hide_env_values = true, global = true)]
    master_key: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the gateway.
    Serve {
        #[arg(long, env = "UF_HOST", default_value = "127.0.0.1")]
        host: String,
        #[arg(long, env = "UF_PORT", default_value_t = 3000)]
        port: u16,
        /// Send the session cookie without `Secure`, for plain HTTP during
        /// development. Never use this on a public address.
        #[arg(long, env = "UF_INSECURE_COOKIES")]
        insecure_cookies: bool,
        /// A network (CIDR) of reverse proxies whose `CF-Connecting-IP` and
        /// `X-Forwarded-For` headers are believed, to find the client's
        /// address. Repeat the flag, or separate with commas in
        /// UF_TRUSTED_PROXIES. Each of these proxies must set or overwrite
        /// `CF-Connecting-IP` and `X-Forwarded-For` itself, never pass on what
        /// the client sent, or the client can choose its own address. Never list a network that clients can reach
        /// directly.
        #[arg(
            long = "trusted-proxy",
            env = "UF_TRUSTED_PROXIES",
            value_name = "CIDR",
            value_delimiter = ','
        )]
        trusted_proxies: Vec<String>,
    },
    /// Manage providers.
    Provider {
        #[command(subcommand)]
        command: ProviderCommand,
    },
    /// Manage virtual keys.
    Key {
        #[command(subcommand)]
        command: KeyCommand,
    },
    /// Print the OpenAPI description of the admin API as JSON.
    Openapi,
}

#[derive(Subcommand)]
enum ProviderCommand {
    /// Add a provider. Call its models as NAME/MODEL.
    Add {
        #[arg(long)]
        name: String,
        /// "openai" (also for OpenAI-compatible APIs), "anthropic", "gemini" or "azure".
        #[arg(long)]
        kind: String,
        /// http:// or https:// URL without credentials, query or fragment.
        #[arg(long)]
        base_url: String,
        /// Provider API key. Prefer the UF_PROVIDER_API_KEY environment
        /// variable: a flag value is visible in the process list and shell
        /// history.
        #[arg(long, env = "UF_PROVIDER_API_KEY", hide_env_values = true)]
        api_key: Option<String>,
        /// Azure OpenAI only, like 2024-10-21. That is the default.
        #[arg(long)]
        api_version: Option<String>,
    },
}

#[derive(Subcommand)]
enum KeyCommand {
    /// Create a virtual key. The key is printed once.
    Create {
        #[arg(long)]
        name: String,
    },
}

const ADMIN_EMAIL: &str = "UF_ADMIN_EMAIL";
const ADMIN_PASSWORD: &str = "UF_ADMIN_PASSWORD";

/// Reads an environment variable. The error names the variable and never
/// shows its value.
fn env_value(name: &str) -> Result<Option<String>> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => bail!("{name} is not valid UTF-8"),
    }
}

/// Checks every argument of the command, and trims what is stored trimmed.
fn validate(command: &mut Command) -> Result<()> {
    match command {
        Command::Serve {
            host,
            port,
            trusted_proxies,
            ..
        } => {
            serve_address(host, *port)?;
            parse_trusted_proxies(trusted_proxies)?;
        }
        Command::Provider {
            command:
                ProviderCommand::Add {
                    name,
                    kind,
                    base_url,
                    api_key,
                    api_version,
                },
        } => {
            validate_provider_name(name)?;
            let Some(parsed) = ProviderKind::parse(kind) else {
                bail!("unknown kind '{kind}'. Use 'openai', 'anthropic', 'gemini' or 'azure'");
            };
            match (parsed, api_version.as_deref()) {
                (ProviderKind::Azure, Some(version)) => validate_api_version(version)?,
                (ProviderKind::Azure, None) => {}
                (_, Some(_)) => bail!("--api-version is only for Azure OpenAI providers"),
                (_, None) => {}
            }
            validate_base_url(base_url)?;
            if api_key.as_deref().is_some_and(|k| k.trim().is_empty()) {
                bail!("the API key must not be empty; leave it out for a provider without one");
            }
        }
        Command::Key {
            command: KeyCommand::Create { name },
        } => {
            *name = trimmed_name(name).map_err(anyhow::Error::msg)?.to_string();
        }
        Command::Openapi => {}
    }
    Ok(())
}

fn serve_address(host: &str, port: u16) -> Result<SocketAddr> {
    format!("{host}:{port}")
        .parse()
        .with_context(|| format!("'{host}:{port}' is not a valid address"))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let mut cli = Cli::parse();
    // Before anything is read or created, so a refused command leaves no files.
    validate(&mut cli.command)?;
    // Answered from the code alone: nothing is read and nothing is created.
    if matches!(cli.command, Command::Openapi) {
        println!("{}", serde_json::to_string_pretty(&spec())?);
        return Ok(());
    }
    let master = load_master_key(&cli.data_dir, cli.master_key.as_deref())?;
    let cipher = Cipher::from_hex(&master)?;
    let store = Store::open(&db_path(&cli.data_dir))
        .await
        .context("could not open the database")?;
    restrict_permissions(&cli.data_dir)?;

    match cli.command {
        Command::Serve {
            host,
            port,
            insecure_cookies,
            trusted_proxies,
        } => {
            let addr = serve_address(&host, port)?;
            tokio::task::spawn_blocking(password::warm_up)
                .await?
                .context("password hashing does not work")?;
            // Read here rather than as flags: a flag value is visible in the
            // process list and shell history.
            bootstrap_admin(&store, env_value(ADMIN_EMAIL)?, env_value(ADMIN_PASSWORD)?).await?;
            let expired = store.delete_expired_sessions().await?;
            tracing::debug!(expired, "removed expired sessions");
            let mut state = AppState::new(store, cipher).await?;
            state.cookie_secure = !insecure_cookies;
            state.trusted_proxies = parse_trusted_proxies(&trusted_proxies)?;
            if !state.trusted_proxies.is_empty() {
                tracing::info!(
                    count = state.trusted_proxies.len(),
                    "trusting forwarding headers from proxies"
                );
            }
            if insecure_cookies {
                tracing::warn!("session cookies are sent without Secure");
            }
            let state = Arc::new(state);
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .with_context(|| format!("could not listen on {addr}"))?;
            tracing::info!(%addr, "gateway listening");
            let (stop, stopped) = tokio::sync::watch::channel(false);
            let refresher = spawn_refresher(state.clone(), stopped);
            let service = router(state).into_make_service_with_connect_info::<SocketAddr>();
            let served = axum::serve(listener, service)
                .with_graceful_shutdown(shutdown_signal())
                .await;
            // Also when serving failed, so the task never outlives the server.
            let _ = stop.send(true);
            let _ = refresher.await;
            served?;
        }
        Command::Provider {
            command:
                ProviderCommand::Add {
                    name,
                    kind,
                    base_url,
                    api_key,
                    api_version,
                },
        } => {
            // An Azure provider without a version gets the default one.
            let api_version = (ProviderKind::parse(&kind) == Some(ProviderKind::Azure))
                .then(|| api_version.unwrap_or_else(|| DEFAULT_AZURE_API_VERSION.to_string()));
            // Whitespace around a pasted key is not part of it.
            let credential = api_key
                .as_deref()
                .map(|k| cipher.encrypt(k.trim().as_bytes()));
            let mut tx = store.begin().await?;
            tx.insert_provider_versioned(
                &name,
                &kind,
                &base_url,
                credential.as_deref(),
                api_version.as_deref(),
            )
            .await
            .with_context(|| format!("could not add provider '{name}' (is the name taken?)"))?;
            tx.commit().await.context("could not save the provider")?;
            println!("Added provider '{name}'. Call its models as {name}/<model>.");
        }
        Command::Key {
            command: KeyCommand::Create { name },
        } => {
            let key = generate_key();
            store
                .insert_key(&name, &key.hash, &key.display, None)
                .await?;
            println!("Created key '{name}'. Copy it now; it is not shown again:");
            println!("{}", key.full);
        }
        Command::Openapi => unreachable!("answered before the data directory is opened"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(kind: &str, api_version: Option<&str>) -> Command {
        Command::Provider {
            command: ProviderCommand::Add {
                name: "p".into(),
                kind: kind.into(),
                base_url: "https://x.example.com".into(),
                api_key: None,
                api_version: api_version.map(str::to_string),
            },
        }
    }

    #[test]
    fn provider_add_takes_the_new_kinds_and_an_azure_api_version() {
        for (kind, version, ok) in [
            ("gemini", None, true),
            ("azure", None, true),
            ("azure", Some("2025-03-01-preview"), true),
            ("azure", Some("latest"), false),
            ("openai", Some("2024-10-21"), false),
            ("palm", None, false),
        ] {
            assert_eq!(
                validate(&mut add(kind, version)).is_ok(),
                ok,
                "{kind} {version:?}"
            );
        }
    }
}
