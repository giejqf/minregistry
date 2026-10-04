//! The management API (`/api/v1/`): JSON, admin session only, documented
//! with utoipa. `minregistry openapi` prints the same document that is served
//! at `/api/v1/openapi.json` and committed as `web/openapi.json`.

mod audit;
mod dto;
mod principals;
mod repositories;
mod session;
mod system;

use axum::{routing::get, Json, Router};
use utoipa::{
    openapi::security::{ApiKey, ApiKeyValue, SecurityScheme},
    Modify, OpenApi,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    app::AppState,
    audit::{AuditEvent, ClientInfo},
    auth::AdminSession,
    error::{AppError, AppResult},
    registry::OciCode,
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "MinRegistry management API",
        description = "Management API of MinRegistry. All endpoints except this document require an \
                       admin session (GitHub OAuth); mutating requests must send \
                       `X-Requested-With: XMLHttpRequest`.",
        license(name = "Apache-2.0", identifier = "Apache-2.0"),
    ),
    modifiers(&SessionCookie),
    tags(
        (name = "session", description = "The signed-in admin"),
        (name = "repositories", description = "Repositories, tags and manifests"),
        (name = "permissions", description = "Per-repository access grants"),
        (name = "principals", description = "GitHub admins, identities and their tokens"),
        (name = "audit", description = "Audit log"),
        (name = "system", description = "Storage, uploads and garbage collection"),
    ),
    components(schemas(dto::ErrorResponse, dto::ErrorDetail))
)]
struct ApiDoc;

struct SessionCookie;

impl Modify for SessionCookie {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "session",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new(crate::auth::session::COOKIE_NAME))),
        );
    }
}

fn api_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(session::get_me))
        .routes(routes!(repositories::list_repositories))
        .routes(routes!(repositories::get_repository, repositories::delete_repository))
        .routes(routes!(repositories::delete_repository_tag))
        .routes(routes!(repositories::delete_repository_manifest))
        .routes(routes!(repositories::list_repository_permissions))
        .routes(routes!(repositories::grant_permission, repositories::revoke_permission))
        .routes(routes!(principals::list_principals, principals::create_identity))
        .routes(routes!(principals::get_principal, principals::update_principal))
        .routes(routes!(principals::list_tokens, principals::create_token))
        .routes(routes!(principals::revoke_token))
        .routes(routes!(audit::list_audit_events))
        .routes(routes!(audit::list_audit_actions))
        .routes(routes!(system::get_system))
        .routes(routes!(system::list_uploads))
        .routes(routes!(system::run_gc))
}

fn split() -> (Router<AppState>, utoipa::openapi::OpenApi) {
    OpenApiRouter::with_openapi(ApiDoc::openapi()).nest("/api/v1", api_routes()).split_for_parts()
}

/// The OpenAPI document of `/api/v1/`.
pub fn openapi() -> utoipa::openapi::OpenApi {
    split().1
}

/// `minregistry openapi` output: pretty JSON plus a trailing newline.
pub fn openapi_json() -> String {
    let mut json = openapi().to_pretty_json().unwrap_or_default();
    json.push('\n');
    json
}

pub(crate) fn router() -> Router<AppState> {
    let (router, doc) = split();
    router.route("/api/v1/openapi.json", get(move || async move { Json(doc) }))
}

/// Ids are integers in the database and strings in the API.
pub(crate) fn parse_id(raw: &str, what: &str) -> AppResult<i64> {
    raw.parse::<i64>().map_err(|_| AppError::not_found(OciCode::NameUnknown, format!("{what} not found")))
}

pub(crate) fn validation(message: impl Into<String>) -> AppError {
    AppError::bad_request(OciCode::Unsupported, message).with_api_code("validation")
}

/// An audit event attributed to the signed-in admin.
pub(crate) fn admin_event(admin: &AdminSession, client: &ClientInfo, action: &'static str) -> AuditEvent {
    AuditEvent::new(action).principal(&admin.principal).client(client).detail("via", "api")
}

#[cfg(test)]
mod tests {
    #[test]
    fn document_is_complete() {
        let doc = super::openapi();
        let paths: Vec<&String> = doc.paths.paths.keys().collect();
        for p in
            ["/api/v1/me", "/api/v1/repositories/{id}", "/api/v1/principals/{id}/tokens", "/api/v1/audit", "/api/v1/gc"]
        {
            assert!(paths.iter().any(|k| *k == p), "{p} missing from {paths:?}");
        }
        assert!(super::openapi_json().ends_with("}\n"));
        assert!(paths.iter().all(|p| !p.starts_with("/v2")), "/v2/ is defined by the OCI spec, not the API doc");
    }
}
