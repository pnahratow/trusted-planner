//! Liveness and a placeholder landing page, so phase 1 is observably working.

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/healthz", get(healthz))
}

/// Confirms the process is up *and* the database answers — a liveness check
/// that cannot pass while the data dir is unwritable is worth more than one
/// that only proves the socket is open.
async fn healthz(State(state): State<AppState>) -> impl IntoResponse {
    match state
        .db
        .with(|conn| conn.query_row("SELECT 1", [], |r| r.get::<_, i64>(0)))
    {
        Ok(_) => (StatusCode::OK, "ok\n").into_response(),
        Err(e) => {
            tracing::error!(error = %e, "health check failed");
            (StatusCode::SERVICE_UNAVAILABLE, "database unavailable\n").into_response()
        }
    }
}
