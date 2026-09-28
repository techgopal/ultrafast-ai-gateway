use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use ultrafast_gateway::app::{http_client, router, AppState, DEFAULT_MAX_BODY_BYTES};
use ultrafast_gateway::config::{db_path, load_master_key, validate_base_url};
use ultrafast_gateway::secrets::{generate_key, Cipher};
use ultrafast_gateway::store::Store;
use ultrafast_translate::provider::ProviderKind;

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
}

#[derive(Subcommand)]
enum ProviderCommand {
    /// Add a provider. Call its models as NAME/MODEL.
    Add {
        #[arg(long)]
        name: String,
        /// "openai" (also for OpenAI-compatible APIs) or "anthropic".
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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let master = load_master_key(&cli.data_dir, cli.master_key.as_deref())?;
    let cipher = Cipher::from_hex(&master)?;
    let store = Store::open(&db_path(&cli.data_dir))
        .await
        .context("could not open the database")?;

    match cli.command {
        Command::Serve { host, port } => {
            let addr: SocketAddr = format!("{host}:{port}")
                .parse()
                .with_context(|| format!("'{host}:{port}' is not a valid address"))?;
            let state = Arc::new(AppState {
                store,
                cipher,
                http: http_client(),
                max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            });
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .with_context(|| format!("could not listen on {addr}"))?;
            tracing::info!(%addr, "gateway listening");
            axum::serve(listener, router(state))
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
        }
        Command::Provider {
            command:
                ProviderCommand::Add {
                    name,
                    kind,
                    base_url,
                    api_key,
                },
        } => {
            if name.is_empty() || name.contains('/') {
                bail!("provider name must not be empty or contain '/'");
            }
            if ProviderKind::parse(&kind).is_none() {
                bail!("unknown kind '{kind}'. Use 'openai' or 'anthropic'");
            }
            validate_base_url(&base_url)?;
            let credential = api_key.as_deref().map(|k| cipher.encrypt(k.as_bytes()));
            store
                .insert_provider(&name, &kind, &base_url, credential.as_deref())
                .await
                .with_context(|| format!("could not add provider '{name}' (is the name taken?)"))?;
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
    }
    Ok(())
}
