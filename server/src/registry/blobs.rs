//! `GET`/`HEAD`/`DELETE /v2/<name>/blobs/<digest>`.

use axum::{
    body::Body,
    http::{header, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::json;

use super::{
    range::{parse_range, RangeRequest},
    Ctx, OciCode,
};
use crate::{
    audit::action,
    auth::Action,
    db,
    digest::{Digest, DigestError},
    error::{AppError, AppResult},
    storage::StorageError,
};

pub(super) fn parse_digest(raw: &str) -> AppResult<Digest> {
    Digest::parse(raw).map_err(|e| {
        let msg = match e {
            DigestError::Invalid => "invalid digest",
            DigestError::Unsupported => "unsupported digest algorithm",
        };
        AppError::bad_request(OciCode::DigestInvalid, msg).with_detail(json!({ "digest": raw }))
    })
}

fn blob_unknown(digest: &str) -> AppError {
    AppError::not_found(OciCode::BlobUnknown, "blob unknown to registry").with_detail(json!({ "digest": digest }))
}

pub(super) async fn get(ctx: &Ctx, method: &Method, name: &str, raw_digest: &str) -> AppResult<Response> {
    let repo = ctx.existing_repo(name, Action::Pull, action::BLOB_PULL).await?;
    let digest = parse_digest(raw_digest)?;
    let size = db::blobs::linked_size(&ctx.state.db.read, repo.id, digest.as_str())
        .await?
        .ok_or_else(|| blob_unknown(raw_digest))?;
    let size = u64::try_from(size).map_err(AppError::internal)?;

    let range_header = ctx.headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    let range = match parse_range(range_header, size) {
        RangeRequest::Full => None,
        RangeRequest::Partial(r) => Some(r),
        RangeRequest::Unsatisfiable => {
            return Ok(AppError::new(
                StatusCode::RANGE_NOT_SATISFIABLE,
                OciCode::RangeInvalid,
                "requested range not satisfiable",
            )
            .with_header(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes */{size}")).map_err(AppError::internal)?,
            )
            .into_oci_response());
        }
    };

    let body = if *method == Method::HEAD {
        Body::empty()
    } else {
        let stream = match ctx.state.storage.get_blob(&digest, range).await {
            Ok(s) => s,
            Err(StorageError::NotFound) => {
                tracing::error!(%digest, "blob is recorded in the database but missing from storage");
                return Err(blob_unknown(raw_digest));
            }
            Err(e) => return Err(e.into()),
        };
        if ctx.state.cfg.core.audit_blob_reads {
            let mut event = ctx.event(action::BLOB_PULL).repository(name).digest(&digest);
            if let Some(r) = range {
                event = event.detail("range", format!("{}-{}", r.start, r.end));
            }
            ctx.record(event).await;
        }
        Body::from_stream(stream)
    };

    let mut res = body.into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    h.insert("docker-content-digest", HeaderValue::from_str(digest.as_str()).map_err(AppError::internal)?);
    h.insert(header::ETAG, HeaderValue::from_str(&format!("\"{digest}\"")).map_err(AppError::internal)?);
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("max-age=31536000"));
    match range {
        Some(r) => {
            *res.status_mut() = StatusCode::PARTIAL_CONTENT;
            let h = res.headers_mut();
            h.insert(header::CONTENT_LENGTH, HeaderValue::from(r.len()));
            h.insert(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes {}-{}/{size}", r.start, r.end)).map_err(AppError::internal)?,
            );
        }
        None => {
            res.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from(size));
        }
    }
    Ok(res)
}

pub(super) async fn delete(ctx: &Ctx, name: &str, raw_digest: &str) -> AppResult<Response> {
    let repo = ctx.existing_repo(name, Action::Delete, action::BLOB_DELETE).await?;
    let digest = parse_digest(raw_digest)?;
    // Only the repository link is removed; storage is reclaimed by GC.
    if !db::blobs::unlink(&ctx.state.db.write, repo.id, digest.as_str()).await? {
        return Err(blob_unknown(raw_digest));
    }
    ctx.record(ctx.event(action::BLOB_DELETE).repository(name).digest(&digest)).await;
    let mut res = StatusCode::ACCEPTED.into_response();
    res.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from_static("0"));
    Ok(res)
}
