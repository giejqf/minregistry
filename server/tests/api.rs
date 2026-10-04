//! `/api/v1/`, OAuth sign-in and garbage collection.

mod common;

use common::*;
use reqwest::{header, Method};
use serde_json::{json, Value};

#[tokio::test]
async fn oauth_sign_in_and_sessions() {
    let srv = TestServer::start().await;

    // No session: 401 JSON (no Basic challenge).
    let anon = reqwest::Client::new();
    let res = anon.get(format!("{}/api/v1/me", srv.url)).send().await.unwrap();
    assert_eq!(res.status(), 401);
    assert!(res.headers().get(header::WWW_AUTHENTICATE).is_none());
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["error"]["code"], "unauthenticated");

    // Basic auth is never accepted on the management API.
    let (_, secret) = srv.identity("alice").await;
    let res = anon.get(format!("{}/api/v1/me", srv.url)).basic_auth("alice", Some(&secret)).send().await.unwrap();
    assert_eq!(res.status(), 401);

    // Not in the allowlist: 403 page, no session.
    let (stranger, status) = ApiClient::try_login(&srv.url, "mallory").await;
    assert_eq!(status, 403);
    assert_eq!(stranger.call(Method::GET, "/me", None).await.0, 401);
    srv.assert_audit("mallory", "admin.login", None, None, "denied").await;

    // Admin: session works and the principal is a GitHub admin.
    let (status, me) = srv.admin.call(Method::GET, "/me", None).await;
    assert_eq!(status, 200);
    assert_eq!(me["principal"]["name"], ADMIN);
    assert_eq!(me["principal"]["kind"], "github");
    assert_eq!(me["principal"]["is_admin"], true);
    srv.assert_audit(ADMIN, "admin.login", None, None, "ok").await;

    // Forged state is rejected.
    let res = anon.get(format!("{}/auth/github/callback?code=x&state=forged", srv.url)).send().await.unwrap();
    assert_eq!(res.status(), 400);

    // CSRF: mutating calls need X-Requested-With.
    let res = srv
        .admin
        .http
        .post(format!("{}/api/v1/principals", srv.url))
        .json(&json!({"name": "csrf"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);
    assert_eq!(res.json::<Value>().await.unwrap()["error"]["code"], "csrf");

    // Logout ends the session.
    let other = ApiClient::login(&srv.url, OTHER_ADMIN).await;
    let res = other
        .http
        .post(format!("{}/auth/logout", srv.url))
        .header("x-requested-with", "XMLHttpRequest")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
    assert_eq!(other.call(Method::GET, "/me", None).await.0, 401);
    srv.assert_audit(OTHER_ADMIN, "admin.logout", None, None, "ok").await;

    // The OpenAPI document is public.
    let doc: Value = anon.get(format!("{}/api/v1/openapi.json", srv.url)).send().await.unwrap().json().await.unwrap();
    assert!(doc["paths"]["/api/v1/repositories"].is_object());
    assert_eq!(doc, serde_json::from_str::<Value>(&minregistry::openapi_json()).unwrap());
}

#[tokio::test]
async fn principals_and_tokens() {
    let srv = TestServer::start().await;
    let (status, body) =
        srv.admin.call(Method::POST, "/principals", Some(json!({"name": "ci-deploy", "display_name": "CI"}))).await;
    assert_eq!(status, 201);
    let id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["kind"], "identity");
    assert_eq!(body["display_name"], "CI");

    for (name, want) in [("ci-deploy", 409), ("Bad Name", 400), (ADMIN, 409), ("-x", 400)] {
        let (status, _) = srv.admin.call(Method::POST, "/principals", Some(json!({"name": name}))).await;
        assert_eq!(status, want, "{name}");
    }

    // Token: secret only once, hash never.
    let (status, created) = srv
        .admin
        .call(
            Method::POST,
            &format!("/principals/{id}/tokens"),
            Some(json!({"name": "deploy", "expires_at": "2999-01-01T00:00:00Z"})),
        )
        .await;
    assert_eq!(status, 201);
    let secret = created["secret"].as_str().unwrap().to_string();
    assert_eq!(created["username"], "ci-deploy");
    assert_eq!(created["token"]["prefix"], &secret[..8]);
    assert_eq!(created["token"]["expires_at"], "2999-01-01T00:00:00.000Z");
    let (_, tokens) = srv.admin.call(Method::GET, &format!("/principals/{id}/tokens"), None).await;
    assert!(!tokens.to_string().contains(&secret));
    assert_eq!(tokens[0]["status"], "active");
    assert_eq!(srv.registry("ci-deploy", &secret).status(Method::GET, "/v2/").await, 200);

    let (status, _) = srv
        .admin
        .call(
            Method::POST,
            &format!("/principals/{id}/tokens"),
            Some(json!({"name": "past", "expires_at": "2001-01-01T00:00:00Z"})),
        )
        .await;
    assert_eq!(status, 400);

    // Admins mint tokens for their own GitHub principal, not for other admins.
    let other = ApiClient::login(&srv.url, OTHER_ADMIN).await;
    let (_, other_me) = other.call(Method::GET, "/me", None).await;
    let other_id = other_me["principal"]["id"].as_str().unwrap();
    let (status, _) =
        srv.admin.call(Method::POST, &format!("/principals/{other_id}/tokens"), Some(json!({"name": "x"}))).await;
    assert_eq!(status, 403);
    let (status, _) =
        other.call(Method::POST, &format!("/principals/{other_id}/tokens"), Some(json!({"name": "mine"}))).await;
    assert_eq!(status, 201);
    // GitHub admins are read-only here.
    let (status, _) =
        srv.admin.call(Method::PATCH, &format!("/principals/{other_id}"), Some(json!({"enabled": false}))).await;
    assert_eq!(status, 400);

    let (_, list) = srv.admin.call(Method::GET, "/principals", None).await;
    assert_eq!(list["admin_logins"], json!([ADMIN, OTHER_ADMIN]));
    let names: Vec<&str> = list["principals"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"ci-deploy") && names.contains(&ADMIN));

    srv.assert_audit(ADMIN, "principal.create", None, None, "ok").await;
    srv.assert_audit(ADMIN, "token.create", None, None, "ok").await;
}

#[tokio::test]
async fn repositories_permissions_and_audit() {
    let srv = TestServer::start().await;
    let reg = srv.admin_registry().await;
    let (digest, _, _) = reg.push_image("acme/web", "v1", "amd64").await;
    reg.push_image("acme/api", "v1", "arm64").await;

    let (_, list) = srv.admin.call(Method::GET, "/repositories?q=acme", None).await;
    assert_eq!(list["total"], 2);
    assert_eq!(list["items"][0]["name"], "acme/api");
    assert_eq!(list["items"][1]["tag_count"], 1);
    let (_, filtered) = srv.admin.call(Method::GET, "/repositories?q=web", None).await;
    assert_eq!(filtered["total"], 1);

    let web = srv.repo_id("acme/web").await;
    let (_, detail) = srv.admin.call(Method::GET, &format!("/repositories/{web}"), None).await;
    assert_eq!(detail["tags"][0]["name"], "v1");
    assert_eq!(detail["manifests"][0]["digest"], digest.as_str());
    assert_eq!(detail["manifests"][0]["platforms"][0]["architecture"], "amd64");
    assert_eq!(detail["created_by"], ADMIN);

    // Grants.
    let (alice_id, alice) = srv.identity("alice").await;
    let (status, perm) = srv
        .admin
        .call(Method::PUT, &format!("/repositories/{web}/permissions/{alice_id}"), Some(json!({"level": "read"})))
        .await;
    assert_eq!(status, 200);
    assert_eq!(perm["level"], "read");
    assert_eq!(srv.registry("alice", &alice).status(Method::GET, "/v2/acme/web/manifests/v1").await, 200);
    let (_, principal) = srv.admin.call(Method::GET, &format!("/principals/{alice_id}"), None).await;
    assert_eq!(principal["permissions"][0]["repository"]["name"], "acme/web");
    let (status, _) =
        srv.admin.call(Method::DELETE, &format!("/repositories/{web}/permissions/{alice_id}"), None).await;
    assert_eq!(status, 204);
    assert_eq!(srv.registry("alice", &alice).status(Method::GET, "/v2/acme/web/manifests/v1").await, 403);
    srv.assert_audit(ADMIN, "permission.grant", Some("acme/web"), None, "ok").await;
    srv.assert_audit(ADMIN, "permission.revoke", Some("acme/web"), None, "ok").await;

    // Tag and manifest deletes through the API.
    let (status, _) = srv.admin.call(Method::DELETE, &format!("/repositories/{web}/tags/v1"), None).await;
    assert_eq!(status, 204);
    let (status, _) = srv.admin.call(Method::DELETE, &format!("/repositories/{web}/tags/v1"), None).await;
    assert_eq!(status, 404);
    let (status, _) = srv.admin.call(Method::DELETE, &format!("/repositories/{web}/manifests/{digest}"), None).await;
    assert_eq!(status, 204);

    // Repository delete; pushing again revives it with a new owner.
    let api = srv.repo_id("acme/api").await;
    let (status, _) = srv.admin.call(Method::DELETE, &format!("/repositories/{api}"), None).await;
    assert_eq!(status, 204);
    assert_eq!(srv.admin.call(Method::GET, &format!("/repositories/{api}"), None).await.0, 404);
    assert_eq!(reg.status(Method::GET, "/v2/acme/api/tags/list").await, 404);
    srv.registry("alice", &alice).push_image("acme/api", "v2", "amd64").await;
    let (_, revived) = srv.admin.call(Method::GET, &format!("/repositories/{api}"), None).await;
    assert_eq!(revived["created_by"], "alice");
    assert_eq!(revived["tags"].as_array().unwrap().len(), 1);
    srv.assert_audit(ADMIN, "repository.delete", Some("acme/api"), None, "ok").await;

    // Audit: filters and cursor pagination.
    let page1 = srv.admin.call(Method::GET, "/audit?limit=3", None).await.1;
    assert_eq!(page1["items"].as_array().unwrap().len(), 3);
    let cursor = page1["next_cursor"].as_str().unwrap();
    let page2 = srv.admin.call(Method::GET, &format!("/audit?limit=3&cursor={cursor}"), None).await.1;
    let first_ids: Vec<u64> =
        page1["items"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap().parse().unwrap()).collect();
    let second_ids: Vec<u64> =
        page2["items"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap().parse().unwrap()).collect();
    assert!(first_ids.iter().min() > second_ids.iter().max());
    let pushes = srv.audit(&[("action", "manifest.push"), ("repository", "acme/web")]).await;
    assert!(pushes.iter().all(|e| e["action"] == "manifest.push" && e["repository"] == "acme/web"));
    assert!(srv.audit(&[("from", "2999-01-01T00:00:00Z")]).await.is_empty());
    assert_eq!(srv.admin.call(Method::GET, "/audit?from=yesterday", None).await.0, 400);
    let (_, actions) = srv.admin.call(Method::GET, "/audit/actions", None).await;
    assert!(actions["actions"].as_array().unwrap().iter().any(|a| a == "blob.mount"));
    let event = &srv.audit(&[("action", "manifest.push")]).await[0];
    assert_eq!(event["client_ip"], "127.0.0.1");
    assert!(event["detail"].is_object());
}

#[tokio::test]
async fn garbage_collection() {
    let srv = TestServer::start().await;
    let reg = srv.admin_registry().await;
    let (old_digest, _, old_layer) = reg.push_image("gc/app", "old", "amd64").await;
    let (keep_digest, _, keep_layer) = reg.push_image("gc/app", "keep", "amd64").await;
    // A blob that no manifest references.
    let dangling = b"dangling".to_vec();
    reg.push_blob("gc/app", &dangling).await;

    let gc = |body: Value| async { srv.admin.call(Method::POST, "/gc", Some(body)).await };

    // Young content is protected by the default min age.
    let (status, report) = gc(json!({"dry_run": true, "delete_untagged": true})).await;
    assert_eq!(status, 200);
    assert_eq!(report["blobs_deleted"], 0);
    assert!(report["blobs_kept_young"].as_i64().unwrap() >= 1);

    // Untagged manifests are kept unless asked.
    assert_eq!(reg.status(Method::DELETE, "/v2/gc/app/manifests/old").await, 202);
    let (_, report) = gc(json!({"dry_run": true, "min_age_seconds": 0})).await;
    assert_eq!(report["manifests_deleted"], 0);
    assert_eq!(report["blobs_deleted"], 1, "only the dangling blob: {report}");

    let (_, dry) = gc(json!({"dry_run": true, "delete_untagged": true, "min_age_seconds": 0})).await;
    assert_eq!(dry["manifests_deleted"], 1);
    assert!(srv.blob_path(&old_layer).exists(), "dry run deletes nothing");

    let (status, report) = gc(json!({"delete_untagged": true, "min_age_seconds": 0})).await;
    assert_eq!(status, 200);
    assert_eq!(report["manifests_deleted"], 1);
    // old manifest blob + old config + old layer + dangling
    assert_eq!(report["blobs_deleted"], 4, "{report}");
    assert!(!srv.blob_path(&old_layer).exists());
    assert!(!srv.blob_path(&old_digest).exists());
    assert!(!srv.blob_path(&sha256(&dangling)).exists());
    assert!(srv.blob_path(&keep_layer).exists());
    assert!(srv.blob_path(&keep_digest).exists());

    // The remaining image still pulls; the deleted one does not.
    assert_eq!(reg.status(Method::GET, "/v2/gc/app/manifests/keep").await, 200);
    assert_eq!(reg.status(Method::GET, &format!("/v2/gc/app/blobs/{keep_layer}")).await, 200);
    assert_eq!(reg.status(Method::GET, &format!("/v2/gc/app/blobs/{old_layer}")).await, 404);

    // Idempotent; pushing deleted content again works.
    let (_, again) = gc(json!({"delete_untagged": true, "min_age_seconds": 0})).await;
    assert_eq!(again["blobs_deleted"], 0);
    assert_eq!(reg.push_blob("gc/app", &dangling).await.status(), 201);
    assert!(srv.blob_path(&sha256(&dangling)).exists());

    // Orphaned storage objects (no database record) are removed.
    let orphan = srv.blob_path(&sha256(b"orphan"));
    std::fs::create_dir_all(orphan.parent().unwrap()).unwrap();
    std::fs::write(&orphan, b"orphan").unwrap();
    let (_, report) = gc(json!({"min_age_seconds": 0})).await;
    assert_eq!(report["orphans_deleted"], 1);
    assert!(!orphan.exists());

    srv.assert_audit(ADMIN, "gc.run", None, None, "ok").await;
    let (_, system) = srv.admin.call(Method::GET, "/system", None).await;
    assert_eq!(system["storage"]["backend"], "fs");
    // keep: manifest, config and layer; the re-pushed dangling blob went with the last run.
    assert_eq!(system["blob_count"], 3);
}

/// `/readyz` needs no credentials: it says which dependency is unavailable,
/// never why (an S3 error names the endpoint and the bucket).
#[tokio::test]
async fn readiness_does_not_disclose_errors() {
    let srv = TestServer::start().await;
    let readyz = || async {
        let res = reqwest::get(format!("{}/readyz", srv.url)).await.unwrap();
        (res.status().as_u16(), res.json::<Value>().await.unwrap())
    };
    assert_eq!(readyz().await, (200, json!({ "database": "ok", "storage": "ok" })));

    let blobs = srv.dir.path().join("blobs/blobs");
    std::fs::remove_dir_all(&blobs).unwrap();
    assert_eq!(readyz().await, (503, json!({ "database": "ok", "storage": "unavailable" })));

    std::fs::create_dir_all(&blobs).unwrap();
    assert_eq!(readyz().await, (200, json!({ "database": "ok", "storage": "ok" })));
}
