//! `GET /v2/<name>/tags/list[?n=&last=]` — lexically sorted, paginated.

use axum::{
    http::{header, HeaderValue},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

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
    let (limit, last) = page(ctx)?;
    // Fetch one extra row to learn whether another page follows.
    let rows =
        if limit == 0 { Vec::new() } else { db::tags::names(&ctx.state.db.read, repo.id, last, limit + 1).await? };
    let (tags, next) = paginate(&format!("/v2/{name}/tags/list"), rows, limit);

    ctx.record(ctx.event(action::TAG_LIST).repository(name).detail("count", tags.len())).await;
    page_response(json!({ "name": name, "tags": tags }), next)
}

/// The page `?n=&last=` asks for: how many rows at most, and the name to
/// continue after.
pub(super) fn page(ctx: &Ctx) -> AppResult<(i64, Option<&str>)> {
    let n = match ctx.query("n") {
        None | Some("") => None,
        Some(raw) => Some(raw.parse::<i64>().ok().filter(|n| *n >= 0).ok_or_else(|| {
            AppError::bad_request(OciCode::PaginationNumberInvalid, "invalid number of results requested")
        })?),
    };
    Ok((n.unwrap_or(MAX_PAGE).min(MAX_PAGE), ctx.query("last").filter(|l| !l.is_empty())))
}

/// A JSON page with the `Link` header of the next one, if any.
pub(super) fn page_response(body: Value, next: Option<String>) -> AppResult<Response> {
    let mut res = Json(body).into_response();
    if let Some(link) = next {
        res.headers_mut().insert(header::LINK, HeaderValue::from_str(&link).map_err(AppError::internal)?);
    }
    Ok(res)
}

/// Cuts `rows` (fetched with `limit + 1`) to one page and builds the `Link`
/// header of the next page of `path`, if any.
pub(crate) fn paginate(path: &str, mut rows: Vec<String>, limit: i64) -> (Vec<String>, Option<String>) {
    let more = rows.len() as i64 > limit;
    rows.truncate(usize::try_from(limit).unwrap_or(0));
    let next = match (more, rows.last()) {
        (true, Some(last)) => {
            let query = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("n", &limit.to_string())
                .append_pair("last", last)
                .finish();
            Some(format!("<{path}?{query}>; rel=\"next\""))
        }
        _ => None,
    };
    (rows, next)
}

#[cfg(test)]
mod tests {
    use super::paginate;

    fn tags(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn last_page_has_no_link() {
        let (page, next) = paginate("/v2/a/b/tags/list", tags(&["t1", "t2"]), 2);
        assert_eq!(page, tags(&["t1", "t2"]));
        assert_eq!(next, None);
    }

    #[test]
    fn more_rows_produce_a_link() {
        let (page, next) = paginate("/v2/a/b/tags/list", tags(&["t1", "t2", "t3"]), 2);
        assert_eq!(page, tags(&["t1", "t2"]));
        assert_eq!(next.as_deref(), Some("</v2/a/b/tags/list?n=2&last=t2>; rel=\"next\""));
    }

    #[test]
    fn zero_and_encoding() {
        assert_eq!(paginate("/v2/a/tags/list", Vec::new(), 0), (Vec::new(), None));
        let (_, next) = paginate("/v2/a/tags/list", tags(&["v1.0-rc_1", "z"]), 1);
        assert_eq!(next.as_deref(), Some("</v2/a/tags/list?n=1&last=v1.0-rc_1>; rel=\"next\""));
    }
}
