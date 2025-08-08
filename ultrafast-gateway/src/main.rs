//! # Ultrafast Gateway Binary
//!
//! This is the main binary entry point for the Ultrafast Gateway server.
//! It provides a high-performance AI gateway that unifies multiple LLM providers
//! through a single, enterprise-grade API.
//!
//! ## Usage
//!
//! ```bash
//! # Basic usage with default configuration
//! ultrafast-gateway
//!
//! # Custom configuration file
//! ultrafast-gateway --config my-config.toml
//!
//! # Custom host and port
//! ultrafast-gateway --host 0.0.0.0 --port 8080
//!
//! # Debug logging
//! ultrafast-gateway --log-level debug
//! ```
//!
//! ## Command Line Arguments
//!
//! - `--config, -c`: Path to configuration file (default: config.toml)
//! - `--port, -p`: Server port (default: 3000)
//! - `--host`: Server host address (default: 127.0.0.1)
//! - `--log-level`: Logging level (default: info)
//!
//! ## Configuration
//!
//! The gateway uses TOML configuration files. See the documentation for
//! detailed configuration options and examples.
//!
//! ## Environment Variables
//!
//! The following environment variables can be used to override configuration:
//!
//! - `GATEWAY_CONFIG_PATH`: Path to configuration file
//! - `GATEWAY_HOST`: Server host address
//! - `GATEWAY_PORT`: Server port
//! - `RUST_LOG`: Logging level
//!
//! ## Health Check
//!
//! Once started, the server provides a health check endpoint at `/health`
//! that returns the service status and uptime information.

use clap::Parser;
use std::net::SocketAddr;
use ultrafast_gateway::{config::Config, server::create_server};

/// Command line arguments for the Ultrafast Gateway server.
///
/// This struct defines all the command line options that can be passed
/// to the gateway binary, with sensible defaults for production use.
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the configuration file.
    ///
    /// The configuration file should be in TOML format and contain
    /// server settings, provider configurations, and authentication
    /// parameters.
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    /// Port number for the HTTP server.
    ///
    /// The gateway will listen for incoming requests on this port.
    /// Make sure the port is available and not blocked by firewall.
    #[arg(short, long, default_value_t = 3000)]
    port: u16,

    /// Host address to bind the server to.
    ///
    /// Use "0.0.0.0" to bind to all interfaces, or "127.0.0.1"
    /// for localhost only. For production, consider using a reverse
    /// proxy like nginx.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Logging level for the application.
    ///
    /// Available levels: trace, debug, info, warn, error
    /// Use "debug" for development and "info" for production.
    #[arg(long, default_value = "info")]
    log_level: String,
}

/// Main entry point for the Ultrafast Gateway server.
///
/// This function:
/// 1. Parses command line arguments
/// 2. Initializes logging and tracing
/// 3. Loads and validates configuration
/// 4. Creates and starts the HTTP server
/// 5. Handles graceful shutdown
///
/// # Errors
///
/// Returns an error if:
/// - Configuration file cannot be loaded or is invalid
/// - Server cannot be created or started
/// - Network binding fails
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Initialize tracing with the specified log level
    // This sets up structured logging for the entire application
    tracing_subscriber::fmt()
        .with_env_filter(&args.log_level)
        .init();

    // Load configuration from the specified file
    // The configuration includes server settings, provider configs, and auth settings
    let config = Config::load(&args.config)?;

    // Validate the configuration to ensure all required fields are present
    // and that the configuration is consistent and valid
    config.validate()?;

    // Create the HTTP server with the loaded configuration
    // This sets up all routes, middleware, and handlers
    let app = create_server(config).await?;

    // Parse the host address and create a socket address
    let addr = SocketAddr::new(args.host.parse()?, args.port);
    tracing::info!("Starting Ultrafast Gateway server on {}", addr);

    // Bind to the specified address and start serving requests
    // The server will run until interrupted (Ctrl+C)
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
