use crate::dashboard;
use axum::{response::Html, routing::get, Router};
use std::net::SocketAddr;

pub async fn run_test_server() {
    // Create a simple router with just the dashboard
    let app = Router::new()
        .route(
            "/",
            get(|| async { Html("<h1>Ultrafast Gateway Test Server</h1>") }),
        )
        .route("/dashboard", get(dashboard::dashboard))
        .route("/health", get(|| async { "OK" }));

    // Start server
    let addr = SocketAddr::from(([127, 0, 0, 1], 3001));
    println!("Test server starting on {addr}");

    axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app)
        .await
        .unwrap();
}
