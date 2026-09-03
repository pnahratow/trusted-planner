//! The endpoint clients poll for column invalidations.

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/changes", get(changes))
}

#[derive(Deserialize)]
struct Poll {
    board: i64,
    /// Where this client got to. 0 on a fresh page, which is not a gap.
    #[serde(default)]
    since: u64,
    /// This tab's id, so its own writes are not echoed back to it.
    #[serde(default)]
    client: Option<String>,
}

/// Answers in a few hundred microseconds and holds no connection open, so any
/// number of tabs can poll without exhausting the browser's per-origin
/// connection pool — see the note in `events.rs`.
async fn changes(State(state): State<AppState>, Query(poll): Query<Poll>) -> Response {
    let changes = state
        .changes
        .since(poll.board, poll.since, poll.client.as_deref());

    Json(serde_json::json!({
        "seq": changes.seq,
        "keys": changes.keys,
        "resync": changes.resync,
    }))
    .into_response()
}
