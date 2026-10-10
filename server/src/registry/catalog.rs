//! `GET /v2/_catalog[?n=&last=]` — the repository list of the Docker registry
//! API. It is not part of the OCI spec, but registry browsers such as
//! Synology's Container Manager need it. It lists only the repositories the
//! caller may pull from (docs/adr/0010).

use axum::response::Response;
use serde_json::json;

use super::{tags, Ctx};
use crate::{audit::action, db, error::AppResult};

pub(super) async fn list(ctx: &Ctx) -> AppResult<Response> {
    let (limit, last) = tags::page(ctx)?;
    let rows = if limit == 0 {
        Vec::new()
    } else {
        let principal = &ctx.principal;
        db::repositories::readable_names(&ctx.state.db.read, principal.id, principal.is_admin, last, limit + 1).await?
    };
    let (repositories, next) = tags::paginate("/v2/_catalog", rows, limit);

    ctx.record(ctx.event(action::CATALOG_LIST).detail("count", repositories.len())).await;
    tags::page_response(json!({ "repositories": repositories }), next)
}
