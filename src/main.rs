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
    /// IANA zone name; unset means the calendar's own default (Europe/Berlin).
    timezone: Option<String>,
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
            // `TZ` as well, because that is the name every other container
            // takes and nobody should have to read this file to find that out.
            timezone: std::env::var("PLANNER_TZ")
                .or_else(|_| std::env::var("TZ"))
                .ok()
                .filter(|tz| !tz.trim().is_empty()),
        }
    }
}

/// Make sure the data directory exists and this process can write to it,
/// before anything tries.
///
/// A mismatch between the container's UID and the owner of the mounted host
/// path is the classic way a `TrueNAS` custom app dies, and it has to be named
/// here: `create_dir_all` succeeds on a directory that already exists whatever
/// its permissions, and a mount always exists, so the failure would otherwise
/// surface as an opaque SQLite error a few lines later.
fn ensure_writable(dir: &std::path::Path) -> Result<(), String> {
    let hint = "on TrueNAS the host path must be writable by the container user, default 568:568";

    std::fs::create_dir_all(dir)
        .map_err(|e| format!("cannot create data dir {}: {e} ({hint})", dir.display()))?;

    let probe = dir.join(".write-test");
    std::fs::write(&probe, b"")
        .map_err(|e| format!("data dir {} is not writable: {e} ({hint})", dir.display()))?;
    let _ = std::fs::remove_file(&probe);

    Ok(())
}

/// Resolves when the process is asked to stop: `docker stop`, `systemctl stop`
/// and a plain `kill` all send SIGTERM, and Ctrl-C in a terminal sends SIGINT.
///
/// Unix only, like everything else here: this ships in a Linux container.
async fn shutdown_signal() -> &'static str {
    use tokio::signal::unix::{SignalKind, signal};

    // Registration failing means the process cannot be shut down cleanly at
    // all, which is worth crashing over at startup rather than discovering
    // during a deploy.
    let mut term = signal(SignalKind::terminate()).expect("listening for SIGTERM");
    let mut interrupt = signal(SignalKind::interrupt()).expect("listening for SIGINT");

    tokio::select! {
        _ = term.recv() => "SIGTERM",
        _ = interrupt.recv() => "SIGINT",
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

    if let Some(tz) = &cfg.timezone {
        calendar::set_timezone(tz)?;
    }
    tracing::info!(timezone = %calendar::timezone(), "dates read in");

    ensure_writable(&cfg.data_dir)?;

    let db_path = cfg.data_dir.join("planner.sqlite3");
    let db = Arc::new(Db::open(&db_path)?);
    tracing::info!(path = %db_path.display(), "database ready");

    let state = AppState {
        db: Arc::clone(&db),
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

    // The signal arrives, in-flight requests are allowed to finish, and only
    // then does `serve` return. A poll or a page render takes microseconds, so
    // this costs nothing against the ten seconds `docker stop` allows before
    // it resorts to SIGKILL.
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let signal = shutdown_signal().await;
            tracing::info!(signal, "shutting down");
        })
        .await?;

    // Nothing is writing any more, so the log can be folded back in and the
    // file left whole for whatever snapshots it next.
    db.checkpoint()?;
    tracing::info!("stopped");

    Ok(())
}
