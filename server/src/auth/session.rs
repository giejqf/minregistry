//! Admin web sessions: `tower-sessions` with a SQLite store and a private
//! (signed + encrypted) cookie, plus the CSRF rule for mutating API calls.

use std::fmt;

use async_trait::async_trait;
use axum::{
    extract::{FromRequestParts, Request},
    http::{request::Parts, Method},
    middleware::Next,
    response::Response,
};
use tower_sessions::{
    cookie::{Key, SameSite},
    session::{Id, Record},
    session_store, Expiry, Session, SessionManagerLayer, SessionStore,
};

use super::{Principal, PrincipalKind};
use crate::{
    app::AppState,
    config::Config,
    db::{self, Db},
    error::AppError,
    registry::error::OciCode,
};

pub(crate) const COOKIE_NAME: &str = "minregistry_session";
pub(crate) const PRINCIPAL_KEY: &str = "principal_id";
pub(crate) const OAUTH_STATE_KEY: &str = "oauth_state";
pub(crate) const OAUTH_NEXT_KEY: &str = "oauth_next";
const IDLE_TIMEOUT: time::Duration = time::Duration::hours(12);

#[derive(Clone)]
pub(crate) struct SqliteSessionStore {
    db: Db,
}

impl fmt::Debug for SqliteSessionStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SqliteSessionStore")
    }
}

impl SqliteSessionStore {
    pub(crate) fn new(db: Db) -> Self {
        SqliteSessionStore { db }
    }

    pub(crate) async fn delete_expired(&self) -> sqlx::Result<u64> {
        db::sessions::delete_expired(&self.db.write, time::OffsetDateTime::now_utc().unix_timestamp()).await
    }
}

fn backend(e: impl fmt::Display) -> session_store::Error {
    session_store::Error::Backend(e.to_string())
}

#[async_trait]
impl SessionStore for SqliteSessionStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        let data = serde_json::to_string(&record.data).map_err(|e| session_store::Error::Encode(e.to_string()))?;
        loop {
            match db::sessions::insert(
                &self.db.write,
                &record.id.to_string(),
                &data,
                record.expiry_date.unix_timestamp(),
            )
            .await
            {
                Ok(()) => return Ok(()),
                Err(e) if db::is_unique_violation(&e) => record.id = Id::default(),
                Err(e) => return Err(backend(e)),
            }
        }
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let data = serde_json::to_string(&record.data).map_err(|e| session_store::Error::Encode(e.to_string()))?;
        db::sessions::upsert(&self.db.write, &record.id.to_string(), &data, record.expiry_date.unix_timestamp())
            .await
            .map_err(backend)
    }

    async fn load(&self, id: &Id) -> session_store::Result<Option<Record>> {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let Some(row) = db::sessions::load(&self.db.read, &id.to_string(), now).await.map_err(backend)? else {
            return Ok(None);
        };
        let data = serde_json::from_str(&row.data).map_err(|e| session_store::Error::Decode(e.to_string()))?;
        let expiry_date = time::OffsetDateTime::from_unix_timestamp(row.expiry_date).map_err(backend)?;
        Ok(Some(Record { id: *id, data, expiry_date }))
    }

    async fn delete(&self, id: &Id) -> session_store::Result<()> {
        db::sessions::delete(&self.db.write, &id.to_string()).await.map_err(backend)
    }
}

pub(crate) fn layer(
    cfg: &Config,
    store: SqliteSessionStore,
) -> SessionManagerLayer<SqliteSessionStore, tower_sessions::service::PrivateCookie> {
    SessionManagerLayer::new(store)
        .with_name(COOKIE_NAME)
        .with_path("/")
        .with_http_only(true)
        .with_same_site(SameSite::Lax)
        .with_secure(cfg.secure_cookies())
        .with_expiry(Expiry::OnInactivity(IDLE_TIMEOUT))
        .with_private(Key::derive_from(&cfg.session_secret))
}

/// A signed-in admin. Re-validated on every request against the allowlist.
pub(crate) struct AdminSession {
    pub principal: Principal,
}

impl FromRequestParts<AppState> for AdminSession {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state)
            .await
            .map_err(|(_, msg)| AppError::internal(format_args!("session layer: {msg}")))?;
        let Some(id) = session.get::<i64>(PRINCIPAL_KEY).await? else {
            return Err(AppError::unauthorized().with_api_code("unauthenticated"));
        };
        let principal = db::principals::by_id(&state.db.read, id)
            .await?
            .and_then(|row| Principal::from_row(&row, &state.cfg))
            .filter(|p| p.kind == PrincipalKind::Github && p.is_admin);
        match principal {
            Some(principal) => Ok(AdminSession { principal }),
            None => {
                session.flush().await?;
                Err(AppError::unauthorized().with_api_code("unauthenticated"))
            }
        }
    }
}

/// Mutating `/api/v1/` requests must carry `X-Requested-With: XMLHttpRequest`
/// (the generated SDK sets it). Browsers do not allow cross-site requests to
/// set that header without a CORS preflight, which we never grant.
pub(crate) async fn require_xhr(req: Request, next: Next) -> Response {
    let safe = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    let xhr = req
        .headers()
        .get("x-requested-with")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("XMLHttpRequest"));
    if safe || xhr {
        next.run(req).await
    } else {
        use axum::response::IntoResponse;
        AppError::new(
            axum::http::StatusCode::FORBIDDEN,
            OciCode::Denied,
            "mutating requests must send X-Requested-With: XMLHttpRequest",
        )
        .with_api_code("csrf")
        .into_response()
    }
}
