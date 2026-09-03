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
    /// The version this client last saw. 0 on a fresh page.
    #[serde(default)]
    since: u64,
}

/// Answers in microseconds and holds no connection open, so any number of tabs
/// can poll without exhausting the browser's per-origin connection pool — see
/// the note in `events.rs`.
///
/// A tab is told about its own writes too. It has the answer already, so the
/// re-fetch is redundant, but filtering it out needs the client to identify
/// itself on every request and the server to track who caused what — real
/// machinery to save one request on an app that is idle most of the time.
async fn changes(State(state): State<AppState>, Query(poll): Query<Poll>) -> Response {
    let changes = state.changes.since(poll.board, poll.since);

    Json(serde_json::json!({
        "seq": changes.seq,
        "keys": changes.keys,
    }))
    .into_response()
}
