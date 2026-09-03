//! Trusted Planner — a self-hosted week/month planner for a trusted LAN.
//!
//! Server-authoritative by design: clients send intent, the server owns state
//! and ordering and renders the truth back.

mod calendar;
mod db;
mod error;
mod events;
mod models;
mod queries;
mod routes;
mod templates;
mod views;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::db::Db;
use crate::events::ChangeLog;
use crate::templates::Templates;

/// Everything a handler needs, cloned cheaply per request.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub tmpl: Templates,
    /// Column invalidations, polled by connected browsers.
    pub changes: Arc<ChangeLog>,
}

/// Runtime configuration, all overridable from the environment so the same
/// image works locally and on `TrueNAS`.
struct Config {
    data_dir: PathBuf,
    template_dir: PathBuf,
    static_dir: PathBuf,
    port: u16,
}

impl Config {
    fn from_env() -> Self {
        fn path(key: &str, default: &str) -> PathBuf {
            std::env::var(key).unwrap_or_else(|_| default.into()).into()
        }

        Self {
            data_dir: path("PLANNER_DATA_DIR", "/data"),
            template_dir: path("PLANNER_TEMPLATE_DIR", "templates"),
            static_dir: path("PLANNER_STATIC_DIR", "static"),
            port: std::env::var("PLANNER_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8080),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "trusted_planner=info,tower_http=info".into()),
        )
        .init();

    let cfg = Config::from_env();

    // Fail loudly here rather than on the first write: a data dir the
    // container user cannot create is the classic TrueNAS UID 568 symptom.
    std::fs::create_dir_all(&cfg.data_dir).map_err(|e| {
        format!(
            "cannot create data dir {}: {e} \
             (on TrueNAS the host path must be writable by the container user, default 568:568)",
            cfg.data_dir.display()
        )
    })?;

    let db_path = cfg.data_dir.join("planner.sqlite3");
    let db = Arc::new(Db::open(&db_path)?);
    tracing::info!(path = %db_path.display(), "database ready");

    let state = AppState {
        db,
        tmpl: Templates::new(&cfg.template_dir),
        changes: Arc::new(ChangeLog::new()),
    };

    let app = Router::new()
        .merge(routes::router())
        .nest_service("/static", ServeDir::new(&cfg.static_dir))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "listening");
    axum::serve(listener, app).await?;

    Ok(())
}
