//! Management API DTOs. Naming: `*Request` / `*Response` / `*Summary` /
//! `*Detail`. Timestamps are RFC 3339 strings; ids are strings.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Serialize, ToSchema)]
pub(crate) struct ErrorResponse {
    pub error: ErrorDetail,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct ErrorDetail {
    /// Machine-readable code, e.g. `not_found`, `unauthenticated`, `csrf`.
    pub code: String,
    pub message: String,
}

// --- Principals --------------------------------------------------------------

#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PrincipalKind {
    Github,
    Identity,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct PrincipalSummary {
    pub id: String,
    pub kind: PrincipalKind,
    pub name: String,
    pub display_name: String,
    pub enabled: bool,
    /// GitHub principal currently listed in `MINREGISTRY_ADMIN_GITHUB_LOGINS`.
    pub is_admin: bool,
    /// Whether the principal can authenticate right now.
    pub active: bool,
    pub active_token_count: i64,
    pub created_at: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct PrincipalRef {
    pub id: String,
    pub kind: PrincipalKind,
    pub name: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct PrincipalListResponse {
    /// GitHub logins allowed to sign in (from configuration, read-only).
    pub admin_logins: Vec<String>,
    pub principals: Vec<PrincipalSummary>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct PrincipalDetail {
    pub principal: PrincipalSummary,
    pub tokens: Vec<TokenSummary>,
    pub permissions: Vec<PrincipalPermission>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct CreateIdentityRequest {
    /// Lower-case letters, digits, `.`, `_`, `-`; used as the `docker login` username.
    pub name: String,
    pub display_name: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct UpdatePrincipalRequest {
    pub enabled: Option<bool>,
    pub display_name: Option<String>,
}

#[derive(Serialize, ToSchema, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TokenStatus {
    Active,
    Expired,
    Revoked,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct TokenSummary {
    pub id: String,
    pub name: String,
    /// First characters of the secret, to recognize it.
    pub prefix: String,
    pub status: TokenStatus,
    pub created_at: String,
    pub created_by: Option<String>,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
    pub last_used_at: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct CreateTokenRequest {
    pub name: String,
    /// Optional RFC 3339 expiry; tokens never expire by default.
    pub expires_at: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct CreateTokenResponse {
    pub token: TokenSummary,
    /// The secret. Shown only in this response; it cannot be retrieved later.
    pub secret: String,
    /// Username to pair with the secret (`docker login -u`).
    pub username: String,
}

// --- Repositories ------------------------------------------------------------

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct RepositoryListQuery {
    /// Substring filter on the repository name.
    pub q: Option<String>,
    /// Page size (default 50, max 500).
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct RepositorySummary {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub created_by: Option<String>,
    pub tag_count: i64,
    pub manifest_count: i64,
    pub last_push_at: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct RepositoryListResponse {
    pub items: Vec<RepositorySummary>,
    pub total: i64,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct RepositoryRef {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct TagSummary {
    pub name: String,
    pub digest: String,
    pub updated_at: String,
    pub updated_by: Option<String>,
}

#[derive(Serialize, Deserialize, ToSchema, Clone)]
pub(crate) struct PlatformSummary {
    pub os: String,
    pub architecture: String,
    pub variant: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct ManifestSummary {
    pub digest: String,
    pub media_type: String,
    pub size: i64,
    pub artifact_type: Option<String>,
    /// Set for referrers (signatures, SBOMs, …): the manifest they refer to.
    pub subject_digest: Option<String>,
    pub platforms: Vec<PlatformSummary>,
    pub annotations: BTreeMap<String, String>,
    pub tags: Vec<String>,
    pub created_at: String,
    pub pushed_by: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct RepositoryDetail {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub created_by: Option<String>,
    pub tags: Vec<TagSummary>,
    pub manifests: Vec<ManifestSummary>,
}

// --- Permissions -------------------------------------------------------------

#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PermissionLevel {
    Read,
    Write,
    Owner,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct RepositoryPermission {
    pub principal: PrincipalRef,
    pub level: PermissionLevel,
    pub granted_by: Option<String>,
    pub granted_at: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct PrincipalPermission {
    pub repository: RepositoryRef,
    pub level: PermissionLevel,
    pub granted_by: Option<String>,
    pub granted_at: String,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct GrantPermissionRequest {
    pub level: PermissionLevel,
}

// --- Audit -------------------------------------------------------------------

#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub(crate) enum AuditOutcome {
    Ok,
    Denied,
    Error,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct AuditQuery {
    /// Exact principal name.
    pub principal: Option<String>,
    /// Exact repository name.
    pub repository: Option<String>,
    /// Exact action, e.g. `manifest.push`.
    pub action: Option<String>,
    pub outcome: Option<AuditOutcome>,
    /// Inclusive lower bound (RFC 3339).
    pub from: Option<String>,
    /// Exclusive upper bound (RFC 3339).
    pub to: Option<String>,
    /// `next_cursor` of the previous page.
    pub cursor: Option<String>,
    /// Page size (default 50, max 500).
    pub limit: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct AuditEventSummary {
    pub id: String,
    pub ts: String,
    pub principal_id: Option<String>,
    pub principal_name: Option<String>,
    pub action: String,
    pub repository: Option<String>,
    pub reference: Option<String>,
    pub digest: Option<String>,
    pub client_ip: Option<String>,
    pub user_agent: Option<String>,
    pub outcome: AuditOutcome,
    #[schema(value_type = Object)]
    pub detail: serde_json::Value,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct AuditListResponse {
    /// Newest first.
    pub items: Vec<AuditEventSummary>,
    /// Pass as `cursor` to get the next (older) page; absent on the last page.
    pub next_cursor: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct AuditActionsResponse {
    pub actions: Vec<String>,
}

// --- System ------------------------------------------------------------------

#[derive(Serialize, ToSchema)]
pub(crate) struct StorageSummary {
    /// `fs` or `s3`.
    pub backend: String,
    pub location: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct DatabaseSummary {
    pub path: String,
    pub size_bytes: i64,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct SystemResponse {
    pub version: String,
    pub started_at: String,
    pub storage: StorageSummary,
    pub database: DatabaseSummary,
    pub upload_dir: String,
    pub upload_ttl_seconds: i64,
    pub uploads_in_flight: i64,
    pub repository_count: i64,
    pub manifest_count: i64,
    pub blob_count: i64,
    pub blob_bytes: i64,
    pub gc_cron: Option<String>,
    pub gc_min_age_seconds: i64,
    pub audit_retention_days: i64,
    pub audit_blob_reads: bool,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct UploadSummary {
    pub uuid: String,
    pub repository: String,
    pub principal: String,
    /// Bytes received so far.
    pub offset: i64,
    pub started_at: String,
    pub last_activity_at: String,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct GcRequest {
    /// Report what would be deleted without deleting anything.
    #[serde(default)]
    pub dry_run: bool,
    /// Also delete manifests that no tag reaches (directly, via an index or as a referrer).
    #[serde(default)]
    pub delete_untagged: bool,
    /// Keep content younger than this (default `MINREGISTRY_GC_MIN_AGE`).
    pub min_age_seconds: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct GcResponse {
    pub dry_run: bool,
    pub delete_untagged: bool,
    pub min_age_seconds: i64,
    pub manifests_deleted: i64,
    pub blobs_deleted: i64,
    pub bytes_freed: i64,
    pub orphans_deleted: i64,
    pub blobs_kept_young: i64,
    pub errors: i64,
    pub duration_ms: i64,
}
