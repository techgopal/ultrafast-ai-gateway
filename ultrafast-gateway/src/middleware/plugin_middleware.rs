use crate::server::AppState;
use axum::body::Body;
use axum::extract::State;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use std::time::Instant;

pub async fn plugin_middleware(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let start = Instant::now();

    // Run before_request plugins
    if let Err(e) = state.plugin_manager.before_request(&mut req).await {
        tracing::error!("Plugin before_request error: {}", e);
        return Response::builder()
            .status(500)
            .body(Body::from(format!("Plugin error: {e}")))
            .unwrap();
    }

    // Process the request
    let mut response = next.run(req).await;

    // Run after_response plugins
    if let Err(e) = state.plugin_manager.after_response(&mut response).await {
        tracing::error!("Plugin after_response error: {}", e);
    }

    let latency = start.elapsed();
    tracing::debug!("Plugin middleware processed in {:?}", latency);

    response
}
