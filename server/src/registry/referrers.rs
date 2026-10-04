//! `GET /v2/<name>/referrers/<digest>[?artifactType=]` — an image index of
//! the manifests whose `subject` is `<digest>`.

use axum::{
    http::{header, HeaderValue},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Map, Value};

use super::{blobs::parse_digest, manifest_parse::OCI_INDEX, Ctx};
use crate::{audit::action, auth::Action, db, error::AppResult};

pub(super) async fn list(ctx: &Ctx, name: &str, raw_digest: &str) -> AppResult<Response> {
    let repo = ctx.existing_repo(name, Action::Pull, action::REFERRERS_LIST).await?;
    let digest = parse_digest(raw_digest)?;
    let artifact_type = ctx.query("artifactType").filter(|t| !t.is_empty());
    let rows = db::manifests::referrers(&ctx.state.db.read, repo.id, digest.as_str(), artifact_type).await?;

    let manifests: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            let mut d = Map::new();
            d.insert("mediaType".into(), r.media_type.into());
            d.insert("digest".into(), r.digest.into());
            d.insert("size".into(), r.size.into());
            if let Some(t) = r.artifact_type {
                d.insert("artifactType".into(), t.into());
            }
            if let Some(Ok(Value::Object(a))) = r.annotations.as_deref().map(serde_json::from_str::<Value>) {
                if !a.is_empty() {
                    d.insert("annotations".into(), Value::Object(a));
                }
            }
            Value::Object(d)
        })
        .collect();

    let mut event = ctx.event(action::REFERRERS_LIST).repository(name).digest(&digest).detail("count", manifests.len());
    if let Some(t) = artifact_type {
        event = event.detail("artifactType", t);
    }
    ctx.record(event).await;

    let index = json!({ "schemaVersion": 2, "mediaType": OCI_INDEX, "manifests": manifests });
    let mut res = Json(index).into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(OCI_INDEX));
    if artifact_type.is_some() {
        h.insert("oci-filters-applied", HeaderValue::from_static("artifactType"));
    }
    Ok(res)
}
