//! `GET /api/v1/me`.

use axum::{extract::State, Json};

use super::{dto::*, principals::summary};
use crate::{app::AppState, auth::AdminSession, db, error::AppResult};

#[derive(serde::Serialize, utoipa::ToSchema)]
pub(crate) struct MeResponse {
    pub principal: PrincipalSummary,
}

/// The signed-in admin.
#[utoipa::path(
    get,
    path = "/me",
    tag = "session",
    responses(
        (status = 200, body = MeResponse),
        (status = 401, description = "Not signed in", body = ErrorResponse),
    ),
    security(("session" = []))
)]
pub(crate) async fn get_me(State(state): State<AppState>, admin: AdminSession) -> AppResult<Json<MeResponse>> {
    let row = db::principals::by_id(&state.db.read, admin.principal.id)
        .await?
        .ok_or_else(crate::error::AppError::unauthorized)?;
    let now = crate::time::now();
    let tokens = db::tokens::active_for(&state.db.read, row.id, &now).await?.len() as i64;
    Ok(Json(MeResponse { principal: summary(&state, &row, tokens) }))
}
