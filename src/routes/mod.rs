//! HTTP surface. Split by concern; each submodule contributes a `Router`.

pub mod board;
pub mod events;
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
        .merge(events::router())
        .merge(settings::router())
        .merge(task::router())
}

/// Which grid this request came from, taken from the page URL htmx sends on
/// every request.
///
/// A mutation has to answer in the shape of the page that asked: ticking a task
/// off inside a four-week cell must come back as a cell, not as a full column.
/// Deriving it from one header rather than threading a hidden field through
/// every form means no handler can quietly forget it.
pub struct Density(pub &'static str);

impl<S: Sync> axum::extract::FromRequestParts<S> for Density {
    type Rejection = std::convert::Infallible;

    fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> impl std::future::Future<Output = std::result::Result<Self, Self::Rejection>> {
        let from_month = parts
            .headers
            .get("HX-Current-URL")
            .and_then(|v| v.to_str().ok())
            .and_then(|url| url.split('?').next())
            .is_some_and(|path| path.contains("/4w/"));

        std::future::ready(Ok(Self(if from_month {
            crate::views::COMPACT
        } else {
            crate::views::FULL
        })))
    }
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

/// The next theme in the cycle, and the symbol standing for the current one.
///
/// Three states rather than a switch, because "system" is a real choice and
/// the only honest default: it follows whatever the device is already doing.
pub fn theme_cycle(current: &str) -> (&'static str, &'static str) {
    match current {
        "light" => ("dark", "\u{2600}"),  // sun
        "dark" => ("system", "\u{263e}"), // moon
        _ => ("light", "\u{25d0}"),       // half-filled circle
    }
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
