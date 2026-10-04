//! `GET /v2/<name>/tags/list[?n=&last=]` — lexically sorted, paginated.

use axum::{
    http::{header, HeaderValue},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use super::{Ctx, OciCode};
use crate::{
    audit::action,
    auth::Action,
    db,
    error::{AppError, AppResult},
};

/// Upper bound on one page, whatever `n` asks for.
const MAX_PAGE: i64 = 10_000;

pub(super) async fn list(ctx: &Ctx, name: &str) -> AppResult<Response> {
    let repo = ctx.existing_repo(name, Action::Pull, action::TAG_LIST).await?;
    let n = match ctx.query("n") {
        None | Some("") => None,
        Some(raw) => Some(raw.parse::<i64>().ok().filter(|n| *n >= 0).ok_or_else(|| {
            AppError::bad_request(OciCode::PaginationNumberInvalid, "invalid number of results requested")
        })?),
    };
    let last = ctx.query("last").filter(|l| !l.is_empty());
    let limit = n.unwrap_or(MAX_PAGE).min(MAX_PAGE);
    // Fetch one extra row to learn whether another page follows.
    let mut tags =
        if limit == 0 { Vec::new() } else { db::tags::names(&ctx.state.db.read, repo.id, last, limit + 1).await? };
    let more = tags.len() as i64 > limit;
    tags.truncate(usize::try_from(limit).unwrap_or(0));

    ctx.record(ctx.event(action::TAG_LIST).repository(name).detail("count", tags.len())).await;

    let next = match (more, tags.last()) {
        (true, Some(last_tag)) => {
            let query = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("n", &limit.to_string())
                .append_pair("last", last_tag)
                .finish();
            Some(format!("</v2/{name}/tags/list?{query}>; rel=\"next\""))
        }
        _ => None,
    };
    let mut res = Json(json!({ "name": name, "tags": tags })).into_response();
    if let Some(link) = next {
        res.headers_mut().insert(header::LINK, HeaderValue::from_str(&link).map_err(AppError::internal)?);
    }
    Ok(res)
}
