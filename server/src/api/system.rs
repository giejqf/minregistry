//! Storage/database information, in-flight uploads and garbage collection.

use std::time::Duration;

use axum::{extract::State, Json};

use super::{admin_event, dto::*, validation};
use crate::{
    app::AppState,
    audit::{action, ClientInfo, Outcome},
    auth::AdminSession,
    db,
    error::{AppError, AppResult},
    gc::{self, GcOptions},
};

fn i(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// Storage backend, database size and registry totals.
#[utoipa::path(
    get,
    path = "/system",
    tag = "system",
    responses((status = 200, body = SystemResponse)),
    security(("session" = []))
)]
pub(crate) async fn get_system(State(state): State<AppState>, _admin: AdminSession) -> AppResult<Json<SystemResponse>> {
    let info = state.storage.describe();
    let blobs = db::blobs::stats(&state.db.read).await?;
    let core = &state.cfg.core;
    Ok(Json(SystemResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        started_at: state.started_at.clone(),
        storage: StorageSummary { backend: info.backend.to_string(), location: info.location },
        database: DatabaseSummary {
            path: state.db.path().display().to_string(),
            size_bytes: i(state.db.size_bytes().await),
        },
        upload_dir: state.uploads.dir().display().to_string(),
        upload_ttl_seconds: i(core.upload_ttl.as_secs()),
        uploads_in_flight: i64::try_from(db::uploads::all_ids(&state.db.read).await?.len()).unwrap_or(i64::MAX),
        repository_count: db::repositories::count(&state.db.read, None).await?,
        manifest_count: db::manifests::count(&state.db.read).await?,
        blob_count: blobs.count,
        blob_bytes: blobs.bytes,
        gc_cron: core.gc_cron.clone(),
        gc_min_age_seconds: i(core.gc_min_age.as_secs()),
        audit_retention_days: i64::from(core.audit_retention_days),
        audit_blob_reads: core.audit_blob_reads,
    }))
}

/// Upload sessions in flight.
#[utoipa::path(
    get,
    path = "/uploads",
    tag = "system",
    responses((status = 200, body = Vec<UploadSummary>)),
    security(("session" = []))
)]
pub(crate) async fn list_uploads(
    State(state): State<AppState>,
    _admin: AdminSession,
) -> AppResult<Json<Vec<UploadSummary>>> {
    Ok(Json(
        db::uploads::list(&state.db.read)
            .await?
            .into_iter()
            .map(|u| UploadSummary {
                uuid: u.uuid,
                repository: u.repository,
                principal: u.principal,
                offset: u.offset,
                started_at: u.started_at,
                last_activity_at: u.last_activity_at,
            })
            .collect(),
    ))
}

/// Runs garbage collection now (use `dry_run` first).
#[utoipa::path(
    post,
    path = "/gc",
    tag = "system",
    request_body = GcRequest,
    responses((status = 200, body = GcResponse), (status = 400, body = ErrorResponse)),
    security(("session" = []))
)]
pub(crate) async fn run_gc(
    State(state): State<AppState>,
    admin: AdminSession,
    client: ClientInfo,
    Json(req): Json<GcRequest>,
) -> AppResult<Json<GcResponse>> {
    let min_age = match req.min_age_seconds {
        None => state.cfg.core.gc_min_age,
        Some(s) if s >= 0 => Duration::from_secs(s as u64),
        Some(_) => return Err(validation("min_age_seconds must not be negative")),
    };
    let opts = GcOptions { dry_run: req.dry_run, delete_untagged: req.delete_untagged, min_age };
    let _guard = state.gc_lock.lock().await;
    let event = admin_event(&admin, &client, action::GC_RUN).detail("trigger", "api");
    match gc::run(&state.db, state.storage.as_ref(), &opts).await {
        Ok(r) => {
            state.audit.record(event.detail("report", serde_json::to_value(&r).unwrap_or_default())).await;
            Ok(Json(GcResponse {
                dry_run: r.dry_run,
                delete_untagged: r.delete_untagged,
                min_age_seconds: i(r.min_age_seconds),
                manifests_deleted: i(r.manifests_deleted),
                blobs_deleted: i(r.blobs_deleted),
                bytes_freed: i(r.bytes_freed),
                orphans_deleted: i(r.orphans_deleted),
                blobs_kept_young: i(r.blobs_kept_young),
                errors: i(r.errors),
                duration_ms: i(r.duration_ms),
            }))
        }
        Err(e) => {
            state.audit.record(event.outcome(Outcome::Error).detail("error", e.to_string())).await;
            Err(AppError::internal(format_args!("garbage collection failed: {e:#}")))
        }
    }
}
