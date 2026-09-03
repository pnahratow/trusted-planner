//! HTTP surface. Split by concern; each submodule contributes a `Router`.

pub mod board;
pub mod health;
pub mod settings;
pub mod task;

use axum::Router;
use axum_extra::extract::CookieJar;

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

pub fn current_user(state: &AppState, jar: &CookieJar) -> Option<User> {
    let id: i64 = jar.get(IDENTITY_COOKIE)?.value().parse().ok()?;
    state.db.with(|conn| crate::queries::user(conn, id)).ok()?
}

/// Renders a template or turns the error into a 500 — templates are edited live,
/// so a typo in one must not take the process down.
pub fn render<S: serde::Serialize>(
    state: &AppState,
    name: &str,
    ctx: S,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::{Html, IntoResponse};

    match state.tmpl.render(name, ctx) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!(template = name, error = %e, "render failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("template error in {name}: {e}\n"),
            )
                .into_response()
        }
    }
}
