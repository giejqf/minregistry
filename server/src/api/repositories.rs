//! Repositories, their tags/manifests, and per-repository permissions.

use std::collections::{BTreeMap, HashMap};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};

use super::{
    admin_event,
    dto::*,
    parse_id,
    principals::{kind, level_dto},
};
use crate::{
    app::AppState,
    audit::{action, ClientInfo},
    auth::AdminSession,
    db::{self, repositories::RepositoryRow},
    digest::Digest,
    error::{AppError, AppResult},
    registry::{names::valid_tag, OciCode},
};

async fn load(state: &AppState, raw_id: &str) -> AppResult<RepositoryRow> {
    let id = parse_id(raw_id, "repository")?;
    db::repositories::by_id(&state.db.read, id)
        .await?
        .ok_or_else(|| AppError::not_found(OciCode::NameUnknown, "repository not found"))
}

/// Repositories, alphabetically, with a substring filter.
#[utoipa::path(
    get,
    path = "/repositories",
    tag = "repositories",
    params(RepositoryListQuery),
    responses((status = 200, body = RepositoryListResponse), (status = 401, body = ErrorResponse)),
    security(("session" = []))
)]
pub(crate) async fn list_repositories(
    State(state): State<AppState>,
    _admin: AdminSession,
    Query(q): Query<RepositoryListQuery>,
) -> AppResult<Json<RepositoryListResponse>> {
    let filter = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let items = db::repositories::list(&state.db.read, filter, limit, offset)
        .await?
        .into_iter()
        .map(|r| RepositorySummary {
            id: r.id.to_string(),
            name: r.name,
            created_at: r.created_at,
            created_by: r.created_by,
            tag_count: r.tag_count,
            manifest_count: r.manifest_count,
            last_push_at: r.last_push_at,
        })
        .collect();
    let total = db::repositories::count(&state.db.read, filter).await?;
    Ok(Json(RepositoryListResponse { items, total }))
}

/// A repository with its tags and manifests (including referrers).
#[utoipa::path(
    get,
    path = "/repositories/{id}",
    tag = "repositories",
    params(("id" = String, Path, description = "Repository id")),
    responses((status = 200, body = RepositoryDetail), (status = 404, body = ErrorResponse)),
    security(("session" = []))
)]
pub(crate) async fn get_repository(
    State(state): State<AppState>,
    _admin: AdminSession,
    Path(id): Path<String>,
) -> AppResult<Json<RepositoryDetail>> {
    let repo = load(&state, &id).await?;
    let tag_rows = db::tags::list(&state.db.read, repo.id).await?;
    let mut tags_by_digest: HashMap<String, Vec<String>> = HashMap::new();
    for t in &tag_rows {
        tags_by_digest.entry(t.manifest_digest.clone()).or_default().push(t.name.clone());
    }
    let manifests = db::manifests::list(&state.db.read, repo.id)
        .await?
        .into_iter()
        .map(|m| {
            let platforms: Vec<PlatformSummary> =
                m.platforms.as_deref().and_then(|p| serde_json::from_str(p).ok()).unwrap_or_default();
            let annotations: BTreeMap<String, String> = m
                .annotations
                .as_deref()
                .and_then(|a| serde_json::from_str::<BTreeMap<String, serde_json::Value>>(a).ok())
                .unwrap_or_default()
                .into_iter()
                .map(|(k, v)| (k, v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())))
                .collect();
            ManifestSummary {
                tags: tags_by_digest.remove(&m.digest).unwrap_or_default(),
                digest: m.digest,
                media_type: m.media_type,
                size: m.size,
                artifact_type: m.artifact_type,
                subject_digest: m.subject_digest,
                platforms,
                annotations,
                created_at: m.created_at,
                pushed_by: m.pushed_by,
            }
        })
        .collect();
    let created_by = match repo.created_by {
        Some(pid) => db::principals::by_id(&state.db.read, pid).await?.map(|p| p.name),
        None => None,
    };
    Ok(Json(RepositoryDetail {
        id: repo.id.to_string(),
        name: repo.name,
        created_at: repo.created_at,
        created_by,
        tags: tag_rows
            .into_iter()
            .map(|t| TagSummary {
                name: t.name,
                digest: t.manifest_digest,
                updated_at: t.updated_at,
                updated_by: t.updated_by,
            })
            .collect(),
        manifests,
    }))
}

/// Deletes a repository: tags, manifests, grants and uploads go; blobs are
/// reclaimed by the next garbage collection.
#[utoipa::path(
    delete,
    path = "/repositories/{id}",
    tag = "repositories",
    params(("id" = String, Path, description = "Repository id")),
    responses((status = 204, description = "Deleted"), (status = 404, body = ErrorResponse)),
    security(("session" = []))
)]
pub(crate) async fn delete_repository(
    State(state): State<AppState>,
    admin: AdminSession,
    client: ClientInfo,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    let repo = load(&state, &id).await?;
    let mut tx = state.db.begin_write().await?;
    let uploads = db::repositories::soft_delete(&mut tx, repo.id, &crate::time::now()).await?;
    tx.commit().await?;
    for uuid in uploads {
        state.uploads.discard(&uuid).await;
    }
    state.audit.record(admin_event(&admin, &client, action::REPOSITORY_DELETE).repository(repo.name)).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Removes a tag (the manifest stays).
#[utoipa::path(
    delete,
    path = "/repositories/{id}/tags/{tag}",
    tag = "repositories",
    params(("id" = String, Path, description = "Repository id"), ("tag" = String, Path, description = "Tag name")),
    responses((status = 204, description = "Deleted"), (status = 404, body = ErrorResponse)),
    security(("session" = []))
)]
pub(crate) async fn delete_repository_tag(
    State(state): State<AppState>,
    admin: AdminSession,
    client: ClientInfo,
    Path((id, tag)): Path<(String, String)>,
) -> AppResult<StatusCode> {
    let repo = load(&state, &id).await?;
    let unknown = || AppError::not_found(OciCode::ManifestUnknown, "tag not found");
    if !valid_tag(&tag) {
        return Err(unknown());
    }
    let digest = db::tags::resolve(&state.db.read, repo.id, &tag).await?;
    if !db::tags::delete(&state.db.write, repo.id, &tag).await? {
        return Err(unknown());
    }
    let mut event = admin_event(&admin, &client, action::TAG_DELETE).repository(repo.name).reference(tag);
    if let Some(d) = digest {
        event = event.digest(d);
    }
    state.audit.record(event).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Deletes a manifest and every tag pointing at it.
#[utoipa::path(
    delete,
    path = "/repositories/{id}/manifests/{digest}",
    tag = "repositories",
    params(("id" = String, Path, description = "Repository id"), ("digest" = String, Path, description = "Manifest digest")),
    responses((status = 204, description = "Deleted"), (status = 404, body = ErrorResponse)),
    security(("session" = []))
)]
pub(crate) async fn delete_repository_manifest(
    State(state): State<AppState>,
    admin: AdminSession,
    client: ClientInfo,
    Path((id, digest)): Path<(String, String)>,
) -> AppResult<StatusCode> {
    let repo = load(&state, &id).await?;
    let unknown = || AppError::not_found(OciCode::ManifestUnknown, "manifest not found");
    let digest = Digest::parse(&digest).map_err(|_| unknown())?;
    if !db::manifests::delete(&state.db.write, repo.id, digest.as_str()).await? {
        return Err(unknown());
    }
    state
        .audit
        .record(
            admin_event(&admin, &client, action::MANIFEST_DELETE)
                .repository(repo.name)
                .reference(digest.as_str())
                .digest(&digest),
        )
        .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Who may access a repository.
#[utoipa::path(
    get,
    path = "/repositories/{id}/permissions",
    tag = "permissions",
    params(("id" = String, Path, description = "Repository id")),
    responses((status = 200, body = Vec<RepositoryPermission>), (status = 404, body = ErrorResponse)),
    security(("session" = []))
)]
pub(crate) async fn list_repository_permissions(
    State(state): State<AppState>,
    _admin: AdminSession,
    Path(id): Path<String>,
) -> AppResult<Json<Vec<RepositoryPermission>>> {
    let repo = load(&state, &id).await?;
    Ok(Json(
        db::permissions::for_repository(&state.db.read, repo.id)
            .await?
            .into_iter()
            .map(|p| RepositoryPermission {
                principal: PrincipalRef {
                    id: p.principal_id.to_string(),
                    kind: if p.principal_kind == db::principals::KIND_GITHUB {
                        PrincipalKind::Github
                    } else {
                        PrincipalKind::Identity
                    },
                    name: p.principal_name,
                },
                level: level_dto(&p.level),
                granted_by: p.granted_by,
                granted_at: p.granted_at,
            })
            .collect(),
    ))
}

/// Grants (or changes) a principal's level on a repository.
#[utoipa::path(
    put,
    path = "/repositories/{id}/permissions/{principal_id}",
    tag = "permissions",
    params(("id" = String, Path, description = "Repository id"), ("principal_id" = String, Path, description = "Principal id")),
    request_body = GrantPermissionRequest,
    responses((status = 200, body = RepositoryPermission), (status = 404, body = ErrorResponse)),
    security(("session" = []))
)]
pub(crate) async fn grant_permission(
    State(state): State<AppState>,
    admin: AdminSession,
    client: ClientInfo,
    Path((id, principal_id)): Path<(String, String)>,
    Json(req): Json<GrantPermissionRequest>,
) -> AppResult<Json<RepositoryPermission>> {
    let repo = load(&state, &id).await?;
    let principal = db::principals::by_id(&state.db.read, parse_id(&principal_id, "principal")?)
        .await?
        .ok_or_else(|| AppError::not_found(OciCode::NameUnknown, "principal not found"))?;
    let level = match req.level {
        PermissionLevel::Read => "read",
        PermissionLevel::Write => "write",
        PermissionLevel::Owner => "owner",
    };
    let now = crate::time::now();
    db::permissions::upsert(&state.db.write, principal.id, repo.id, level, admin.principal.id, &now).await?;
    state
        .audit
        .record(
            admin_event(&admin, &client, action::PERMISSION_GRANT)
                .repository(repo.name)
                .detail("principal", principal.name.as_str())
                .detail("level", level),
        )
        .await;
    Ok(Json(RepositoryPermission {
        principal: PrincipalRef { id: principal.id.to_string(), kind: kind(&principal), name: principal.name },
        level: req.level,
        granted_by: Some(admin.principal.name.clone()),
        granted_at: now,
    }))
}

/// Removes a principal's grant on a repository.
#[utoipa::path(
    delete,
    path = "/repositories/{id}/permissions/{principal_id}",
    tag = "permissions",
    params(("id" = String, Path, description = "Repository id"), ("principal_id" = String, Path, description = "Principal id")),
    responses((status = 204, description = "Revoked"), (status = 404, body = ErrorResponse)),
    security(("session" = []))
)]
pub(crate) async fn revoke_permission(
    State(state): State<AppState>,
    admin: AdminSession,
    client: ClientInfo,
    Path((id, principal_id)): Path<(String, String)>,
) -> AppResult<StatusCode> {
    let repo = load(&state, &id).await?;
    let principal = db::principals::by_id(&state.db.read, parse_id(&principal_id, "principal")?)
        .await?
        .ok_or_else(|| AppError::not_found(OciCode::NameUnknown, "principal not found"))?;
    if !db::permissions::delete(&state.db.write, principal.id, repo.id).await? {
        return Err(AppError::not_found(OciCode::NameUnknown, "no such grant"));
    }
    state
        .audit
        .record(
            admin_event(&admin, &client, action::PERMISSION_REVOKE)
                .repository(repo.name)
                .detail("principal", principal.name.as_str()),
        )
        .await;
    Ok(StatusCode::NO_CONTENT)
}
