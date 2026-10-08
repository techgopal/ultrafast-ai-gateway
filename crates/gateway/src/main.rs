use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use ultrafast_gateway::api::auth::bootstrap_admin;
use ultrafast_gateway::api::openapi::spec;
use ultrafast_gateway::api::trimmed_name;
use ultrafast_gateway::app::{router, shutdown_signal, spawn_refresher, AppState};
use ultrafast_gateway::budgets::{self, FLUSH_INTERVAL};
use ultrafast_gateway::catalog::{add_model, describe_model_add, validate_model_name};
use ultrafast_gateway::config::{
    db_path, load_master_key, master_key_from_env_only, parse_database_url, parse_public_url,
    parse_trusted_proxies, restrict_permissions, validate_api_version, validate_base_url,
    validate_database_max_connections, validate_provider_name, DEFAULT_DATABASE_MAX_CONNECTIONS,
};
use ultrafast_gateway::identity::password;
use ultrafast_gateway::logs::{self, LogSink, QUEUE_CAPACITY};
use ultrafast_gateway::otel::{Exporter, OtelConfig};
use ultrafast_gateway::portable;
use ultrafast_gateway::secrets::{generate_key, Cipher};
use ultrafast_gateway::store::{Store, POSTGRES_BACKUP_TEXT};
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
    /// Use this PostgreSQL database (postgres://user:password@host/db) instead
    /// of the SQLite file in the data directory. Then UF_MASTER_KEY is
    /// required, and several gateways may share the database. Prefer the
    /// UF_DATABASE_URL environment variable: a flag value is visible in the
    /// process list and shell history, and the URL holds the password.
    #[arg(long, env = "UF_DATABASE_URL", hide_env_values = true, global = true)]
    database_url: Option<String>,
    /// The most connections one gateway opens to PostgreSQL.
    #[arg(
        long,
        env = "UF_DATABASE_MAX_CONNECTIONS",
        default_value_t = DEFAULT_DATABASE_MAX_CONNECTIONS,
        global = true
    )]
    database_max_connections: u32,
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
        /// the client sent, or the client can choose its own address. Never
        /// list a network that clients can reach directly.
        #[arg(
            long = "trusted-proxy",
            env = "UF_TRUSTED_PROXIES",
            value_name = "CIDR",
            value_delimiter = ','
        )]
        trusted_proxies: Vec<String>,
        /// The address people reach the gateway at, like
        /// https://gateway.example.com. Single sign-on needs it: the
        /// identity provider sends the browser back to
        /// <URL>/api/auth/oidc/callback. Only the address: no path. Plain
        /// http is accepted for localhost, or with --insecure-cookies.
        /// Unset: single sign-on cannot be turned on.
        #[arg(long, env = "UF_PUBLIC_URL", value_name = "URL")]
        public_url: Option<String>,
        /// Serve Prometheus metrics at `GET /metrics` to callers that send
        /// this token as `Authorization: Bearer <token>`. Unset (or empty):
        /// `/metrics` does not exist. Prefer the UF_METRICS_TOKEN environment
        /// variable: a flag value is visible in the process list.
        #[arg(long, env = "UF_METRICS_TOKEN", hide_env_values = true)]
        metrics_token: Option<String>,
        /// Export a trace of every `/v1` call over OTLP/HTTP (JSON) to the
        /// collector at this base URL, like http://localhost:4318 (spans are
        /// posted to <URL>/v1/traces; a URL that already ends with that is
        /// used as it is). Unset: no traces are exported.
        /// A span holds no prompt, answer or credential.
        #[arg(long, env = "UF_OTEL_ENDPOINT")]
        otel_endpoint: Option<String>,
        /// Headers sent with each export, like `authorization=Bearer abc,x-team=a`
        /// (name=value pairs separated by commas). Prefer the UF_OTEL_HEADERS
        /// environment variable: a flag value is visible in the process list.
        #[arg(long, env = "UF_OTEL_HEADERS", hide_env_values = true)]
        otel_headers: Option<String>,
        /// The `service.name` of the exported traces.
        #[arg(long, env = "UF_OTEL_SERVICE_NAME", default_value = "ultrafast")]
        otel_service_name: String,
        /// The share of traces exported, 0.0 to 1.0, decided per trace. A
        /// call whose `traceparent` says sampled is always exported, one that
        /// says not sampled never.
        #[arg(long, env = "UF_OTEL_SAMPLE_RATIO", default_value_t = 1.0)]
        otel_sample_ratio: f64,
    },
    /// Manage providers.
    Provider {
        #[command(subcommand)]
        command: ProviderCommand,
    },
    /// Manage the model catalog.
    Model {
        #[command(subcommand)]
        command: ModelCommand,
    },
    /// Manage virtual keys.
    Key {
        #[command(subcommand)]
        command: KeyCommand,
    },
    /// Write a consistent copy of the database to a file, which must not
    /// exist. The gateway may be running. The copy has no master key: without
    /// the key (`master.key`, or UF_MASTER_KEY) it is useless, as the
    /// provider credentials in it cannot be read.
    Backup { path: PathBuf },
    /// Export and import the configuration as a file.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Print the OpenAPI description of the admin API as JSON.
    Openapi,
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Write providers (without credentials), models and their grants, teams,
    /// routes, limits, budgets and settings to a JSON file. The file holds no
    /// key, token, password or log. The file must not exist.
    Export { file: PathBuf },
    /// Read such a file: create what is missing and update what exists, by
    /// name. Nothing is deleted. A file with errors writes nothing. New
    /// providers have no credential until one is set.
    Import {
        file: PathBuf,
        /// Only say what would be done.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum ProviderCommand {
    /// Add a provider. Call its models as NAME/MODEL once enabled and granted.
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
enum ModelCommand {
    /// Put a model of a provider in the catalog (when it is missing), and
    /// optionally enable it and grant it to everyone.
    Add {
        /// The provider, as named when it was added.
        #[arg(long)]
        provider: String,
        /// The provider's own id for the model.
        #[arg(long)]
        model: String,
        /// Enable the model so it can be called.
        #[arg(long)]
        enable: bool,
        /// Grant the model to everyone (replaces its other grants).
        #[arg(long)]
        everyone: bool,
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
            public_url,
            insecure_cookies,
            otel_endpoint,
            otel_headers,
            otel_service_name,
            otel_sample_ratio,
            ..
        } => {
            serve_address(host, *port)?;
            parse_trusted_proxies(trusted_proxies)?;
            if let Some(url) = public_url.as_deref().filter(|u| !u.trim().is_empty()) {
                parse_public_url(url, *insecure_cookies)?;
            }
            validate_otel(
                otel_endpoint.as_deref(),
                otel_headers.as_deref(),
                otel_service_name,
                *otel_sample_ratio,
            )?;
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
        Command::Model {
            command: ModelCommand::Add { model, .. },
        } => {
            validate_model_name(model).map_err(anyhow::Error::msg)?;
        }
        Command::Key {
            command: KeyCommand::Create { name },
        } => {
            *name = trimmed_name(name).map_err(anyhow::Error::msg)?.to_string();
        }
        Command::Backup { .. } | Command::Config { .. } | Command::Openapi => {}
    }
    Ok(())
}

/// Checks the trace export settings; the messages never show a header value.
fn validate_otel(
    endpoint: Option<&str>,
    headers: Option<&str>,
    service_name: &str,
    ratio: f64,
) -> Result<()> {
    if let Some(endpoint) = endpoint.filter(|e| !e.trim().is_empty()) {
        let parsed = reqwest::Url::parse(endpoint.trim())
            .map_err(|_| anyhow::anyhow!("the OTLP endpoint is not a valid URL"))?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            bail!("the OTLP endpoint must be an http:// or https:// URL");
        }
    }
    if let Some(headers) = headers {
        ultrafast_gateway::otel::parse_headers(headers).map_err(anyhow::Error::msg)?;
    }
    if service_name.trim().is_empty() {
        bail!("the OTLP service name must not be empty");
    }
    if !(0.0..=1.0).contains(&ratio) {
        bail!("the OTLP sample ratio must be between 0.0 and 1.0");
    }
    Ok(())
}

/// How long start-up waits for PostgreSQL.
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Where the database is: the SQLite file in the data directory, or the
/// PostgreSQL database `UF_DATABASE_URL` names.
struct Database {
    url: Option<String>,
    max_connections: u32,
}

impl Database {
    fn of(cli: &Cli) -> Result<Self> {
        Ok(Self {
            url: parse_database_url(cli.database_url.as_deref())?,
            max_connections: validate_database_max_connections(cli.database_max_connections)?,
        })
    }

    fn is_postgres(&self) -> bool {
        self.url.is_some()
    }

    /// Opens PostgreSQL. The error never shows the URL (it holds the password).
    async fn connect(url: &str, max: u32) -> Result<Store> {
        let opened = tokio::time::timeout(CONNECT_TIMEOUT, Store::connect_url(url, max))
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "could not connect to the PostgreSQL database named by UF_DATABASE_URL within {} s",
                    CONNECT_TIMEOUT.as_secs()
                )
            })?;
        opened.context("could not connect to the PostgreSQL database named by UF_DATABASE_URL")
    }
}

/// `ultrafast backup <path>`.
async fn backup_command(data_dir: &Path, database: &Database, path: &Path) -> Result<()> {
    if database.is_postgres() {
        bail!("{POSTGRES_BACKUP_TEXT}");
    }
    let db = db_path(data_dir);
    if !db.exists() {
        bail!("there is no database in {}", data_dir.display());
    }
    let store = Store::open(&db)
        .await
        .context("could not open the database")?;
    store
        .backup_to(path)
        .await
        .with_context(|| format!("could not write the backup to {}", path.display()))?;
    restrict_file(path)?;
    println!(
        "Wrote a backup of the database to {}. It does not hold the master key, and is useless without the master key (master.key in the data directory, or UF_MASTER_KEY): keep that safe, apart from the backup.",
        path.display()
    );
    Ok(())
}

/// The backup is readable by its owner alone, as the database is.
#[cfg(unix)]
fn restrict_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("could not restrict {}", path.display()))
}

#[cfg(not(unix))]
fn restrict_file(_path: &Path) -> Result<()> {
    Ok(())
}

/// `ultrafast config ...`. It needs no master key: the file holds no secret,
/// and a key is neither read nor made.
async fn config_command(
    data_dir: &Path,
    database: &Database,
    command: ConfigCommand,
) -> Result<()> {
    let db = db_path(data_dir);
    match command {
        ConfigCommand::Export { file } => {
            let store = if let Some(url) = &database.url {
                Database::connect(url, database.max_connections).await?
            } else {
                if !db.exists() {
                    bail!("there is no database in {}", data_dir.display());
                }
                Store::open(&db)
                    .await
                    .context("could not open the database")?
            };
            let exported = portable::export(&store).await?;
            let bytes = serde_json::to_vec_pretty(&exported)?;
            let mut out = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&file)
                .with_context(|| {
                    format!(
                        "could not create {} (it must not exist yet)",
                        file.display()
                    )
                })?;
            out.write_all(&bytes)?;
            out.write_all(b"\n")?;
            println!(
                "Wrote the configuration to {}. It holds no credentials, keys or logs.",
                file.display()
            );
        }
        ConfigCommand::Import { file, dry_run } => {
            let length = std::fs::metadata(&file)
                .with_context(|| format!("could not read {}", file.display()))?
                .len();
            if length > portable::MAX_FILE_BYTES as u64 {
                bail!("{} is larger than 8 MiB", file.display());
            }
            let bytes = std::fs::read(&file)
                .with_context(|| format!("could not read {}", file.display()))?;
            let parsed = match portable::parse(&bytes) {
                Ok(parsed) => parsed,
                Err(report) => bail!("{}", report.describe(dry_run)),
            };
            let store = if let Some(url) = &database.url {
                Database::connect(url, database.max_connections).await?
            } else {
                // An import may start a data directory of its own.
                std::fs::create_dir_all(data_dir)
                    .with_context(|| format!("could not create {}", data_dir.display()))?;
                let store = Store::open(&db)
                    .await
                    .context("could not open the database")?;
                restrict_permissions(data_dir)?;
                store
            };
            let actor = portable::Actor {
                user_id: None,
                email: "cli",
                cipher: None,
            };
            let report = portable::import(&store, &parsed, &actor, dry_run).await?;
            println!("{}", report.describe(dry_run));
            if !report.is_clean() {
                bail!("the file was not imported");
            }
            if !dry_run {
                println!("A running gateway picks this up within 30 seconds.");
            }
        }
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
    let database = Database::of(&cli)?;
    // Neither needs the master key: it is not read, and not made.
    match cli.command {
        Command::Config { command } => {
            return config_command(&cli.data_dir, &database, command).await
        }
        Command::Backup { path } => return backup_command(&cli.data_dir, &database, &path).await,
        _ => {}
    }
    let (cipher, store) = if let Some(url) = &database.url {
        // No data directory: nothing to keep a master key in, nothing created.
        let master = master_key_from_env_only(cli.master_key.as_deref())?;
        let cipher = Cipher::from_hex(&master)?;
        (
            cipher,
            Database::connect(url, database.max_connections).await?,
        )
    } else {
        let master = load_master_key(&cli.data_dir, cli.master_key.as_deref())?;
        let cipher = Cipher::from_hex(&master)?;
        let store = Store::open(&db_path(&cli.data_dir))
            .await
            .context("could not open the database")?;
        restrict_permissions(&cli.data_dir)?;
        (cipher, store)
    };

    match cli.command {
        Command::Serve {
            host,
            port,
            insecure_cookies,
            trusted_proxies,
            public_url,
            metrics_token,
            otel_endpoint,
            otel_headers,
            otel_service_name,
            otel_sample_ratio,
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
            if let Some(code) = &state.setup_code {
                // Its own target, so it can be shown with every other info
                // line hidden. It is the only way to the first admin.
                tracing::info!(
                    target: "ultrafast::setup",
                    "Setup code: {code}; open the console to create the first admin."
                );
            }
            let (log_sink, log_queue) = LogSink::channel(QUEUE_CAPACITY);
            let log_stats = log_sink.stats();
            state.sink = Arc::new(log_sink);
            state.metrics.attach_logs(log_stats.clone());
            state.metrics_token = metrics_token
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty());
            state.cookie_secure = !insecure_cookies;
            state.trusted_proxies = parse_trusted_proxies(&trusted_proxies)?;
            state.public_url = public_url
                .filter(|u| !u.trim().is_empty())
                .map(|u| parse_public_url(&u, insecure_cookies))
                .transpose()?;
            state.reload_sign_in().await?;
            if !state.trusted_proxies.is_empty() {
                tracing::info!(
                    count = state.trusted_proxies.len(),
                    "trusting forwarding headers from proxies"
                );
            }
            if insecure_cookies {
                tracing::warn!("session cookies are sent without Secure");
            }
            let (stop, stopped) = tokio::sync::watch::channel(false);
            let mut otel_task = None;
            if let Some(endpoint) = otel_endpoint.filter(|e| !e.trim().is_empty()) {
                let headers = ultrafast_gateway::otel::parse_headers(
                    otel_headers.as_deref().unwrap_or_default(),
                )
                .map_err(anyhow::Error::msg)?;
                let (exporter, task) = Exporter::spawn(
                    OtelConfig {
                        endpoint: endpoint.trim().to_string(),
                        headers,
                        service_name: otel_service_name.trim().to_string(),
                        sample_ratio: otel_sample_ratio,
                    },
                    // Its own client: no redirects, so a custom header is
                    // never sent to another host.
                    ultrafast_gateway::app::http_client(),
                    state.metrics.clone(),
                    stopped.clone(),
                );
                state.otel = Some(exporter);
                otel_task = Some(task);
                tracing::info!(
                    sample_ratio = otel_sample_ratio,
                    "exporting traces over OTLP"
                );
            }
            let (deliverer, alert_task) = ultrafast_gateway::alerts::Deliverer::spawn(
                state.store.clone(),
                state.cipher.clone(),
                state.http.clone(),
                state.metrics.clone(),
                ultrafast_gateway::alerts::DeliveryConfig::default(),
                stopped.clone(),
            );
            let (engine, engine_task) = ultrafast_gateway::alerts::engine::spawn(
                state.store.clone(),
                Some(deliverer.clone()),
                Some(state.health.clone()),
                ultrafast_gateway::alerts::EngineConfig::default(),
                stopped.clone(),
            );
            state.health.watch(engine.health_sender());
            state.alerts = Some(deliverer);
            state.alert_engine = Some(engine);
            let state = Arc::new(state);
            // Before the listener is bound, so the first call is already
            // counted against what was spent before the restart.
            budgets::rebuild(&state, time::OffsetDateTime::now_utc())
                .await
                .context("could not rebuild the budgets from the request logs")?;
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .with_context(|| format!("could not listen on {addr}"))?;
            tracing::info!(%addr, "gateway listening");
            let refresher = spawn_refresher(state.clone(), stopped.clone());
            let log_writer = logs::writer::spawn_accounted(
                state.store.clone(),
                log_queue,
                logs::snapshot_prices(state.clone()),
                log_stats,
                logs::writer::WriterConfig::default(),
                stopped.clone(),
                budgets::accountant(state.clone()),
            );
            let budget_flush = budgets::spawn_flush(state.clone(), FLUSH_INTERVAL, stopped.clone());
            let log_retention = logs::retention::spawn(
                state.store.clone(),
                logs::retention::RetentionConfig::default(),
                stopped,
            );
            let service = router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
            let served = axum::serve(listener, service)
                .with_graceful_shutdown(shutdown_signal())
                .await;
            // Also when serving failed, so the task never outlives the server.
            let _ = stop.send(true);
            let _ = refresher.await;
            // The writer writes what is still queued before the process ends.
            let _ = log_writer.await;
            // Then the traces: the last of them are sent within 5 seconds.
            if let Some(task) = otel_task {
                let _ = task.await;
            }
            let _ = engine_task.await;
            // Deliveries in progress get 5 seconds to finish.
            let _ = alert_task.await;
            // After the writer: what it counted while draining is written too.
            let _ = budget_flush.await;
            budgets::flush(&state).await;
            let _ = log_retention.await;
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
            println!(
                "Added provider '{name}'. Add and enable models with `ultrafast model add`, then call them as {name}/<model>."
            );
        }
        Command::Model {
            command:
                ModelCommand::Add {
                    provider,
                    model,
                    enable,
                    everyone,
                },
        } => {
            let out = add_model(&store, &provider, &model, enable, everyone).await?;
            println!(
                "{}",
                describe_model_add(&out, &provider, &model, enable, everyone)
            );
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
        Command::Backup { .. } | Command::Config { .. } | Command::Openapi => {
            unreachable!("answered before the master key is read")
        }
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
    fn serve_checks_the_public_url() {
        let serve = |url: Option<&str>, insecure_cookies: bool| Command::Serve {
            host: "127.0.0.1".into(),
            port: 3000,
            insecure_cookies,
            trusted_proxies: vec![],
            public_url: url.map(str::to_string),
            metrics_token: None,
            otel_endpoint: None,
            otel_headers: None,
            otel_service_name: "ultrafast".into(),
            otel_sample_ratio: 1.0,
        };
        for (url, insecure, ok) in [
            (None, false, true),
            (Some(""), false, true),
            (Some("https://gateway.example.com"), false, true),
            (Some("gateway.example.com"), false, false),
            (Some("https://u:p@gateway.example.com"), false, false),
            (Some("https://gateway.example.com/gw"), false, false),
            (Some("http://gateway.example.com"), false, false),
            (Some("http://gateway.example.com"), true, true),
            (Some("http://localhost:3000"), false, true),
        ] {
            assert_eq!(
                validate(&mut serve(url, insecure)).is_ok(),
                ok,
                "{url:?} {insecure}"
            );
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
