//! HTTP Basic authentication for `/v2/`: `username = principal name`,
//! `password = token`. Credentials are never logged.

use std::time::{Duration, Instant};

use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts},
};
use base64::Engine;

use super::{tokens, Principal};
use crate::{
    app::AppState,
    audit::{action, AuditEvent, ClientInfo, Outcome},
    db,
    error::AppError,
};

/// The authenticated principal of a registry request.
pub(crate) struct RegistryPrincipal(pub Principal);

/// `last_used_at` is written at most this often per token.
const TOUCH_INTERVAL: Duration = Duration::from_secs(60);

struct Credentials {
    username: String,
    secret: String,
}

fn parse_basic(parts: &Parts) -> Option<Credentials> {
    let value = parts.headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, encoded) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = base64::engine::general_purpose::STANDARD.decode(encoded.trim()).ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (username, secret) = decoded.split_once(':')?;
    Some(Credentials { username: username.to_string(), secret: secret.to_string() })
}

impl FromRequestParts<AppState> for RegistryPrincipal {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let Some(creds) = parse_basic(parts) else {
            // The normal challenge round-trip: not audited.
            return Err(AppError::unauthorized());
        };
        match authenticate(state, &creds).await? {
            Ok(principal) => Ok(RegistryPrincipal(principal)),
            Err(reason) => {
                let Ok(client) = ClientInfo::from_request_parts(parts, state).await;
                let username: String = creds.username.chars().take(128).collect();
                state
                    .audit
                    .record(
                        AuditEvent::new(action::LOGIN)
                            .outcome(Outcome::Denied)
                            .principal_name(username)
                            .client(&client)
                            .detail("reason", reason)
                            .detail("path", parts.uri.path()),
                    )
                    .await;
                Err(AppError::unauthorized())
            }
        }
    }
}

/// `Ok(Err(reason))` for bad credentials, `Err` for internal failures.
async fn authenticate(state: &AppState, creds: &Credentials) -> Result<Result<Principal, &'static str>, AppError> {
    let name = creds.username.to_ascii_lowercase();
    let Some(row) = db::principals::by_name(&state.db.read, &name).await? else {
        return Ok(Err("unknown principal"));
    };
    let Some(principal) = Principal::from_row(&row, &state.cfg) else {
        return Ok(Err(if row.enabled { "github principal is not an admin" } else { "principal disabled" }));
    };
    let presented = tokens::hash(&creds.secret);
    let now = crate::time::now();
    let candidates = db::tokens::active_for(&state.db.read, principal.id, &now).await?;
    // Compare against every candidate without short-circuiting.
    let mut matched = None;
    for t in &candidates {
        if tokens::hashes_equal(&t.hash, &presented) {
            matched = Some(t.id);
        }
    }
    let Some(token_id) = matched else {
        return Ok(Err("invalid, revoked or expired token"));
    };
    touch(state, token_id, now);
    Ok(Ok(principal))
}

/// Records token use, throttled to once per [`TOUCH_INTERVAL`] per token.
fn touch(state: &AppState, token_id: i64, now: String) {
    {
        let mut seen = state.token_touches.lock().unwrap_or_else(|p| p.into_inner());
        let due = seen.get(&token_id).is_none_or(|t| t.elapsed() >= TOUCH_INTERVAL);
        if !due {
            return;
        }
        seen.insert(token_id, Instant::now());
        if seen.len() > 10_000 {
            seen.retain(|_, t| t.elapsed() < TOUCH_INTERVAL);
        }
    }
    let db = state.db.clone();
    tokio::spawn(async move {
        if let Err(e) = db::tokens::touch(&db.write, token_id, &now).await {
            tracing::warn!(error = %e, token_id, "failed to update token last_used_at");
        }
    });
}

#[cfg(test)]
mod tests {
    use axum::http::Request;

    use super::*;

    fn parts(auth: Option<&str>) -> Parts {
        let mut b = Request::builder().uri("/v2/");
        if let Some(a) = auth {
            b = b.header(header::AUTHORIZATION, a);
        }
        b.body(()).unwrap().into_parts().0
    }

    #[test]
    fn parses_basic_credentials() {
        let enc = base64::engine::general_purpose::STANDARD.encode("ci-deploy:mr_abc:def");
        let c = parse_basic(&parts(Some(&format!("Basic {enc}")))).unwrap();
        assert_eq!(c.username, "ci-deploy");
        assert_eq!(c.secret, "mr_abc:def");
        assert!(parse_basic(&parts(Some(&format!("basic {enc}")))).is_some());
        assert!(parse_basic(&parts(Some("Bearer abc"))).is_none());
        assert!(parse_basic(&parts(Some("Basic !!!"))).is_none());
        assert!(parse_basic(&parts(None)).is_none());
    }
}
