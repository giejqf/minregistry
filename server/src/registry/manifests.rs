//! `PUT`/`GET`/`HEAD`/`DELETE /v2/<name>/manifests/<reference>`.

use axum::{
    body::Body,
    http::{header, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
};
use futures::TryStreamExt;
use serde_json::json;

use super::{
    blobs::parse_digest,
    content,
    manifest_parse::{self, Kind, ParseError, Platform},
    names::valid_tag,
    Ctx, OciCode,
};
use crate::{
    audit::action,
    auth::Action,
    db,
    digest::Digest,
    error::{AppError, AppResult},
};

/// Config blobs larger than this are not read for platform information.
const MAX_CONFIG_READ: i64 = 1024 * 1024;
const ATTEMPTS: usize = 3;

enum Reference {
    Tag(String),
    Digest(Digest),
}

fn manifest_unknown(reference: &str) -> AppError {
    AppError::not_found(OciCode::ManifestUnknown, "manifest unknown to registry")
        .with_detail(json!({ "reference": reference }))
}

/// Parses a reference for writes: invalid references are client errors.
fn parse_reference(reference: &str) -> AppResult<Reference> {
    if reference.contains(':') {
        Ok(Reference::Digest(parse_digest(reference)?))
    } else if valid_tag(reference) {
        Ok(Reference::Tag(reference.to_string()))
    } else {
        Err(AppError::bad_request(OciCode::TagInvalid, "invalid tag").with_detail(json!({ "tag": reference })))
    }
}

/// Resolves a reference for reads; anything unresolvable is unknown (404).
async fn resolve(ctx: &Ctx, repository_id: i64, reference: &str) -> AppResult<Digest> {
    match parse_reference(reference) {
        Ok(Reference::Digest(d)) => Ok(d),
        Ok(Reference::Tag(tag)) => {
            let digest = db::tags::resolve(&ctx.state.db.read, repository_id, &tag)
                .await?
                .ok_or_else(|| manifest_unknown(reference))?;
            Digest::parse(&digest).map_err(AppError::internal)
        }
        Err(_) => Err(manifest_unknown(reference)),
    }
}

fn media_type_param(ctx: &Ctx) -> Option<String> {
    ctx.headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or_default().trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty())
}

pub(super) async fn put(ctx: &Ctx, name: &str, reference: &str, body: Body) -> AppResult<Response> {
    let reference_kind = parse_reference(reference)?;
    let repo = ctx.repo_for_push(name, action::MANIFEST_PUSH).await?;

    let bytes = axum::body::to_bytes(body, manifest_parse::MAX_MANIFEST_SIZE + 1)
        .await
        .map_err(|_| AppError::bad_request(OciCode::ManifestInvalid, "could not read the manifest body"))?;
    if bytes.len() > manifest_parse::MAX_MANIFEST_SIZE {
        return Err(AppError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            OciCode::SizeInvalid,
            "manifest exceeds the 4 MiB limit",
        ));
    }
    let digest = Digest::of(&bytes);
    if let Reference::Digest(expected) = &reference_kind {
        if *expected != digest {
            return Err(AppError::bad_request(OciCode::DigestInvalid, "manifest digest does not match the reference")
                .with_detail(json!({ "expected": expected.as_str(), "actual": digest.as_str() })));
        }
    }
    let content_type = media_type_param(ctx);
    let parsed = manifest_parse::parse(content_type.as_deref(), &bytes).map_err(|e| {
        let msg = e.to_string();
        let err = AppError::bad_request(OciCode::ManifestInvalid, msg);
        match e {
            ParseError::Unsupported(t) => err.with_detail(json!({ "mediaType": t })),
            ParseError::Invalid(_) => err,
        }
    })?;

    let platforms = match parsed.kind {
        Kind::Index => parsed.platforms.clone(),
        Kind::Image => config_platform(ctx, repo.id, &parsed).await.into_iter().collect(),
    };
    let platforms_json = (!platforms.is_empty()).then(|| serde_json::to_string(&platforms).unwrap_or_default());
    let annotations_json = parsed.annotations.as_ref().map(|a| serde_json::Value::Object(a.clone()).to_string());

    let staged = ctx.state.uploads.path(&format!("manifest-{}", uuid::Uuid::new_v4()));
    tokio::fs::write(&staged, &bytes).await?;
    let stored = store(
        ctx,
        repo.id,
        &digest,
        &bytes,
        &staged,
        &parsed,
        &reference_kind,
        platforms_json.as_deref(),
        annotations_json.as_deref(),
    )
    .await;
    let _ = tokio::fs::remove_file(&staged).await;
    stored?;

    let mut event = ctx
        .event(action::MANIFEST_PUSH)
        .repository(name)
        .reference(reference)
        .digest(&digest)
        .detail("mediaType", parsed.media_type.as_str());
    if let Some(subject) = &parsed.subject {
        event = event.detail("subject", subject.as_str());
    }
    ctx.record(event).await;

    let mut res = StatusCode::CREATED.into_response();
    let h = res.headers_mut();
    h.insert(
        header::LOCATION,
        HeaderValue::from_str(&format!("/v2/{name}/manifests/{digest}")).map_err(AppError::internal)?,
    );
    h.insert("docker-content-digest", HeaderValue::from_str(digest.as_str()).map_err(AppError::internal)?);
    h.insert(header::CONTENT_LENGTH, HeaderValue::from_static("0"));
    if let Some(subject) = &parsed.subject {
        h.insert("oci-subject", HeaderValue::from_str(subject.as_str()).map_err(AppError::internal)?);
    }
    Ok(res)
}

#[allow(clippy::too_many_arguments)]
async fn store(
    ctx: &Ctx,
    repository_id: i64,
    digest: &Digest,
    bytes: &[u8],
    staged: &std::path::Path,
    parsed: &manifest_parse::Parsed,
    reference: &Reference,
    platforms: Option<&str>,
    annotations: Option<&str>,
) -> AppResult<()> {
    let size = bytes.len() as u64;
    let mut put = content::needs_put(&ctx.state, digest).await?;
    for _ in 0..ATTEMPTS {
        if put {
            ctx.state.storage.put_blob_from_file(digest, staged).await?;
        }
        let mut tx = ctx.state.db.begin_write().await?;
        if !content::ensure_blob_row(&ctx.state, &mut tx, digest, size).await? {
            tx.rollback().await?;
            put = true;
            continue;
        }
        // Validate references inside the write lock so GC cannot interleave.
        for blob in &parsed.blobs {
            if db::blobs::linked_size(&mut *tx, repository_id, blob.as_str()).await?.is_none() {
                return Err(AppError::bad_request(
                    OciCode::ManifestBlobUnknown,
                    "manifest references a blob unknown to this repository",
                )
                .with_detail(json!({ "digest": blob.as_str() })));
            }
        }
        for child in &parsed.manifests {
            if !db::manifests::exists(&mut *tx, repository_id, child.as_str()).await? {
                return Err(AppError::bad_request(
                    OciCode::ManifestBlobUnknown,
                    "index references a manifest unknown to this repository",
                )
                .with_detail(json!({ "digest": child.as_str() })));
            }
        }
        let now = crate::time::now();
        let inserted = db::manifests::insert(
            &mut tx,
            &db::manifests::NewManifest {
                repository_id,
                digest: digest.as_str(),
                media_type: &parsed.media_type,
                size: i64::try_from(size).map_err(AppError::internal)?,
                subject_digest: parsed.subject.as_ref().map(|s| s.as_str()),
                artifact_type: parsed.artifact_type.as_deref(),
                annotations,
                platforms,
                pushed_by: ctx.principal.id,
                now: &now,
            },
        )
        .await?;
        if inserted {
            for blob in &parsed.blobs {
                db::manifests::insert_ref(&mut tx, repository_id, digest.as_str(), blob.as_str(), "blob").await?;
            }
            for child in &parsed.manifests {
                db::manifests::insert_ref(&mut tx, repository_id, digest.as_str(), child.as_str(), "manifest").await?;
            }
        }
        if let Reference::Tag(tag) = reference {
            db::tags::upsert(&mut tx, repository_id, tag, digest.as_str(), ctx.principal.id, &now).await?;
        }
        tx.commit().await?;
        return Ok(());
    }
    Err(AppError::internal(format_args!("manifest {digest} kept disappearing from storage during commit")))
}

/// The platform of a single-image manifest, read from its config blob.
async fn config_platform(ctx: &Ctx, repository_id: i64, parsed: &manifest_parse::Parsed) -> Option<Platform> {
    let (digest, media_type, size) = parsed.config.as_ref()?;
    if !matches!(media_type.as_str(), manifest_parse::OCI_CONFIG | manifest_parse::DOCKER_CONFIG)
        || *size > MAX_CONFIG_READ
    {
        return None;
    }
    db::blobs::linked_size(&ctx.state.db.read, repository_id, digest.as_str()).await.ok()??;
    let stream = ctx.state.storage.get_blob(digest, None).await.ok()?;
    let chunks: Vec<bytes::Bytes> = stream.try_collect().await.ok()?;
    manifest_parse::platform_from_config(&chunks.concat())
}

pub(super) async fn get(ctx: &Ctx, method: &Method, name: &str, reference: &str) -> AppResult<Response> {
    let repo = ctx.existing_repo(name, Action::Pull, action::MANIFEST_PULL).await?;
    let digest = resolve(ctx, repo.id, reference).await?;
    let head = db::manifests::head(&ctx.state.db.read, repo.id, digest.as_str())
        .await?
        .ok_or_else(|| manifest_unknown(reference))?;
    let body = if *method == Method::HEAD {
        Body::empty()
    } else {
        let stream = ctx.state.storage.get_blob(&digest, None).await.map_err(|e| {
            tracing::error!(%digest, error = %e, "manifest is recorded in the database but unreadable from storage");
            AppError::from(e)
        })?;
        ctx.record(ctx.event(action::MANIFEST_PULL).repository(name).reference(reference).digest(&digest)).await;
        Body::from_stream(stream)
    };
    let mut res = body.into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(&head.media_type).map_err(AppError::internal)?);
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(head.size));
    h.insert("docker-content-digest", HeaderValue::from_str(digest.as_str()).map_err(AppError::internal)?);
    h.insert(header::ETAG, HeaderValue::from_str(&format!("\"{digest}\"")).map_err(AppError::internal)?);
    Ok(res)
}

pub(super) async fn delete(ctx: &Ctx, name: &str, reference: &str) -> AppResult<Response> {
    let reference_kind = parse_reference(reference)?;
    let audit_action = match reference_kind {
        Reference::Tag(_) => action::TAG_DELETE,
        Reference::Digest(_) => action::MANIFEST_DELETE,
    };
    let repo = ctx.existing_repo(name, Action::Delete, audit_action).await?;
    let event = ctx.event(audit_action).repository(name).reference(reference);
    match reference_kind {
        Reference::Tag(tag) => {
            let digest = db::tags::resolve(&ctx.state.db.read, repo.id, &tag).await?;
            if !db::tags::delete(&ctx.state.db.write, repo.id, &tag).await? {
                return Err(manifest_unknown(reference));
            }
            ctx.record(match digest {
                Some(d) => event.digest(d),
                None => event,
            })
            .await;
        }
        Reference::Digest(digest) => {
            // Tags pointing at the manifest go with it.
            if !db::manifests::delete(&ctx.state.db.write, repo.id, digest.as_str()).await? {
                return Err(manifest_unknown(reference));
            }
            ctx.record(event.digest(&digest)).await;
        }
    }
    let mut res = StatusCode::ACCEPTED.into_response();
    res.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from_static("0"));
    Ok(res)
}
