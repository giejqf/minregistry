//! Principals (GitHub admins and token-only identities) and their tokens.

use std::{collections::HashMap, sync::LazyLock};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use regex::Regex;

use super::{admin_event, dto::*, parse_id, validation};
use crate::{
    app::AppState,
    audit::{action, ClientInfo},
    auth::{tokens, AdminSession, Level, Principal},
    db::{
        self,
        principals::{PrincipalRow, KIND_GITHUB, KIND_IDENTITY},
        tokens::TokenRow,
    },
    error::{AppError, AppResult},
    registry::OciCode,
};

static IDENTITY_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9]([a-z0-9._-]{0,62}[a-z0-9])?$").expect("valid regex"));

pub(crate) fn kind(row: &PrincipalRow) -> PrincipalKind {
    if row.kind == KIND_GITHUB {
        PrincipalKind::Github
    } else {
        PrincipalKind::Identity
    }
}

pub(crate) fn summary(state: &AppState, row: &PrincipalRow, active_tokens: i64) -> PrincipalSummary {
    let is_admin = row.kind == KIND_GITHUB && state.cfg.is_admin_login(&row.name);
    PrincipalSummary {
        id: row.id.to_string(),
        kind: kind(row),
        name: row.name.clone(),
        display_name: row.display_name.clone(),
        enabled: row.enabled,
        is_admin,
        active: Principal::from_row(row, &state.cfg).is_some(),
        active_token_count: active_tokens,
        created_at: row.created_at.clone(),
    }
}

pub(crate) fn token_summary(row: TokenRow, now: &str) -> TokenSummary {
    let status = if row.revoked_at.is_some() {
        TokenStatus::Revoked
    } else if row.expires_at.as_deref().is_some_and(|e| e <= now) {
        TokenStatus::Expired
    } else {
        TokenStatus::Active
    };
    TokenSummary {
        id: row.id.to_string(),
        name: row.name,
        prefix: row.prefix,
        status,
        created_at: row.created_at,
        created_by: row.created_by,
        expires_at: row.expires_at,
        revoked_at: row.revoked_at,
        last_used_at: row.last_used_at,
    }
}

pub(crate) fn level_dto(level: &str) -> PermissionLevel {
    match Level::parse(level) {
        Some(Level::Owner) => PermissionLevel::Owner,
        Some(Level::Write) => PermissionLevel::Write,
        _ => PermissionLevel::Read,
    }
}

async fn load(state: &AppState, raw_id: &str) -> AppResult<PrincipalRow> {
    let id = parse_id(raw_id, "principal")?;
    db::principals::by_id(&state.db.read, id)
        .await?
        .ok_or_else(|| AppError::not_found(OciCode::NameUnknown, "principal not found"))
}

/// GitHub admins (from configuration) and all principals.
#[utoipa::path(
    get,
    path = "/principals",
    tag = "principals",
    responses((status = 200, body = PrincipalListResponse), (status = 401, body = ErrorResponse)),
    security(("session" = []))
)]
pub(crate) async fn list_principals(
    State(state): State<AppState>,
    _admin: AdminSession,
) -> AppResult<Json<PrincipalListResponse>> {
    let now = crate::time::now();
    let counts: HashMap<i64, i64> = db::tokens::active_counts(&state.db.read, &now).await?.into_iter().collect();
    let principals = db::principals::list(&state.db.read)
        .await?
        .iter()
        .map(|row| summary(&state, row, counts.get(&row.id).copied().unwrap_or(0)))
        .collect();
    Ok(Json(PrincipalListResponse { admin_logins: state.cfg.admin_logins.clone(), principals }))
}

/// Creates a token-only identity (no web login).
#[utoipa::path(
    post,
    path = "/principals",
    tag = "principals",
    request_body = CreateIdentityRequest,
    responses(
        (status = 201, body = PrincipalSummary),
        (status = 400, body = ErrorResponse),
        (status = 409, description = "Name already taken", body = ErrorResponse),
    ),
    security(("session" = []))
)]
pub(crate) async fn create_identity(
    State(state): State<AppState>,
    admin: AdminSession,
    client: ClientInfo,
    Json(req): Json<CreateIdentityRequest>,
) -> AppResult<(StatusCode, Json<PrincipalSummary>)> {
    let name = req.name.trim().to_string();
    if !IDENTITY_NAME.is_match(&name) {
        return Err(validation(
            "name must be 1-64 lower-case letters, digits, '.', '_' or '-', starting and ending with a letter or digit",
        ));
    }
    if state.cfg.is_admin_login(&name) {
        return Err(AppError::conflict("this name is reserved for a GitHub admin"));
    }
    let display_name = validate_display_name(req.display_name.as_deref())?.unwrap_or_else(|| name.clone());
    let id =
        match db::principals::insert(&state.db.write, KIND_IDENTITY, &name, None, &display_name, &crate::time::now())
            .await
        {
            Ok(id) => id,
            Err(e) if db::is_unique_violation(&e) => {
                return Err(AppError::conflict("a principal with this name already exists"))
            }
            Err(e) => return Err(e.into()),
        };
    let row = load(&state, &id.to_string()).await?;
    state.audit.record(admin_event(&admin, &client, action::PRINCIPAL_CREATE).detail("principal", name.as_str())).await;
    Ok((StatusCode::CREATED, Json(summary(&state, &row, 0))))
}

fn validate_display_name(v: Option<&str>) -> AppResult<Option<String>> {
    match v.map(str::trim) {
        None | Some("") => Ok(None),
        Some(v) if v.chars().count() > 100 => Err(validation("display_name must be at most 100 characters")),
        Some(v) => Ok(Some(v.to_string())),
    }
}

/// A principal with its tokens and repository grants.
#[utoipa::path(
    get,
    path = "/principals/{id}",
    tag = "principals",
    params(("id" = String, Path, description = "Principal id")),
    responses((status = 200, body = PrincipalDetail), (status = 404, body = ErrorResponse)),
    security(("session" = []))
)]
pub(crate) async fn get_principal(
    State(state): State<AppState>,
    _admin: AdminSession,
    Path(id): Path<String>,
) -> AppResult<Json<PrincipalDetail>> {
    let row = load(&state, &id).await?;
    let now = crate::time::now();
    let tokens: Vec<TokenSummary> =
        db::tokens::list_for(&state.db.read, row.id).await?.into_iter().map(|t| token_summary(t, &now)).collect();
    let active = tokens.iter().filter(|t| t.status == TokenStatus::Active).count() as i64;
    let permissions = db::permissions::for_principal(&state.db.read, row.id)
        .await?
        .into_iter()
        .map(|p| PrincipalPermission {
            repository: RepositoryRef { id: p.repository_id.to_string(), name: p.repository_name },
            level: level_dto(&p.level),
            granted_by: p.granted_by,
            granted_at: p.granted_at,
        })
        .collect();
    Ok(Json(PrincipalDetail { principal: summary(&state, &row, active), tokens, permissions }))
}

/// Enables/disables an identity or renames its display name.
#[utoipa::path(
    patch,
    path = "/principals/{id}",
    tag = "principals",
    params(("id" = String, Path, description = "Principal id")),
    request_body = UpdatePrincipalRequest,
    responses(
        (status = 200, body = PrincipalSummary),
        (status = 400, description = "GitHub admins are managed through configuration", body = ErrorResponse),
        (status = 404, body = ErrorResponse),
    ),
    security(("session" = []))
)]
pub(crate) async fn update_principal(
    State(state): State<AppState>,
    admin: AdminSession,
    client: ClientInfo,
    Path(id): Path<String>,
    Json(req): Json<UpdatePrincipalRequest>,
) -> AppResult<Json<PrincipalSummary>> {
    let row = load(&state, &id).await?;
    if row.kind == KIND_GITHUB {
        return Err(validation("GitHub admins are managed through MINREGISTRY_ADMIN_GITHUB_LOGINS"));
    }
    let display_name = validate_display_name(req.display_name.as_deref())?;
    db::principals::update(&state.db.write, row.id, req.enabled, display_name.as_deref()).await?;
    let mut event = admin_event(&admin, &client, action::PRINCIPAL_UPDATE).detail("principal", row.name.as_str());
    if let Some(enabled) = req.enabled {
        event = event.detail("enabled", enabled);
    }
    if let Some(d) = &display_name {
        event = event.detail("display_name", d.as_str());
    }
    state.audit.record(event).await;
    let row = load(&state, &id).await?;
    let now = crate::time::now();
    let active = db::tokens::active_for(&state.db.read, row.id, &now).await?.len() as i64;
    Ok(Json(summary(&state, &row, active)))
}

/// Tokens of a principal (secrets are never returned).
#[utoipa::path(
    get,
    path = "/principals/{id}/tokens",
    tag = "principals",
    params(("id" = String, Path, description = "Principal id")),
    responses((status = 200, body = Vec<TokenSummary>), (status = 404, body = ErrorResponse)),
    security(("session" = []))
)]
pub(crate) async fn list_tokens(
    State(state): State<AppState>,
    _admin: AdminSession,
    Path(id): Path<String>,
) -> AppResult<Json<Vec<TokenSummary>>> {
    let row = load(&state, &id).await?;
    let now = crate::time::now();
    Ok(Json(db::tokens::list_for(&state.db.read, row.id).await?.into_iter().map(|t| token_summary(t, &now)).collect()))
}

/// Issues a token. The secret is returned once. Admins may mint tokens for
/// identities and for their own GitHub principal only.
#[utoipa::path(
    post,
    path = "/principals/{id}/tokens",
    tag = "principals",
    params(("id" = String, Path, description = "Principal id")),
    request_body = CreateTokenRequest,
    responses(
        (status = 201, body = CreateTokenResponse),
        (status = 400, body = ErrorResponse),
        (status = 403, description = "Another admin's GitHub principal", body = ErrorResponse),
        (status = 404, body = ErrorResponse),
    ),
    security(("session" = []))
)]
pub(crate) async fn create_token(
    State(state): State<AppState>,
    admin: AdminSession,
    client: ClientInfo,
    Path(id): Path<String>,
    Json(req): Json<CreateTokenRequest>,
) -> AppResult<(StatusCode, Json<CreateTokenResponse>)> {
    let row = load(&state, &id).await?;
    if row.kind == KIND_GITHUB && row.id != admin.principal.id {
        return Err(AppError::denied("tokens for a GitHub admin can only be created by that admin"));
    }
    let name = req.name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(validation("token name must be 1-64 characters"));
    }
    let now = crate::time::now();
    let expires_at = match req.expires_at.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => None,
        Some(raw) => {
            let ts =
                crate::time::normalize(raw).ok_or_else(|| validation("expires_at must be an RFC 3339 timestamp"))?;
            if ts <= now {
                return Err(validation("expires_at must be in the future"));
            }
            Some(ts)
        }
    };
    let token = tokens::generate()?;
    let token_id = db::tokens::insert(
        &state.db.write,
        row.id,
        name,
        &token.hash,
        &token.prefix,
        admin.principal.id,
        &now,
        expires_at.as_deref(),
    )
    .await?;
    let stored = db::tokens::get(&state.db.read, row.id, token_id)
        .await?
        .ok_or_else(|| AppError::internal("token vanished after creation"))?;
    let mut event = admin_event(&admin, &client, action::TOKEN_CREATE)
        .detail("principal", row.name.as_str())
        .detail("token_id", token_id)
        .detail("token_name", name)
        .detail("prefix", token.prefix.as_str());
    if let Some(e) = &expires_at {
        event = event.detail("expires_at", e.as_str());
    }
    state.audit.record(event).await;
    Ok((
        StatusCode::CREATED,
        Json(CreateTokenResponse { token: token_summary(stored, &now), secret: token.secret, username: row.name }),
    ))
}

/// Revokes a token immediately.
#[utoipa::path(
    delete,
    path = "/principals/{id}/tokens/{token_id}",
    tag = "principals",
    params(("id" = String, Path, description = "Principal id"), ("token_id" = String, Path, description = "Token id")),
    responses((status = 204, description = "Revoked"), (status = 404, body = ErrorResponse)),
    security(("session" = []))
)]
pub(crate) async fn revoke_token(
    State(state): State<AppState>,
    admin: AdminSession,
    client: ClientInfo,
    Path((id, token_id)): Path<(String, String)>,
) -> AppResult<StatusCode> {
    let row = load(&state, &id).await?;
    let token_id = parse_id(&token_id, "token")?;
    if !db::tokens::revoke(&state.db.write, row.id, token_id, &crate::time::now()).await? {
        return Err(AppError::not_found(OciCode::NameUnknown, "token not found or already revoked"));
    }
    state
        .audit
        .record(
            admin_event(&admin, &client, action::TOKEN_REVOKE)
                .detail("principal", row.name.as_str())
                .detail("token_id", token_id),
        )
        .await;
    Ok(StatusCode::NO_CONTENT)
}
