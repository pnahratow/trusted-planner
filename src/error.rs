//! One error type for handlers.
//!
//! Anything a handler can fail at converts into `AppError` with `?`, which
//! renders a 500 and logs the whole `anyhow` chain. Before this existed the
//! route layer had to match every `Result` by hand, and the tedium showed:
//! several paths answered a database failure with `unwrap_or_default()` or a
//! redirect to `/`, so a broken query rendered an empty week or bounced the
//! browser instead of saying anything.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

pub struct AppError(anyhow::Error);

/// Lets `?` lift any error that `anyhow` accepts.
impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // `{:#}` prints the context chain on one line: the innermost SQLite
        // message plus every `.context()` the call stack added.
        tracing::error!(error = format!("{:#}", self.0), "request failed");
        (StatusCode::INTERNAL_SERVER_ERROR, format!("{:#}\n", self.0)).into_response()
    }
}

/// Handler result: a rendered response, or a 500 with context.
pub type AppResult<T = Response> = Result<T, AppError>;
