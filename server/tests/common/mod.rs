//! In-process test harness: a fake GitHub, a MinRegistry instance on a random
//! port with a temporary SQLite database and filesystem storage, and helpers
//! for the registry (`/v2/`) and management (`/api/v1/`) APIs.

#![allow(dead_code)]

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use axum::{
    extract::{Form, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use base64::Engine;
use reqwest::{redirect::Policy, Method};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

pub const ADMIN: &str = "octo-admin";
pub const OTHER_ADMIN: &str = "second-admin";

pub fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

// --- Fake GitHub ---------------------------------------------------------------

#[derive(Clone, Default)]
struct FakeGithub {
    codes: Arc<Mutex<HashMap<String, String>>>,
    tokens: Arc<Mutex<HashMap<String, String>>>,
}

#[derive(serde::Deserialize)]
struct AuthorizeQuery {
    redirect_uri: String,
    state: String,
    scope: String,
    login: Option<String>,
}

async fn authorize(State(gh): State<FakeGithub>, Query(q): Query<AuthorizeQuery>) -> Response {
    assert_eq!(q.scope, "read:user", "MinRegistry must only ask for read:user");
    let code = uuid::Uuid::new_v4().to_string();
    gh.codes.lock().unwrap().insert(code.clone(), q.login.unwrap_or_else(|| ADMIN.to_string()));
    Redirect::to(&format!("{}?code={code}&state={}", q.redirect_uri, q.state)).into_response()
}

async fn access_token(State(gh): State<FakeGithub>, Form(form): Form<HashMap<String, String>>) -> Json<Value> {
    assert_eq!(form.get("client_secret").map(String::as_str), Some("test-secret"));
    let Some(login) = form.get("code").and_then(|c| gh.codes.lock().unwrap().remove(c)) else {
        return Json(json!({"error": "bad_verification_code"}));
    };
    let token = format!("gho_{}", uuid::Uuid::new_v4().simple());
    gh.tokens.lock().unwrap().insert(token.clone(), login);
    Json(json!({"access_token": token, "token_type": "bearer", "scope": "read:user"}))
}

async fn user(State(gh): State<FakeGithub>, headers: HeaderMap) -> Response {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default()
        .to_string();
    match gh.tokens.lock().unwrap().get(&token) {
        Some(login) => {
            let id = u64::from_be_bytes(Sha256::digest(login.as_bytes())[..8].try_into().unwrap()) >> 16;
            Json(json!({"login": login, "id": id, "name": format!("{login} (GitHub)")})).into_response()
        }
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

async fn spawn(router: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
    });
    addr
}

// --- The server under test ------------------------------------------------------

pub struct TestServer {
    pub url: String,
    pub addr: SocketAddr,
    pub dir: TempDir,
    pub admin: ApiClient,
}

pub struct ApiClient {
    pub http: reqwest::Client,
    pub url: String,
}

impl TestServer {
    pub async fn start() -> TestServer {
        Self::start_with(&[]).await
    }

    pub async fn start_with(extra: &[(&str, &str)]) -> TestServer {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let gh = FakeGithub::default();
        let gh_addr = spawn(
            Router::new()
                .route("/login/oauth/authorize", get(authorize))
                .route("/login/oauth/access_token", post(access_token))
                .route("/user", get(user))
                .with_state(gh),
        )
        .await;

        let dir = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}");
        let p = |name: &str| dir.path().join(name).display().to_string();
        let mut vars: HashMap<String, String> = [
            ("MINREGISTRY_PUBLIC_URL", url.clone()),
            ("MINREGISTRY_DB_PATH", p("db/minregistry.db")),
            ("MINREGISTRY_FS_ROOT", p("blobs")),
            ("MINREGISTRY_UPLOAD_DIR", p("uploads")),
            ("MINREGISTRY_GITHUB_CLIENT_ID", "test-client".into()),
            ("MINREGISTRY_GITHUB_CLIENT_SECRET", "test-secret".into()),
            ("MINREGISTRY_GITHUB_URL", format!("http://{gh_addr}")),
            ("MINREGISTRY_GITHUB_API_URL", format!("http://{gh_addr}")),
            ("MINREGISTRY_ADMIN_GITHUB_LOGINS", format!("{ADMIN},{OTHER_ADMIN}")),
            ("MINREGISTRY_SESSION_SECRET", base64::engine::general_purpose::STANDARD.encode([7u8; 48])),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        for (k, v) in extra {
            vars.insert(k.to_string(), v.to_string());
        }
        let cfg = minregistry::Config::from_lookup(&|k| vars.get(k).cloned()).unwrap();
        let app = minregistry::App::build(cfg).await.unwrap();
        tokio::spawn(async move {
            app.serve(listener, std::future::pending()).await.unwrap();
        });

        let admin = ApiClient::login(&url, ADMIN).await;
        TestServer { url, addr, dir, admin }
    }

    pub fn blob_path(&self, digest: &str) -> PathBuf {
        let hex = digest.trim_start_matches("sha256:");
        self.dir.path().join("blobs/blobs/sha256").join(&hex[..2]).join(hex).join("data")
    }

    /// A registry client authenticating as `user:secret`.
    pub fn registry(&self, user: &str, secret: &str) -> Registry {
        Registry { http: reqwest::Client::new(), url: self.url.clone(), user: Some((user.into(), secret.into())) }
    }

    pub fn anonymous(&self) -> Registry {
        Registry { http: reqwest::Client::new(), url: self.url.clone(), user: None }
    }

    /// Creates an identity and a token; returns (principal id, secret).
    pub async fn identity(&self, name: &str) -> (String, String) {
        let (status, body) = self.admin.call(Method::POST, "/principals", Some(json!({"name": name}))).await;
        assert_eq!(status, 201, "{body}");
        let id = body["id"].as_str().unwrap().to_string();
        (id.clone(), self.token(&id).await)
    }

    pub async fn token(&self, principal_id: &str) -> String {
        let (status, body) = self
            .admin
            .call(Method::POST, &format!("/principals/{principal_id}/tokens"), Some(json!({"name": "test"})))
            .await;
        assert_eq!(status, 201, "{body}");
        body["secret"].as_str().unwrap().to_string()
    }

    /// A registry client for the signed-in admin's own GitHub principal.
    pub async fn admin_registry(&self) -> Registry {
        let (_, me) = self.admin.call(Method::GET, "/me", None).await;
        let id = me["principal"]["id"].as_str().unwrap().to_string();
        let secret = self.token(&id).await;
        self.registry(ADMIN, &secret)
    }

    pub async fn repo_id(&self, name: &str) -> String {
        let (_, body) = self.admin.call(Method::GET, &format!("/repositories?q={name}"), None).await;
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == name)
            .unwrap_or_else(|| panic!("repository {name} not found in {body}"))["id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    pub async fn grant(&self, repo: &str, principal_id: &str, level: &str) {
        let id = self.repo_id(repo).await;
        let (status, body) = self
            .admin
            .call(Method::PUT, &format!("/repositories/{id}/permissions/{principal_id}"), Some(json!({"level": level})))
            .await;
        assert_eq!(status, 200, "{body}");
    }

    /// Audit events matching all given filters (newest first).
    pub async fn audit(&self, filters: &[(&str, &str)]) -> Vec<Value> {
        let query: String = filters
            .iter()
            .map(|(k, v)| format!("{k}={}", url::form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()))
            .collect::<Vec<_>>()
            .join("&");
        let (status, body) = self.admin.call(Method::GET, &format!("/audit?limit=500&{query}"), None).await;
        assert_eq!(status, 200, "{body}");
        body["items"].as_array().unwrap().clone()
    }

    /// Asserts an audit event `(principal, action, repository, reference, outcome)` exists.
    pub async fn assert_audit(
        &self,
        principal: &str,
        action: &str,
        repository: Option<&str>,
        reference: Option<&str>,
        outcome: &str,
    ) {
        let mut filters = vec![("principal", principal), ("action", action), ("outcome", outcome)];
        if let Some(r) = repository {
            filters.push(("repository", r));
        }
        let events = self.audit(&filters).await;
        let found = events.iter().any(|e| reference.is_none_or(|r| e["reference"] == r));
        assert!(
            found,
            "no audit event ({principal}, {action}, {repository:?}, {reference:?}, {outcome}); got {events:#?}"
        );
    }
}

impl ApiClient {
    /// Signs in through the OAuth flow; panics unless it succeeds.
    pub async fn login(url: &str, login: &str) -> ApiClient {
        let (client, status) = Self::try_login(url, login).await;
        assert_eq!(status, 303, "admin login failed");
        client
    }

    /// Runs the OAuth flow; returns the client and the callback's status.
    pub async fn try_login(url: &str, login: &str) -> (ApiClient, u16) {
        let http = reqwest::Client::builder().cookie_store(true).redirect(Policy::none()).build().unwrap();
        let res = http.get(format!("{url}/auth/github/login")).send().await.unwrap();
        assert_eq!(res.status(), 303);
        let authorize = res.headers()[header::LOCATION].to_str().unwrap().to_string();
        let res = http.get(format!("{authorize}&login={login}")).send().await.unwrap();
        let callback = res.headers()[header::LOCATION].to_str().unwrap().to_string();
        let res = http.get(&callback).send().await.unwrap();
        let status = res.status().as_u16();
        (ApiClient { http, url: url.to_string() }, status)
    }

    pub async fn call(&self, method: Method, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut req = self
            .http
            .request(method, format!("{}/api/v1{path}", self.url))
            .header("x-requested-with", "XMLHttpRequest");
        if let Some(b) = body {
            req = req.json(&b);
        }
        let res = req.send().await.unwrap();
        let status = res.status().as_u16();
        let text = res.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }
}

// --- Registry client --------------------------------------------------------------

pub struct Registry {
    pub http: reqwest::Client,
    pub url: String,
    pub user: Option<(String, String)>,
}

impl Registry {
    pub fn req(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        let url = if path.starts_with("http") { path.to_string() } else { format!("{}{path}", self.url) };
        let req = self.http.request(method, url);
        match &self.user {
            Some((u, p)) => req.basic_auth(u, Some(p)),
            None => req,
        }
    }

    pub async fn status(&self, method: Method, path: &str) -> u16 {
        self.req(method, path).send().await.unwrap().status().as_u16()
    }

    /// Monolithic upload (POST + PUT); returns the response of the PUT.
    pub async fn push_blob(&self, name: &str, content: &[u8]) -> reqwest::Response {
        let res = self.req(Method::POST, &format!("/v2/{name}/blobs/uploads/")).send().await.unwrap();
        assert_eq!(res.status(), 202, "start upload: {}", res.text().await.unwrap());
        let location = res.headers()[header::LOCATION].to_str().unwrap().to_string();
        self.req(Method::PUT, &format!("{location}?digest={}", sha256(content)))
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(content.to_vec())
            .send()
            .await
            .unwrap()
    }

    pub async fn push_manifest(&self, name: &str, reference: &str, media_type: &str, body: &[u8]) -> reqwest::Response {
        self.req(Method::PUT, &format!("/v2/{name}/manifests/{reference}"))
            .header(header::CONTENT_TYPE, media_type)
            .body(body.to_vec())
            .send()
            .await
            .unwrap()
    }

    /// Pushes config + one layer + an image manifest tagged `tag`; returns
    /// (manifest digest, manifest bytes, layer digest).
    pub async fn push_image(&self, name: &str, tag: &str, arch: &str) -> (String, Vec<u8>, String) {
        let config = serde_json::to_vec(&json!({"architecture": arch, "os": "linux", "rootfs": {"type": "layers", "diff_ids": []}, "nonce": uuid::Uuid::new_v4().to_string()})).unwrap();
        let layer = format!("layer {name} {tag} {}", uuid::Uuid::new_v4()).into_bytes();
        for blob in [&config, &layer] {
            let res = self.push_blob(name, blob).await;
            assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
        }
        let manifest = image_manifest(&config, &[&layer], None);
        let res = self.push_manifest(name, tag, OCI_MANIFEST, &manifest).await;
        assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
        (sha256(&manifest), manifest, sha256(&layer))
    }
}

pub const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
pub const OCI_INDEX: &str = "application/vnd.oci.image.index.v1+json";

pub fn image_manifest(config: &[u8], layers: &[&[u8]], subject: Option<(&str, usize)>) -> Vec<u8> {
    let mut m = json!({
        "schemaVersion": 2,
        "mediaType": OCI_MANIFEST,
        "config": {"mediaType": "application/vnd.oci.image.config.v1+json", "digest": sha256(config), "size": config.len()},
        "layers": layers.iter().map(|l| json!({"mediaType": "application/vnd.oci.image.layer.v1.tar", "digest": sha256(l), "size": l.len()})).collect::<Vec<_>>(),
    });
    if let Some((digest, size)) = subject {
        m["subject"] = json!({"mediaType": OCI_MANIFEST, "digest": digest, "size": size});
    }
    serde_json::to_vec_pretty(&m).unwrap()
}
