//! HTTP surface. Split by concern; each submodule contributes a `Router`.

pub mod board;
pub mod health;
pub mod settings;
pub mod task;

use anyhow::{Context, Result};
use axum::Router;
use axum_extra::extract::CookieJar;

use crate::error::AppResult;
use crate::models::User;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(health::router())
        .merge(board::router())
        .merge(settings::router())
        .merge(task::router())
}

/// Identity is a cookie holding a user id and nothing else — no session store,
/// no auth, no permission checks (D8). Everyone on the LAN is trusted.
pub const IDENTITY_COOKIE: &str = "user_id";

/// `Ok(None)` means nobody is signed in here — a normal state that routes
/// answer with the picker. A database failure is an error, not an anonymous
/// visitor, so it propagates instead of being flattened into `None`.
pub fn current_user(state: &AppState, jar: &CookieJar) -> Result<Option<User>> {
    let Some(raw) = jar.get(IDENTITY_COOKIE) else {
        return Ok(None);
    };
    let Ok(id) = raw.value().parse::<i64>() else {
        return Ok(None); // a mangled cookie is not an error, just not an identity
    };
    state
        .db
        .with(|conn| crate::queries::user(conn, id))
        .with_context(|| format!("loading the signed-in user {id}"))
}

/// The app-wide "completed tasks sink to the bottom" setting. Ordering is a
/// property of the column, not of the reader, so everyone sees it the same way.
pub fn move_completed(state: &AppState) -> Result<bool> {
    state
        .db
        .with(|conn| crate::queries::get_flag(conn, crate::queries::MOVE_COMPLETED, true))
        .context("reading the move-completed setting")
}

/// Renders a template. Templates are read from disk at render time and run
/// under strict-undefined, so a typo or a missing context name surfaces here
/// as a 500 with the template named, rather than taking the process down or
/// silently rendering a hole.
pub fn render<S: serde::Serialize>(state: &AppState, name: &str, ctx: S) -> AppResult {
    use axum::response::{Html, IntoResponse};

    let html = state
        .tmpl
        .render(name, ctx)
        .with_context(|| format!("rendering template {name}"))?;
    Ok(Html(html).into_response())
}
