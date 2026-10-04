//! Application state, router assembly and the middleware stack.

use std::{
    collections::HashMap,
    future::Future,
    net::SocketAddr,
    ops::Deref,
    sync::{Arc, Mutex},
    time::Instant,
};

use axum::{
    extract::{Request, State},
    http::{HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::json;
use tokio::net::TcpListener;
use tower_http::catch_panic::CatchPanicLayer;
use tracing::{field::Empty, Instrument};

use crate::{
    api,
    audit::AuditLog,
    auth::{self, session::SqliteSessionStore},
    config::ServeConfig,
    db::Db,
    digest::Digest,
    registry::{self, UploadManager},
    storage::{self, Storage},
    tasks, ui,
};

pub(crate) struct StateInner {
    pub cfg: ServeConfig,
    pub db: Db,
    pub storage: Arc<dyn Storage>,
    pub uploads: UploadManager,
    pub audit: AuditLog,
    /// HTTP client for GitHub (no redirects, as OAuth token requests require).
    pub oauth_http: reqwest::Client,
    /// Throttles `tokens.last_used_at` writes.
    pub token_touches: Mutex<HashMap<i64, Instant>>,
    /// One garbage collection at a time per process.
    pub gc_lock: tokio::sync::Mutex<()>,
    pub started_at: String,
}

#[derive(Clone)]
pub(crate) struct AppState(Arc<StateInner>);

impl Deref for AppState {
    type Target = StateInner;
    fn deref(&self) -> &StateInner {
        &self.0
    }
}

/// A fully wired MinRegistry server.
pub struct App {
    state: AppState,
    router: Router,
}

impl App {
    /// Opens the database (applying migrations), storage and upload staging.
    pub async fn build(cfg: ServeConfig) -> anyhow::Result<App> {
        crate::install_crypto_provider();
        let db = Db::open(&cfg.core.db_path).await?;
        db.migrate().await?;
        let storage = storage::from_config(&cfg.core.storage).await?;
        let uploads = UploadManager::new(cfg.core.upload_dir.clone()).await?;
        let oauth_http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("minregistry/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        let state = AppState(Arc::new(StateInner {
            audit: AuditLog::new(db.clone()),
            cfg,
            db,
            storage,
            uploads,
            oauth_http,
            token_touches: Mutex::new(HashMap::new()),
            gc_lock: tokio::sync::Mutex::new(()),
            started_at: crate::time::now(),
        }));
        let router = router(state.clone());
        Ok(App { state, router })
    }

    pub fn router(&self) -> Router {
        self.router.clone()
    }

    /// Starts upload expiry, session cleanup, audit retention and scheduled GC.
    pub fn spawn_background_tasks(&self) {
        tasks::spawn_all(&self.state);
    }

    pub async fn serve(
        self,
        listener: TcpListener,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> std::io::Result<()> {
        axum::serve(listener, self.router.into_make_service_with_connect_info::<SocketAddr>())
            .with_graceful_shutdown(shutdown)
            .await
    }
}

fn router(state: AppState) -> Router {
    // Session-authenticated surface: management API + OAuth. Never /v2/.
    let session_layer = auth::session::layer(&state.cfg, SqliteSessionStore::new(state.db.clone()));
    let with_sessions = Router::new()
        .merge(api::router())
        .merge(auth::github::routes())
        .layer(middleware::from_fn(auth::session::require_xhr))
        .layer(session_layer);

    Router::new()
        .merge(registry::router())
        .merge(with_sessions)
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .fallback(ui::serve)
        .layer(middleware::from_fn(trace))
        .layer(CatchPanicLayer::new())
        .with_state(state)
}

/// One span per request with `request_id`, `principal` and `repo` fields.
/// Only the path is logged: never headers or query strings.
async fn trace(req: Request, next: Next) -> Response {
    let request_id = uuid::Uuid::new_v4().simple().to_string();
    let span = tracing::info_span!(
        "request",
        request_id = %request_id,
        method = %req.method(),
        path = %req.uri().path(),
        principal = Empty,
        repo = Empty,
    );
    let started = Instant::now();
    let mut res = next.run(req).instrument(span.clone()).await;
    let status = res.status().as_u16();
    let latency_ms = started.elapsed().as_millis() as u64;
    span.in_scope(|| {
        if status >= 500 {
            tracing::error!(status, latency_ms, "request failed");
        } else {
            tracing::info!(status, latency_ms, "request");
        }
    });
    if let Ok(v) = HeaderValue::from_str(&request_id) {
        res.headers_mut().insert("x-request-id", v);
    }
    res
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz(State(state): State<AppState>) -> Response {
    let db = state.db.ping().await.map_err(|e| e.to_string());
    let probe = Digest::of(b"minregistry readiness probe");
    let storage = state.storage.blob_exists(&probe).await.map(|_| ()).map_err(|e| e.to_string());
    let ok = db.is_ok() && storage.is_ok();
    let body = json!({
        "database": db.err().unwrap_or_else(|| "ok".into()),
        "storage": storage.err().unwrap_or_else(|| "ok".into()),
    });
    let status = if ok { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (status, Json(body)).into_response()
}
