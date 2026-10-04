//! The audit log (read-only).

use axum::{
    extract::{Query, State},
    Json,
};

use super::{dto::*, validation};
use crate::{
    app::AppState,
    audit::action,
    auth::AdminSession,
    db::{self, audit::AuditFilter},
    error::AppResult,
};

/// Audit events, newest first, with filters and cursor pagination.
#[utoipa::path(
    get,
    path = "/audit",
    tag = "audit",
    params(AuditQuery),
    responses((status = 200, body = AuditListResponse), (status = 400, body = ErrorResponse)),
    security(("session" = []))
)]
pub(crate) async fn list_audit_events(
    State(state): State<AppState>,
    _admin: AdminSession,
    Query(q): Query<AuditQuery>,
) -> AppResult<Json<AuditListResponse>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let ts = |v: Option<&String>, field: &str| -> AppResult<Option<String>> {
        match v.map(|s| s.trim()).filter(|s| !s.is_empty()) {
            None => Ok(None),
            Some(raw) => crate::time::normalize(raw)
                .map(Some)
                .ok_or_else(|| validation(format!("{field} must be an RFC 3339 timestamp"))),
        }
    };
    let from = ts(q.from.as_ref(), "from")?;
    let to = ts(q.to.as_ref(), "to")?;
    let before_id = match q.cursor.as_deref().filter(|c| !c.is_empty()) {
        None => None,
        Some(c) => Some(c.parse::<i64>().map_err(|_| validation("invalid cursor"))?),
    };
    let outcome = q.outcome.map(|o| match o {
        AuditOutcome::Ok => "ok",
        AuditOutcome::Denied => "denied",
        AuditOutcome::Error => "error",
    });
    let nonempty = |v: &Option<String>| v.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let (principal, repository, action) = (nonempty(&q.principal), nonempty(&q.repository), nonempty(&q.action));
    let filter = AuditFilter {
        principal: principal.as_deref(),
        repository: repository.as_deref(),
        action: action.as_deref(),
        outcome,
        from: from.as_deref(),
        to: to.as_deref(),
        before_id,
    };
    let rows = db::audit::list(&state.db.read, &filter, limit).await?;
    let next_cursor = (rows.len() as i64 == limit).then(|| rows.last().map(|r| r.id.to_string())).flatten();
    let items = rows
        .into_iter()
        .map(|r| AuditEventSummary {
            id: r.id.to_string(),
            ts: r.ts,
            principal_id: r.principal_id.map(|i| i.to_string()),
            principal_name: r.principal_name,
            action: r.action,
            repository: r.repository,
            reference: r.reference,
            digest: r.digest,
            client_ip: r.client_ip,
            user_agent: r.user_agent,
            outcome: match r.outcome.as_str() {
                "denied" => AuditOutcome::Denied,
                "error" => AuditOutcome::Error,
                _ => AuditOutcome::Ok,
            },
            detail: serde_json::from_str(&r.detail).unwrap_or(serde_json::Value::Null),
        })
        .collect();
    Ok(Json(AuditListResponse { items, next_cursor }))
}

/// Every action name the audit log can contain.
#[utoipa::path(
    get,
    path = "/audit/actions",
    tag = "audit",
    responses((status = 200, body = AuditActionsResponse)),
    security(("session" = []))
)]
pub(crate) async fn list_audit_actions(_admin: AdminSession) -> Json<AuditActionsResponse> {
    Json(AuditActionsResponse { actions: action::ALL.iter().map(|a| a.to_string()).collect() })
}
