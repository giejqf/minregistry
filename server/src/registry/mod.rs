//! The OCI Distribution API (`/v2/`), spec v1.1.
//!
//! Repository names contain slashes, so every `/v2/` request goes through one
//! dispatcher that parses the path from its end (`names::parse_path`).
//! Authentication happens before anything else; every handler authorizes
//! through `auth::authz::authorize` and audits its outcome.

pub(crate) mod content;
pub(crate) mod error;
pub(crate) mod manifest_parse;
pub(crate) mod names;

mod blobs;
mod catalog;
mod manifests;
mod range;
mod referrers;
mod tags;
mod uploads;

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::any,
    Json, Router,
};
pub(crate) use error::OciCode;
use names::Endpoint;
use serde_json::json;
use tower_http::set_header::SetResponseHeaderLayer;
pub(crate) use uploads::UploadManager;

use crate::{
    app::AppState,
    audit::{action, AuditEvent, ClientInfo, Outcome},
    auth::{
        authz::{self, AuthzError},
        Action, Level, Principal, RegistryPrincipal,
    },
    db::{self, repositories::RepositoryRow},
    error::{AppError, AppResult},
};

pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/v2", any(dispatch)).route("/v2/", any(dispatch)).route("/v2/{*path}", any(dispatch)).layer(
        SetResponseHeaderLayer::overriding(
            HeaderName::from_static("docker-distribution-api-version"),
            HeaderValue::from_static("registry/2.0"),
        ),
    )
}

/// Per-request context shared by the handlers.
pub(crate) struct Ctx {
    pub state: AppState,
    pub principal: Principal,
    pub client: ClientInfo,
    pub headers: HeaderMap,
    pub path: String,
    query: Vec<(String, String)>,
}

impl Ctx {
    pub(crate) fn query(&self, key: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub(crate) fn event(&self, action: &'static str) -> AuditEvent {
        AuditEvent::new(action).principal(&self.principal).client(&self.client)
    }

    pub(crate) async fn record(&self, event: AuditEvent) {
        self.state.audit.record(event).await
    }

    /// Authorizes `action`; denials are audited and become 403.
    async fn authorize(
        &self,
        name: &str,
        action: Action,
        audit_action: &'static str,
    ) -> AppResult<Option<RepositoryRow>> {
        tracing::Span::current().record("repo", name);
        match authz::authorize(&self.state, &self.principal, name, action).await {
            Ok(repo) => Ok(repo),
            Err(AuthzError::Denied) => {
                self.record(
                    self.event(audit_action)
                        .outcome(Outcome::Denied)
                        .repository(name)
                        .detail("required", format!("{action:?}").to_lowercase())
                        .detail("path", self.path.as_str()),
                )
                .await;
                Err(AppError::denied("requested access to the resource is denied"))
            }
            Err(AuthzError::Db(e)) => Err(e.into()),
        }
    }

    /// An existing repository the principal may act on (404 if unknown to an admin).
    pub(crate) async fn existing_repo(
        &self,
        name: &str,
        action: Action,
        audit_action: &'static str,
    ) -> AppResult<RepositoryRow> {
        self.authorize(name, action, audit_action).await?.ok_or_else(|| {
            AppError::not_found(OciCode::NameUnknown, "repository name not known to registry")
                .with_detail(json!({ "name": name }))
        })
    }

    /// The repository to push to, created on first push with the pusher as owner.
    pub(crate) async fn repo_for_push(&self, name: &str, audit_action: &'static str) -> AppResult<RepositoryRow> {
        if let Some(repo) = self.authorize(name, Action::Push, audit_action).await? {
            return Ok(repo);
        }
        let now = crate::time::now();
        let mut tx = self.state.db.begin_write().await?;
        let created = db::repositories::create_or_revive(&mut tx, name, self.principal.id, &now).await?;
        let id = match created {
            db::repositories::Created::New(id) => {
                db::permissions::upsert(
                    &mut *tx,
                    self.principal.id,
                    id,
                    Level::Owner.as_str(),
                    self.principal.id,
                    &now,
                )
                .await?;
                tx.commit().await?;
                self.record(
                    self.event(action::REPOSITORY_CREATE)
                        .repository(name)
                        .detail("owner", self.principal.name.as_str()),
                )
                .await;
                id
            }
            db::repositories::Created::Existing => {
                // Lost a creation race: authorize against the winner's grants.
                tx.rollback().await?;
                return self.existing_repo(name, Action::Push, audit_action).await;
            }
        };
        db::repositories::by_id(&self.state.db.read, id)
            .await?
            .ok_or_else(|| AppError::internal("repository vanished after creation"))
    }
}

#[allow(clippy::too_many_arguments)]
async fn dispatch(
    State(state): State<AppState>,
    auth: Result<RegistryPrincipal, AppError>,
    client: ClientInfo,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let principal = match auth {
        Ok(p) => p.0,
        Err(e) => return e.into_oci_response(),
    };
    tracing::Span::current().record("principal", principal.name.as_str());
    let rest = uri.path().strip_prefix("/v2").unwrap_or_default().trim_start_matches('/');
    let Some(endpoint) = names::parse_path(rest) else {
        return AppError::not_found(OciCode::Unsupported, "unknown registry endpoint").into_oci_response();
    };
    let query =
        uri.query().map(|q| url::form_urlencoded::parse(q.as_bytes()).into_owned().collect()).unwrap_or_default();
    let ctx = Ctx { state, principal, client, headers, path: uri.path().to_string(), query };
    match route(&ctx, &method, endpoint, body).await {
        Ok(res) => res,
        Err(e) => e.into_oci_response(),
    }
}

fn check_name(name: &str) -> AppResult<()> {
    if names::valid_name(name) {
        Ok(())
    } else {
        Err(AppError::bad_request(OciCode::NameInvalid, "invalid repository name").with_detail(json!({ "name": name })))
    }
}

fn method_not_allowed() -> AppError {
    AppError::new(StatusCode::METHOD_NOT_ALLOWED, OciCode::Unsupported, "method not allowed for this endpoint")
}

async fn route(ctx: &Ctx, method: &Method, endpoint: Endpoint, body: Body) -> AppResult<Response> {
    use Endpoint::*;
    let m = method.clone();
    match endpoint {
        Base => match m {
            Method::GET | Method::HEAD => base(ctx, &m).await,
            _ => Err(method_not_allowed()),
        },
        Catalog => match m {
            Method::GET => catalog::list(ctx).await,
            _ => Err(method_not_allowed()),
        },
        Tags { name } => {
            check_name(&name)?;
            match m {
                Method::GET => tags::list(ctx, &name).await,
                _ => Err(method_not_allowed()),
            }
        }
        Manifest { name, reference } => {
            check_name(&name)?;
            match m {
                Method::GET | Method::HEAD => manifests::get(ctx, &m, &name, &reference).await,
                Method::PUT => manifests::put(ctx, &name, &reference, body).await,
                Method::DELETE => manifests::delete(ctx, &name, &reference).await,
                _ => Err(method_not_allowed()),
            }
        }
        Blob { name, digest } => {
            check_name(&name)?;
            match m {
                Method::GET | Method::HEAD => blobs::get(ctx, &m, &name, &digest).await,
                Method::DELETE => blobs::delete(ctx, &name, &digest).await,
                _ => Err(method_not_allowed()),
            }
        }
        UploadStart { name } => {
            check_name(&name)?;
            match m {
                Method::POST => uploads::start(ctx, &name, body).await,
                _ => Err(method_not_allowed()),
            }
        }
        Upload { name, uuid } => {
            check_name(&name)?;
            match m {
                Method::GET => uploads::status(ctx, &name, &uuid).await,
                Method::PATCH => uploads::patch(ctx, &name, &uuid, body).await,
                Method::PUT => uploads::put(ctx, &name, &uuid, body).await,
                Method::DELETE => uploads::cancel(ctx, &name, &uuid).await,
                _ => Err(method_not_allowed()),
            }
        }
        Referrers { name, digest } => {
            check_name(&name)?;
            match m {
                Method::GET => referrers::list(ctx, &name, &digest).await,
                _ => Err(method_not_allowed()),
            }
        }
    }
}

/// `GET /v2/`: credentials are valid (clients use this for `login`).
async fn base(ctx: &Ctx, method: &Method) -> AppResult<Response> {
    if *method == Method::GET {
        ctx.record(ctx.event(action::LOGIN)).await;
    }
    let mut res = Json(json!({})).into_response();
    if *method == Method::HEAD {
        res = StatusCode::OK.into_response();
        res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    }
    Ok(res)
}
