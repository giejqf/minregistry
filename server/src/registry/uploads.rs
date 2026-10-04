//! Blob uploads: monolithic (POST/PUT with `digest`), streamed and chunked
//! (PATCH), cross-repository mounts, status and cancellation.
//!
//! Data is staged in `MINREGISTRY_UPLOAD_DIR/<uuid>` and hashed as it arrives,
//! so the digest is verified exactly once, without reading the blob back.
//! The running hash lives in memory; after a restart it is rebuilt from the
//! staging file (docs/adr/0005). Bytes received before a client disconnects
//! are kept, so interrupted uploads can resume from the reported `Range`.

use std::{
    collections::HashMap,
    io::SeekFrom,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use axum::{
    body::Body,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufWriter},
    sync::OwnedMutexGuard,
};

use super::{range::parse_content_range, Ctx, OciCode};
use crate::{
    audit::{action, Outcome},
    auth::Action,
    db,
    digest::{Digest, DigestError},
    error::{AppError, AppResult},
};

const WRITE_BUFFER: usize = 1024 * 1024;

/// In-memory state of an upload session, guarded by a per-upload lock that
/// serializes concurrent requests for the same session.
#[derive(Default)]
struct Live {
    hasher: Option<Sha256>,
    /// Bytes covered by `hasher`.
    hashed: u64,
}

pub(crate) struct UploadManager {
    dir: PathBuf,
    live: Mutex<HashMap<String, Arc<tokio::sync::Mutex<Live>>>>,
}

impl UploadManager {
    pub(crate) async fn new(dir: PathBuf) -> std::io::Result<Self> {
        tokio::fs::create_dir_all(&dir).await?;
        Ok(UploadManager { dir, live: Mutex::new(HashMap::new()) })
    }

    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    pub(crate) fn path(&self, uuid: &str) -> PathBuf {
        self.dir.join(uuid)
    }

    fn entry(&self, uuid: &str) -> Arc<tokio::sync::Mutex<Live>> {
        let mut map = self.live.lock().unwrap_or_else(|p| p.into_inner());
        map.entry(uuid.to_string()).or_default().clone()
    }

    pub(crate) fn forget(&self, uuid: &str) {
        self.live.lock().unwrap_or_else(|p| p.into_inner()).remove(uuid);
    }

    /// Removes the staging file and in-memory state of a session.
    pub(crate) async fn discard(&self, uuid: &str) {
        self.forget(uuid);
        if let Err(e) = tokio::fs::remove_file(self.path(uuid)).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(error = %e, uuid, "failed to remove upload staging file");
            }
        }
    }
}

/// Rebuilds the running hash from the staging file when it is missing or stale.
async fn ensure_hasher(path: &Path, live: &mut Live, offset: u64) -> std::io::Result<()> {
    if live.hasher.is_some() && live.hashed == offset {
        return Ok(());
    }
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut remaining = offset;
    let mut buf = vec![0u8; 256 * 1024];
    while remaining > 0 {
        let want = buf.len().min(usize::try_from(remaining).unwrap_or(usize::MAX));
        let n = file.read(&mut buf[..want]).await?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "staging file is shorter than the upload offset",
            ));
        }
        hasher.update(&buf[..n]);
        remaining -= n as u64;
    }
    live.hasher = Some(hasher);
    live.hashed = offset;
    Ok(())
}

struct Appended {
    offset: u64,
    /// The client stream failed (e.g. disconnected) after `offset` bytes.
    client_error: Option<String>,
}

/// Appends the request body at `offset`, hashing as bytes arrive. A body that
/// ends before its declared `expected` length counts as interrupted.
async fn append(path: &Path, live: &mut Live, offset: u64, body: Body, expected: Option<u64>) -> AppResult<Appended> {
    let file = tokio::fs::OpenOptions::new().write(true).open(path).await?;
    let result: std::io::Result<Appended> = async {
        // Drop any bytes past the committed offset (from an earlier failure).
        file.set_len(offset).await?;
        let mut file = file;
        file.seek(SeekFrom::Start(offset)).await?;
        let mut writer = BufWriter::with_capacity(WRITE_BUFFER, file);
        let hasher = live.hasher.as_mut().ok_or_else(|| std::io::Error::other("upload hash state missing"))?;
        let mut stream = body.into_data_stream();
        let mut written = 0u64;
        let mut client_error = None;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    writer.write_all(&bytes).await?;
                    hasher.update(&bytes);
                    written += bytes.len() as u64;
                }
                Err(e) => {
                    client_error = Some(e.to_string());
                    break;
                }
            }
        }
        writer.flush().await?;
        if client_error.is_none() {
            if let Some(expected) = expected.filter(|e| written < *e) {
                client_error = Some(format!("request body ended after {written} of {expected} bytes"));
            }
        }
        Ok(Appended { offset: offset + written, client_error })
    }
    .await;
    match result {
        Ok(a) => {
            live.hashed = a.offset;
            Ok(a)
        }
        Err(e) => {
            // The hash may cover bytes that never reached the disk: discard
            // this request's data and force a rebuild.
            live.hasher = None;
            if let Ok(f) = tokio::fs::OpenOptions::new().write(true).open(path).await {
                let _ = f.set_len(offset).await;
            }
            Err(e.into())
        }
    }
}

fn content_length(headers: &axum::http::HeaderMap) -> Option<u64> {
    headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok())
}

/// Receives a chunk of a session in a task of its own. When a client
/// disconnects mid-chunk the server may cancel the request handler; the task
/// still records the bytes that arrived, so the upload can resume from the
/// reported `Range`, and the in-memory hash never covers bytes that the
/// recorded offset does not.
async fn receive(
    ctx: &Ctx,
    uuid: &str,
    mut live: OwnedMutexGuard<Live>,
    offset: u64,
    body: Body,
) -> AppResult<(OwnedMutexGuard<Live>, Appended)> {
    let state = ctx.state.clone();
    let uuid = uuid.to_string();
    let expected = content_length(&ctx.headers);
    tokio::spawn(async move {
        let path = state.uploads.path(&uuid);
        let appended = append(&path, &mut live, offset, body, expected).await?;
        if appended.offset != offset {
            let recorded = i64::try_from(appended.offset).unwrap_or(i64::MAX);
            db::uploads::set_offset(&state.db.write, &uuid, recorded, &crate::time::now()).await?;
        }
        if let Some(e) = &appended.client_error {
            tracing::info!(%uuid, offset = appended.offset, error = %e, "upload interrupted; progress kept");
        }
        Ok((live, appended))
    })
    .await
    .map_err(|e| AppError::internal(format_args!("upload task failed: {e}")))?
}

fn range_header(offset: u64) -> HeaderValue {
    // An empty session reports 0-0, as the reference implementation does.
    HeaderValue::from_str(&format!("0-{}", offset.saturating_sub(1))).unwrap_or(HeaderValue::from_static("0-0"))
}

fn upload_location(name: &str, uuid: &str) -> String {
    format!("/v2/{name}/blobs/uploads/{uuid}")
}

fn session_response(status: StatusCode, name: &str, uuid: &str, offset: u64) -> Response {
    let mut res = status.into_response();
    let h = res.headers_mut();
    if let Ok(loc) = HeaderValue::from_str(&upload_location(name, uuid)) {
        h.insert(header::LOCATION, loc);
    }
    h.insert(header::RANGE, range_header(offset));
    if let Ok(id) = HeaderValue::from_str(uuid) {
        h.insert("docker-upload-uuid", id);
    }
    h.insert(header::CONTENT_LENGTH, HeaderValue::from_static("0"));
    res
}

fn created_response(name: &str, digest: &Digest) -> Response {
    let mut res = StatusCode::CREATED.into_response();
    let h = res.headers_mut();
    if let Ok(loc) = HeaderValue::from_str(&format!("/v2/{name}/blobs/{digest}")) {
        h.insert(header::LOCATION, loc);
    }
    if let Ok(d) = HeaderValue::from_str(digest.as_str()) {
        h.insert("docker-content-digest", d);
    }
    h.insert(header::CONTENT_LENGTH, HeaderValue::from_static("0"));
    res
}

fn parse_expected_digest(raw: &str) -> AppResult<Digest> {
    Digest::parse(raw).map_err(|e| match e {
        DigestError::Invalid => {
            AppError::bad_request(OciCode::DigestInvalid, "invalid digest").with_detail(json!({ "digest": raw }))
        }
        DigestError::Unsupported => AppError::bad_request(OciCode::Unsupported, "only sha256 digests are supported")
            .with_detail(json!({ "digest": raw })),
    })
}

/// `POST /v2/<name>/blobs/uploads/[?digest=|?mount=&from=]`
pub(super) async fn start(ctx: &Ctx, name: &str, body: Body) -> AppResult<Response> {
    let repo = ctx.repo_for_push(name, action::BLOB_UPLOAD).await?;

    if let Some(mount) = ctx.query("mount") {
        if let Some(res) = try_mount(ctx, name, repo.id, mount).await? {
            return Ok(res);
        }
        // Per the spec, a mount that cannot be honoured becomes a normal upload session.
    }

    if let Some(raw) = ctx.query("digest") {
        let expected = parse_expected_digest(raw)?;
        return monolithic(ctx, name, repo.id, expected, body).await;
    }

    let uuid = uuid::Uuid::new_v4().to_string();
    tokio::fs::File::create(ctx.state.uploads.path(&uuid)).await?;
    db::uploads::insert(&ctx.state.db.write, &uuid, repo.id, ctx.principal.id, &crate::time::now()).await?;
    let entry = ctx.state.uploads.entry(&uuid);
    entry.lock().await.hasher = Some(Sha256::new());
    Ok(session_response(StatusCode::ACCEPTED, name, &uuid, 0))
}

/// A cross-repository mount: 201 when `from` is readable and holds the blob.
async fn try_mount(ctx: &Ctx, name: &str, repo_id: i64, mount: &str) -> AppResult<Option<Response>> {
    let (Some(from), Ok(digest)) = (ctx.query("from"), Digest::parse(mount)) else {
        return Ok(None);
    };
    if !super::names::valid_name(from) {
        return Ok(None);
    }
    let source = match crate::auth::authz::authorize(&ctx.state, &ctx.principal, from, Action::Mount).await {
        Ok(Some(source)) => source,
        Ok(None) => return Ok(None),
        Err(crate::auth::authz::AuthzError::Denied) => {
            ctx.record(
                ctx.event(action::BLOB_MOUNT)
                    .outcome(Outcome::Denied)
                    .repository(name)
                    .digest(&digest)
                    .detail("from", from)
                    .detail("reason", "no read access to the source repository"),
            )
            .await;
            return Ok(None);
        }
        Err(crate::auth::authz::AuthzError::Db(e)) => return Err(e.into()),
    };
    if db::blobs::linked_size(&ctx.state.db.read, source.id, digest.as_str()).await?.is_none() {
        return Ok(None);
    }
    let mut tx = ctx.state.db.begin_write().await?;
    // Re-check inside the write lock: the source link may have been removed.
    if db::blobs::linked_size(&mut *tx, source.id, digest.as_str()).await?.is_none() {
        return Ok(None);
    }
    db::blobs::unmark(&mut tx, digest.as_str()).await?;
    db::blobs::link(&mut tx, repo_id, digest.as_str(), &crate::time::now()).await?;
    tx.commit().await?;
    ctx.record(ctx.event(action::BLOB_MOUNT).repository(name).digest(&digest).detail("from", from)).await;
    Ok(Some(created_response(name, &digest)))
}

/// Upload in a single request (`POST ?digest=`).
async fn monolithic(ctx: &Ctx, name: &str, repo_id: i64, expected: Digest, body: Body) -> AppResult<Response> {
    let tmp = ctx.state.uploads.path(&format!("mono-{}", uuid::Uuid::new_v4()));
    tokio::fs::File::create(&tmp).await?;
    let mut live = Live { hasher: Some(Sha256::new()), hashed: 0 };
    let result = async {
        let appended = append(&tmp, &mut live, 0, body, content_length(&ctx.headers)).await?;
        if let Some(e) = appended.client_error {
            return Err(AppError::bad_request(OciCode::BlobUploadInvalid, format!("upload interrupted: {e}")));
        }
        finish(ctx, name, repo_id, &tmp, &mut live, appended.offset, &expected).await
    }
    .await;
    let _ = tokio::fs::remove_file(&tmp).await;
    result
}

/// Verifies the digest, commits the blob and audits the upload.
async fn finish(
    ctx: &Ctx,
    name: &str,
    repo_id: i64,
    path: &Path,
    live: &mut Live,
    size: u64,
    expected: &Digest,
) -> AppResult<Response> {
    ensure_hasher(path, live, size).await?;
    let actual = Digest::from_sha256(&live.hasher.clone().unwrap_or_default().finalize());
    if actual != *expected {
        ctx.record(
            ctx.event(action::BLOB_UPLOAD)
                .outcome(Outcome::Error)
                .repository(name)
                .digest(expected)
                .detail("reason", "digest mismatch")
                .detail("actual", actual.to_string()),
        )
        .await;
        return Err(AppError::bad_request(OciCode::DigestInvalid, "provided digest did not match uploaded content")
            .with_detail(json!({ "expected": expected.as_str(), "actual": actual.as_str() })));
    }
    super::content::commit_blob(&ctx.state, expected, size, path, repo_id).await?;
    ctx.record(ctx.event(action::BLOB_UPLOAD).repository(name).digest(expected).detail("size", size)).await;
    Ok(created_response(name, expected))
}

/// Looks up a session for `name`, owned by the caller (or any, for admins).
async fn session(ctx: &Ctx, name: &str, uuid: &str) -> AppResult<(i64, db::uploads::UploadRow)> {
    let repo = ctx.existing_repo(name, Action::Push, action::BLOB_UPLOAD).await?;
    let unknown = || AppError::not_found(OciCode::BlobUploadUnknown, "blob upload unknown to registry");
    if uuid::Uuid::parse_str(uuid).is_err() {
        return Err(unknown());
    }
    let row = db::uploads::get(&ctx.state.db.read, uuid).await?.ok_or_else(unknown)?;
    if row.repository_id != repo.id || (row.principal_id != ctx.principal.id && !ctx.principal.is_admin) {
        return Err(unknown());
    }
    Ok((repo.id, row))
}

fn offset_of(row: &db::uploads::UploadRow) -> u64 {
    u64::try_from(row.offset).unwrap_or(0)
}

/// Checks a `Content-Range` against the session offset (416 if out of order).
fn check_content_range(ctx: &Ctx, name: &str, uuid: &str, offset: u64) -> AppResult<()> {
    let Some(raw) = ctx.headers.get(header::CONTENT_RANGE).and_then(|v| v.to_str().ok()) else {
        return Ok(());
    };
    let range = parse_content_range(raw)
        .ok_or_else(|| AppError::bad_request(OciCode::BlobUploadInvalid, "invalid Content-Range"))?;
    let length =
        ctx.headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    if range.start != offset {
        return Err(AppError::new(StatusCode::RANGE_NOT_SATISFIABLE, OciCode::BlobUploadInvalid, "chunk out of order")
            .with_detail(json!({ "expected_start": offset }))
            .with_header(
                header::LOCATION,
                HeaderValue::from_str(&upload_location(name, uuid)).map_err(AppError::internal)?,
            )
            .with_header(header::RANGE, range_header(offset)));
    }
    if length.is_some_and(|l| l != range.len()) {
        return Err(AppError::bad_request(OciCode::BlobUploadInvalid, "Content-Length does not match Content-Range"));
    }
    Ok(())
}

/// `PATCH /v2/<name>/blobs/uploads/<uuid>` — streamed or chunked data.
pub(super) async fn patch(ctx: &Ctx, name: &str, uuid: &str, body: Body) -> AppResult<Response> {
    let (_, _) = session(ctx, name, uuid).await?;
    let mut live = ctx.state.uploads.entry(uuid).lock_owned().await;
    // Re-read under the session lock: a concurrent request may have advanced it.
    let row = db::uploads::get(&ctx.state.db.read, uuid)
        .await?
        .ok_or_else(|| AppError::not_found(OciCode::BlobUploadUnknown, "blob upload unknown to registry"))?;
    let offset = offset_of(&row);
    check_content_range(ctx, name, uuid, offset)?;
    let path = ctx.state.uploads.path(uuid);
    ensure_hasher(&path, &mut live, offset).await.map_err(|e| missing_staging(uuid, e))?;
    let (_live, appended) = receive(ctx, uuid, live, offset, body).await?;
    if appended.client_error.is_some() {
        return Err(AppError::bad_request(OciCode::BlobUploadInvalid, "upload interrupted"));
    }
    Ok(session_response(StatusCode::ACCEPTED, name, uuid, appended.offset))
}

fn missing_staging(uuid: &str, e: std::io::Error) -> AppError {
    tracing::warn!(uuid, error = %e, "upload staging data unusable");
    AppError::not_found(OciCode::BlobUploadUnknown, "blob upload data is no longer available; restart the upload")
}

/// `PUT /v2/<name>/blobs/uploads/<uuid>?digest=` — optional last chunk, then commit.
pub(super) async fn put(ctx: &Ctx, name: &str, uuid: &str, body: Body) -> AppResult<Response> {
    let (repo_id, _) = session(ctx, name, uuid).await?;
    let expected = parse_expected_digest(ctx.query("digest").unwrap_or_default())?;
    let mut live = ctx.state.uploads.entry(uuid).lock_owned().await;
    let row = db::uploads::get(&ctx.state.db.read, uuid)
        .await?
        .ok_or_else(|| AppError::not_found(OciCode::BlobUploadUnknown, "blob upload unknown to registry"))?;
    let mut offset = offset_of(&row);
    check_content_range(ctx, name, uuid, offset)?;
    let path = ctx.state.uploads.path(uuid);
    ensure_hasher(&path, &mut live, offset).await.map_err(|e| missing_staging(uuid, e))?;
    let (mut live, appended) = receive(ctx, uuid, live, offset, body).await?;
    if let Some(e) = appended.client_error {
        return Err(AppError::bad_request(OciCode::BlobUploadInvalid, format!("upload interrupted: {e}")));
    }
    offset = appended.offset;
    let result = finish(ctx, name, repo_id, &path, &mut live, offset, &expected).await;
    // Success or digest mismatch both end the session.
    if result.is_ok() || result.as_ref().is_err_and(|e| e.code == OciCode::DigestInvalid) {
        db::uploads::delete(&ctx.state.db.write, uuid).await?;
        drop(live);
        ctx.state.uploads.discard(uuid).await;
    }
    result
}

/// How long a status request waits for a chunk that is still being received.
const STATUS_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// `GET /v2/<name>/blobs/uploads/<uuid>` — progress of a session.
pub(super) async fn status(ctx: &Ctx, name: &str, uuid: &str) -> AppResult<Response> {
    let (_, _) = session(ctx, name, uuid).await?;
    // A client that gave up on a chunk asks for the progress right away, while
    // the server may still be storing the bytes that already arrived. Wait for
    // that chunk to finish so `Range` covers everything that will be kept.
    let entry = ctx.state.uploads.entry(uuid);
    let _settled = tokio::time::timeout(STATUS_WAIT, entry.lock()).await.ok();
    let row = db::uploads::get(&ctx.state.db.read, uuid)
        .await?
        .ok_or_else(|| AppError::not_found(OciCode::BlobUploadUnknown, "blob upload unknown to registry"))?;
    Ok(session_response(StatusCode::NO_CONTENT, name, uuid, offset_of(&row)))
}

/// `DELETE /v2/<name>/blobs/uploads/<uuid>` — cancel a session.
pub(super) async fn cancel(ctx: &Ctx, name: &str, uuid: &str) -> AppResult<Response> {
    let (_, _) = session(ctx, name, uuid).await?;
    let entry = ctx.state.uploads.entry(uuid);
    let live = entry.lock().await;
    db::uploads::delete(&ctx.state.db.write, uuid).await?;
    drop(live);
    ctx.state.uploads.discard(uuid).await;
    ctx.record(ctx.event(action::UPLOAD_CANCEL).repository(name).detail("uuid", uuid)).await;
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from_static("0"));
    Ok(res)
}
