//! Audit events: who did what to which repository/reference, from where.
//! The table and the action vocabulary (docs/audit.md) are a public contract;
//! change them only with an ADR.

use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{header, request::Parts},
};
use serde_json::Value;

use crate::{app::AppState, auth::Principal, db, db::Db};

/// Action names recorded in `audit_events.action`.
pub(crate) mod action {
    // Registry API (/v2/)
    pub(crate) const LOGIN: &str = "login";
    pub(crate) const REPOSITORY_CREATE: &str = "repository.create";
    pub(crate) const BLOB_UPLOAD: &str = "blob.upload";
    pub(crate) const BLOB_MOUNT: &str = "blob.mount";
    pub(crate) const BLOB_PULL: &str = "blob.pull";
    pub(crate) const BLOB_DELETE: &str = "blob.delete";
    pub(crate) const UPLOAD_CANCEL: &str = "upload.cancel";
    pub(crate) const MANIFEST_PUSH: &str = "manifest.push";
    pub(crate) const MANIFEST_PULL: &str = "manifest.pull";
    pub(crate) const MANIFEST_DELETE: &str = "manifest.delete";
    pub(crate) const TAG_DELETE: &str = "tag.delete";
    pub(crate) const TAG_LIST: &str = "tag.list";
    pub(crate) const REFERRERS_LIST: &str = "referrers.list";
    // Management (/api/v1/, /auth/) and maintenance
    pub(crate) const ADMIN_LOGIN: &str = "admin.login";
    pub(crate) const ADMIN_LOGOUT: &str = "admin.logout";
    pub(crate) const PRINCIPAL_CREATE: &str = "principal.create";
    pub(crate) const PRINCIPAL_UPDATE: &str = "principal.update";
    pub(crate) const TOKEN_CREATE: &str = "token.create";
    pub(crate) const TOKEN_REVOKE: &str = "token.revoke";
    pub(crate) const PERMISSION_GRANT: &str = "permission.grant";
    pub(crate) const PERMISSION_REVOKE: &str = "permission.revoke";
    pub(crate) const REPOSITORY_DELETE: &str = "repository.delete";
    pub(crate) const GC_RUN: &str = "gc.run";
    pub(crate) const AUDIT_PRUNE: &str = "audit.prune";

    /// Every action, for documentation and the UI filter.
    pub(crate) const ALL: &[&str] = &[
        LOGIN,
        REPOSITORY_CREATE,
        BLOB_UPLOAD,
        BLOB_MOUNT,
        BLOB_PULL,
        BLOB_DELETE,
        UPLOAD_CANCEL,
        MANIFEST_PUSH,
        MANIFEST_PULL,
        MANIFEST_DELETE,
        TAG_DELETE,
        TAG_LIST,
        REFERRERS_LIST,
        ADMIN_LOGIN,
        ADMIN_LOGOUT,
        PRINCIPAL_CREATE,
        PRINCIPAL_UPDATE,
        TOKEN_CREATE,
        TOKEN_REVOKE,
        PERMISSION_GRANT,
        PERMISSION_REVOKE,
        REPOSITORY_DELETE,
        GC_RUN,
        AUDIT_PRUNE,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Ok,
    Denied,
    Error,
}

impl Outcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Denied => "denied",
            Outcome::Error => "error",
        }
    }
}

/// Where a request came from.
#[derive(Clone, Debug, Default)]
pub(crate) struct ClientInfo {
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

impl FromRequestParts<AppState> for ClientInfo {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let peer = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
        let forwarded = if state.cfg.trust_proxy {
            // The proxy appends the address it saw; the right-most entry is
            // the only one a client cannot forge.
            parts
                .headers
                .get_all("x-forwarded-for")
                .iter()
                .filter_map(|v| v.to_str().ok())
                .flat_map(|v| v.split(','))
                .next_back()
                .and_then(|s| s.trim().parse::<IpAddr>().ok())
        } else {
            None
        };
        let user_agent =
            parts.headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).map(|s| s.chars().take(512).collect());
        Ok(ClientInfo { ip: forwarded.or(peer).map(|ip| ip.to_string()), user_agent })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct AuditEvent {
    pub action: &'static str,
    pub outcome: Outcome,
    pub principal_id: Option<i64>,
    pub principal_name: Option<String>,
    pub repository: Option<String>,
    pub reference: Option<String>,
    pub digest: Option<String>,
    pub client: ClientInfo,
    pub detail: Value,
}

impl AuditEvent {
    pub(crate) fn new(action: &'static str) -> Self {
        AuditEvent {
            action,
            outcome: Outcome::Ok,
            principal_id: None,
            principal_name: None,
            repository: None,
            reference: None,
            digest: None,
            client: ClientInfo::default(),
            detail: Value::Object(Default::default()),
        }
    }

    pub(crate) fn principal(mut self, p: &Principal) -> Self {
        self.principal_id = Some(p.id);
        self.principal_name = Some(p.name.clone());
        self
    }

    pub(crate) fn principal_name(mut self, name: impl Into<String>) -> Self {
        self.principal_name = Some(name.into());
        self
    }

    pub(crate) fn outcome(mut self, outcome: Outcome) -> Self {
        self.outcome = outcome;
        self
    }

    pub(crate) fn repository(mut self, name: impl Into<String>) -> Self {
        self.repository = Some(name.into());
        self
    }

    pub(crate) fn reference(mut self, reference: impl Into<String>) -> Self {
        self.reference = Some(reference.into());
        self
    }

    pub(crate) fn digest(mut self, digest: impl ToString) -> Self {
        self.digest = Some(digest.to_string());
        self
    }

    pub(crate) fn client(mut self, client: &ClientInfo) -> Self {
        self.client = client.clone();
        self
    }

    /// Adds `key: value` to the JSON detail object.
    pub(crate) fn detail(mut self, key: &str, value: impl Into<Value>) -> Self {
        if let Value::Object(map) = &mut self.detail {
            map.insert(key.to_string(), value.into());
        }
        self
    }
}

/// Appends audit events. Failures are logged, never surfaced to clients.
#[derive(Clone)]
pub(crate) struct AuditLog {
    db: Db,
}

impl AuditLog {
    pub(crate) fn new(db: Db) -> Self {
        AuditLog { db }
    }

    pub(crate) async fn record(&self, e: AuditEvent) {
        let detail = e.detail.to_string();
        let ts = crate::time::now();
        let row = db::audit::NewAuditEvent {
            ts: &ts,
            principal_id: e.principal_id,
            principal_name: e.principal_name.as_deref(),
            action: e.action,
            repository: e.repository.as_deref(),
            reference: e.reference.as_deref(),
            digest: e.digest.as_deref(),
            client_ip: e.client.ip.as_deref(),
            user_agent: e.client.user_agent.as_deref(),
            outcome: e.outcome.as_str(),
            detail: &detail,
        };
        if let Err(err) = db::audit::insert(&self.db.write, &row).await {
            tracing::error!(error = %err, action = e.action, "failed to write audit event");
        }
    }
}
