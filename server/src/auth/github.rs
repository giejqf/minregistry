//! GitHub OAuth sign-in for admins (scope `read:user` only). Only logins in
//! `MINREGISTRY_ADMIN_GITHUB_LOGINS` get a session; everyone else gets a 403.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use oauth2::{
    basic::BasicClient, AuthType, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet,
    EndpointSet, RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use serde::Deserialize;
use tower_sessions::Session;

use super::session::{OAUTH_NEXT_KEY, OAUTH_STATE_KEY, PRINCIPAL_KEY};
use crate::{
    app::AppState,
    audit::{action, AuditEvent, ClientInfo, Outcome},
    config::Config,
    db::{self, principals::KIND_GITHUB},
    error::AppError,
};

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/github/login", get(login))
        .route("/auth/github/callback", get(callback))
        .route("/auth/logout", post(logout))
}

type GithubClient = BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

fn oauth_client(cfg: &Config) -> anyhow::Result<GithubClient> {
    let base = cfg.github.base_url.as_str().trim_end_matches('/');
    let callback = cfg.public_url.join("/auth/github/callback")?;
    Ok(BasicClient::new(ClientId::new(cfg.github.client_id.clone()))
        .set_client_secret(ClientSecret::new(cfg.github.client_secret.clone()))
        .set_auth_type(AuthType::RequestBody)
        .set_auth_uri(AuthUrl::new(format!("{base}/login/oauth/authorize"))?)
        .set_token_uri(TokenUrl::new(format!("{base}/login/oauth/access_token"))?)
        .set_redirect_uri(RedirectUrl::new(callback.to_string())?))
}

/// Only same-origin absolute paths are accepted as post-login destinations.
fn safe_next(next: &str) -> bool {
    next.starts_with('/') && !next.starts_with("//") && !next.contains('\\') && !next.starts_with("/auth/")
}

#[derive(Deserialize)]
struct LoginQuery {
    next: Option<String>,
}

async fn login(
    State(state): State<AppState>,
    session: Session,
    Query(q): Query<LoginQuery>,
) -> Result<Redirect, AppError> {
    let client = oauth_client(&state.cfg)?;
    let (url, csrf) = client.authorize_url(CsrfToken::new_random).add_scope(Scope::new("read:user".into())).url();
    session.insert(OAUTH_STATE_KEY, csrf.secret()).await?;
    match q.next.filter(|n| safe_next(n)) {
        Some(next) => session.insert(OAUTH_NEXT_KEY, next).await?,
        None => {
            session.remove::<String>(OAUTH_NEXT_KEY).await?;
        }
    }
    Ok(Redirect::to(url.as_str()))
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct GithubUser {
    login: String,
    id: i64,
    name: Option<String>,
}

fn page(status: StatusCode, title: &str, message: &str) -> Response {
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\">\
         <title>{title} · MinRegistry</title><style>body{{font-family:system-ui,sans-serif;max-width:32rem;margin:4rem auto;\
         padding:0 1rem;line-height:1.5}}</style></head><body><h1>{title}</h1><p>{message}</p>\
         <p><a href=\"/\">Back to MinRegistry</a></p></body></html>"
    );
    (status, Html(html)).into_response()
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

async fn callback(
    State(state): State<AppState>,
    session: Session,
    client: ClientInfo,
    Query(q): Query<CallbackQuery>,
) -> Result<Response, AppError> {
    let expected: Option<String> = session.remove(OAUTH_STATE_KEY).await?;
    if q.error.is_some() {
        return Ok(page(StatusCode::BAD_REQUEST, "Sign-in cancelled", "GitHub did not authorize the sign-in."));
    }
    let (Some(code), Some(returned), Some(expected)) = (q.code, q.state, expected) else {
        return Ok(page(
            StatusCode::BAD_REQUEST,
            "Sign-in failed",
            "The sign-in request is missing or has expired. Please try again.",
        ));
    };
    if !super::tokens::hashes_equal(&returned, &expected) {
        return Ok(page(
            StatusCode::BAD_REQUEST,
            "Sign-in failed",
            "The sign-in state did not match. Please try again.",
        ));
    }

    let user = match fetch_user(&state, code).await {
        Ok(user) => user,
        Err(e) => {
            tracing::warn!(error = %e, "GitHub OAuth exchange failed");
            return Ok(page(StatusCode::BAD_GATEWAY, "Sign-in failed", "Could not complete the sign-in with GitHub."));
        }
    };
    let login = user.login.to_ascii_lowercase();
    let event =
        AuditEvent::new(action::ADMIN_LOGIN).principal_name(&login).client(&client).detail("github_id", user.id);

    if !state.cfg.is_admin_login(&login) || !crate::config::is_valid_github_login(&login) {
        session.flush().await?;
        state.audit.record(event.outcome(Outcome::Denied).detail("reason", "not in admin allowlist")).await;
        return Ok(page(
            StatusCode::FORBIDDEN,
            "Not authorized",
            &format!("The GitHub account <strong>{}</strong> is not a MinRegistry admin.", escape(&user.login)),
        ));
    }

    let display_name = user.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| user.login.clone());
    let principal_id = match upsert_principal(&state, &login, user.id, &display_name).await? {
        Ok(id) => id,
        Err(reason) => {
            session.flush().await?;
            state.audit.record(event.outcome(Outcome::Denied).detail("reason", reason)).await;
            return Ok(page(StatusCode::FORBIDDEN, "Not authorized", &escape(reason)));
        }
    };

    session.cycle_id().await?;
    session.insert(PRINCIPAL_KEY, principal_id).await?;
    let next: Option<String> = session.remove(OAUTH_NEXT_KEY).await?;
    let mut event = event;
    event.principal_id = Some(principal_id);
    state.audit.record(event).await;
    Ok(Redirect::to(next.as_deref().filter(|n| safe_next(n)).unwrap_or("/")).into_response())
}

/// `Ok(Err(reason))` when the account may not sign in.
async fn upsert_principal(
    state: &AppState,
    login: &str,
    github_id: i64,
    display_name: &str,
) -> Result<Result<i64, &'static str>, AppError> {
    let mut tx = state.db.begin_write().await?;
    let id = if let Some(row) = db::principals::by_github_id(&mut *tx, github_id).await? {
        if !row.enabled {
            return Ok(Err("This admin account is disabled."));
        }
        if row.name != login {
            if let Some(other) = db::principals::by_name(&mut *tx, login).await? {
                if other.id != row.id {
                    return Ok(Err("Another principal already uses this name."));
                }
            }
        }
        db::principals::update_github(&mut *tx, row.id, login, display_name).await?;
        row.id
    } else {
        if db::principals::by_name(&mut *tx, login).await?.is_some() {
            return Ok(Err("Another principal already uses this name."));
        }
        db::principals::insert(&mut *tx, KIND_GITHUB, login, Some(github_id), display_name, &crate::time::now()).await?
    };
    tx.commit().await?;
    Ok(Ok(id))
}

async fn fetch_user(state: &AppState, code: String) -> anyhow::Result<GithubUser> {
    let client = oauth_client(&state.cfg)?;
    let http = state.oauth_http.clone();
    let send = move |req: oauth2::HttpRequest| {
        let http = http.clone();
        async move {
            let req = reqwest::Request::try_from(req)?;
            let res = http.execute(req).await?;
            let mut builder = oauth2::http::Response::builder().status(res.status());
            for (k, v) in res.headers() {
                builder = builder.header(k, v);
            }
            let body = res.bytes().await?.to_vec();
            Ok::<_, reqwest::Error>(builder.body(body).unwrap_or_default())
        }
    };
    let token = client.exchange_code(AuthorizationCode::new(code)).request_async(&send).await?;
    let api = state.cfg.github.api_url.as_str().trim_end_matches('/');
    let user = state
        .oauth_http
        .get(format!("{api}/user"))
        .bearer_auth(token.access_token().secret())
        .header("accept", "application/vnd.github+json")
        .header("x-github-api-version", "2022-11-28")
        .send()
        .await?
        .error_for_status()?
        .json::<GithubUser>()
        .await?;
    Ok(user)
}

async fn logout(State(state): State<AppState>, session: Session, client: ClientInfo) -> Result<StatusCode, AppError> {
    if let Some(id) = session.get::<i64>(PRINCIPAL_KEY).await? {
        let mut event = AuditEvent::new(action::ADMIN_LOGOUT).client(&client);
        if let Some(row) = db::principals::by_id(&state.db.read, id).await? {
            event = event.principal_name(row.name);
        }
        event.principal_id = Some(id);
        state.audit.record(event).await;
    }
    session.flush().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_must_be_a_local_path() {
        assert!(safe_next("/repositories/3"));
        assert!(!safe_next("//evil.example"));
        assert!(!safe_next("https://evil.example"));
        assert!(!safe_next("/\\evil.example"));
        assert!(!safe_next("/auth/logout"));
    }
}
