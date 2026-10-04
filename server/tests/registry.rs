//! `/v2/` behaviour against an in-process server, with audit rows asserted.

mod common;

use common::*;
use reqwest::{header, Method};
use serde_json::{json, Value};

#[tokio::test]
async fn version_check_requires_credentials() {
    let srv = TestServer::start().await;
    let res = srv.anonymous().req(Method::GET, "/v2/").send().await.unwrap();
    assert_eq!(res.status(), 401);
    assert_eq!(res.headers()[header::WWW_AUTHENTICATE], "Basic realm=\"minregistry\"");
    assert_eq!(res.headers()["docker-distribution-api-version"], "registry/2.0");
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["errors"][0]["code"], "UNAUTHORIZED");

    let (_, secret) = srv.identity("alice").await;
    assert_eq!(srv.registry("alice", &secret).status(Method::GET, "/v2/").await, 200);
    assert_eq!(srv.registry("ALICE", &secret).status(Method::GET, "/v2/").await, 200, "usernames are case-insensitive");
    assert_eq!(srv.registry("alice", "mr_wrong").status(Method::GET, "/v2/").await, 401);
    assert_eq!(srv.registry("nobody", &secret).status(Method::GET, "/v2/").await, 401);
    // Every other endpoint needs credentials too.
    assert_eq!(srv.anonymous().status(Method::GET, "/v2/x/tags/list").await, 401);
    srv.assert_audit("alice", "login", None, None, "ok").await;
    srv.assert_audit("alice", "login", None, None, "denied").await;
    srv.assert_audit("nobody", "login", None, None, "denied").await;
}

#[tokio::test]
async fn push_pull_and_auto_create() {
    let srv = TestServer::start().await;
    let (alice_id, secret) = srv.identity("alice").await;
    let reg = srv.registry("alice", &secret);

    let (digest, manifest, layer) = reg.push_image("team/app", "v1", "amd64").await;
    srv.assert_audit("alice", "repository.create", Some("team/app"), None, "ok").await;
    srv.assert_audit("alice", "blob.upload", Some("team/app"), None, "ok").await;
    srv.assert_audit("alice", "manifest.push", Some("team/app"), Some("v1"), "ok").await;

    // The pusher became owner.
    let repo = srv.repo_id("team/app").await;
    let (_, perms) = srv.admin.call(Method::GET, &format!("/repositories/{repo}/permissions"), None).await;
    assert_eq!(perms[0]["principal"]["id"], alice_id.as_str());
    assert_eq!(perms[0]["level"], "owner");

    for reference in ["v1", digest.as_str()] {
        let res = reg.req(Method::GET, &format!("/v2/team/app/manifests/{reference}")).send().await.unwrap();
        assert_eq!(res.status(), 200);
        assert_eq!(res.headers()[header::CONTENT_TYPE], OCI_MANIFEST);
        assert_eq!(res.headers()["docker-content-digest"], digest.as_str());
        assert_eq!(res.bytes().await.unwrap().as_ref(), manifest.as_slice(), "exact bytes are served");
        let head = reg.req(Method::HEAD, &format!("/v2/team/app/manifests/{reference}")).send().await.unwrap();
        assert_eq!(head.status(), 200);
        assert_eq!(head.headers()[header::CONTENT_LENGTH], manifest.len().to_string().as_str());
        assert_eq!(head.headers()["docker-content-digest"], digest.as_str());
    }
    srv.assert_audit("alice", "manifest.pull", Some("team/app"), Some("v1"), "ok").await;
    srv.assert_audit("alice", "manifest.pull", Some("team/app"), Some(&digest), "ok").await;

    let blob = reg.req(Method::GET, &format!("/v2/team/app/blobs/{layer}")).send().await.unwrap();
    assert_eq!(blob.status(), 200);
    assert_eq!(blob.headers()["docker-content-digest"], layer.as_str());
    assert!(String::from_utf8(blob.bytes().await.unwrap().to_vec()).unwrap().starts_with("layer team/app v1"));
    // Blob reads are not audited by default.
    assert!(srv.audit(&[("action", "blob.pull")]).await.is_empty());

    assert_eq!(reg.status(Method::GET, "/v2/team/app/manifests/v2").await, 404);
    assert_eq!(reg.status(Method::GET, "/v2/team/app/manifests/.INVALID_MANIFEST_NAME").await, 404);
    assert_eq!(reg.status(Method::GET, &format!("/v2/team/app/blobs/{}", sha256(b"nope"))).await, 404);
    assert_eq!(reg.status(Method::GET, "/v2/Team/app/tags/list").await, 400);
}

#[tokio::test]
async fn upload_flows() {
    let srv = TestServer::start().await;
    let reg = srv.admin_registry().await;
    let name = "uploads/test";

    // Monolithic in a single POST.
    let content = b"monolithic".to_vec();
    let res = reg
        .req(Method::POST, &format!("/v2/{name}/blobs/uploads/?digest={}", sha256(&content)))
        .body(content.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    assert_eq!(res.headers()[header::LOCATION], format!("/v2/{name}/blobs/{}", sha256(&content)).as_str());

    // Streamed PATCH then PUT.
    let res = reg.req(Method::POST, &format!("/v2/{name}/blobs/uploads/")).send().await.unwrap();
    assert_eq!(res.status(), 202);
    assert_eq!(res.headers()[header::RANGE], "0-0");
    assert!(res.headers().contains_key("docker-upload-uuid"));
    let loc = res.headers()[header::LOCATION].to_str().unwrap().to_string();
    let res = reg.req(Method::PATCH, &loc).body(b"streamed-".to_vec()).send().await.unwrap();
    assert_eq!(res.status(), 202);
    assert_eq!(res.headers()[header::RANGE], "0-8");
    let loc = res.headers()[header::LOCATION].to_str().unwrap().to_string();
    let res = reg
        .req(Method::PUT, &format!("{loc}?digest={}", sha256(b"streamed-data")))
        .body(b"data".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);

    // Chunked with Content-Range: out of order is 416, status reports progress.
    let blob: Vec<u8> = (0..100u8).collect();
    let res = reg.req(Method::POST, &format!("/v2/{name}/blobs/uploads/")).send().await.unwrap();
    let loc = res.headers()[header::LOCATION].to_str().unwrap().to_string();
    let chunk = |range: &str, data: &[u8]| {
        reg.req(Method::PATCH, &loc)
            .header(header::CONTENT_RANGE, range)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(data.to_vec())
    };
    assert_eq!(chunk("50-99", &blob[50..]).send().await.unwrap().status(), 416);
    let res = chunk("0-49", &blob[..50]).send().await.unwrap();
    assert_eq!(res.status(), 202);
    assert_eq!(res.headers()[header::RANGE], "0-49");
    assert_eq!(chunk("0-49", &blob[..50]).send().await.unwrap().status(), 416, "retrying a chunk is out of order");
    let res = reg.req(Method::GET, &loc).send().await.unwrap();
    assert_eq!(res.status(), 204);
    assert_eq!(res.headers()[header::RANGE], "0-49");
    assert_eq!(chunk("50-99", &blob[50..]).send().await.unwrap().status(), 202);
    // Wrong digest fails and ends the session.
    let res = reg.req(Method::PUT, &format!("{loc}?digest={}", sha256(b"other"))).send().await.unwrap();
    assert_eq!(res.status(), 400);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["errors"][0]["code"], "DIGEST_INVALID");
    assert_eq!(reg.status(Method::GET, &loc).await, 404);
    srv.assert_audit(ADMIN, "blob.upload", Some(name), None, "error").await;

    // Cancel.
    let res = reg.req(Method::POST, &format!("/v2/{name}/blobs/uploads/")).send().await.unwrap();
    let loc = res.headers()[header::LOCATION].to_str().unwrap().to_string();
    assert_eq!(reg.status(Method::DELETE, &loc).await, 204);
    assert_eq!(reg.status(Method::GET, &loc).await, 404);
    srv.assert_audit(ADMIN, "upload.cancel", Some(name), None, "ok").await;

    // Another principal cannot use someone else's session.
    let res = reg.req(Method::POST, &format!("/v2/{name}/blobs/uploads/")).send().await.unwrap();
    let loc = res.headers()[header::LOCATION].to_str().unwrap().to_string();
    let (bob_id, bob_secret) = srv.identity("bob").await;
    srv.grant(name, &bob_id, "write").await;
    assert_eq!(srv.registry("bob", &bob_secret).status(Method::GET, &loc).await, 404);

    let (_, sys) = srv.admin.call(Method::GET, "/uploads", None).await;
    assert_eq!(sys.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn blob_ranges_head_and_delete() {
    let srv = TestServer::start_with(&[("MINREGISTRY_AUDIT_BLOB_READS", "true")]).await;
    let reg = srv.admin_registry().await;
    let content: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
    let digest = sha256(&content);
    assert_eq!(reg.push_blob("r/blobs", &content).await.status(), 201);
    let path = format!("/v2/r/blobs/blobs/{digest}");

    let head = reg.req(Method::HEAD, &path).send().await.unwrap();
    assert_eq!(head.status(), 200);
    assert_eq!(head.headers()[header::CONTENT_LENGTH], "1000");
    assert_eq!(head.headers()[header::ACCEPT_RANGES], "bytes");

    let res = reg.req(Method::GET, &path).header(header::RANGE, "bytes=100-199").send().await.unwrap();
    assert_eq!(res.status(), 206);
    assert_eq!(res.headers()[header::CONTENT_RANGE], "bytes 100-199/1000");
    assert_eq!(res.bytes().await.unwrap().as_ref(), &content[100..200]);
    let res = reg.req(Method::GET, &path).header(header::RANGE, "bytes=900-").send().await.unwrap();
    assert_eq!(res.bytes().await.unwrap().as_ref(), &content[900..]);
    let res = reg.req(Method::GET, &path).header(header::RANGE, "bytes=5000-").send().await.unwrap();
    assert_eq!(res.status(), 416);
    assert_eq!(res.headers()[header::CONTENT_RANGE], "bytes */1000");
    srv.assert_audit(ADMIN, "blob.pull", Some("r/blobs"), None, "ok").await;

    assert_eq!(reg.status(Method::DELETE, &path).await, 202);
    assert_eq!(reg.status(Method::GET, &path).await, 404);
    assert_eq!(reg.status(Method::DELETE, &path).await, 404);
    srv.assert_audit(ADMIN, "blob.delete", Some("r/blobs"), None, "ok").await;
    assert!(srv.blob_path(&digest).exists(), "storage is reclaimed by GC, not by delete");
}

#[tokio::test]
async fn manifests_validation_tags_and_deletes() {
    let srv = TestServer::start().await;
    let reg = srv.admin_registry().await;
    let name = "m/test";
    let config = br#"{"architecture":"arm64","os":"linux","variant":"v8"}"#;
    assert_eq!(reg.push_blob(name, config).await.status(), 201);

    // Unknown layer.
    let missing = image_manifest(config, &[b"not pushed"], None);
    let res = reg.push_manifest(name, "bad", OCI_MANIFEST, &missing).await;
    assert_eq!(res.status(), 400);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["errors"][0]["code"], "MANIFEST_BLOB_UNKNOWN");
    assert_eq!(body["errors"][0]["detail"]["digest"], sha256(b"not pushed"));

    // Invalid JSON, digest mismatch, invalid tag.
    assert_eq!(reg.push_manifest(name, "bad", OCI_MANIFEST, b"blablabla").await.status(), 400);
    let good = image_manifest(config, &[], None);
    let res = reg.push_manifest(name, &sha256(b"x"), OCI_MANIFEST, &good).await;
    assert_eq!(res.status(), 400);
    assert_eq!(res.json::<Value>().await.unwrap()["errors"][0]["code"], "DIGEST_INVALID");
    assert_eq!(reg.push_manifest(name, "-bad-tag", OCI_MANIFEST, &good).await.status(), 400);

    // A manifest without layers, pushed by digest, then tagged many times.
    let digest = sha256(&good);
    let res = reg.push_manifest(name, &digest, OCI_MANIFEST, &good).await;
    assert_eq!(res.status(), 201);
    assert_eq!(res.headers()[header::LOCATION], format!("/v2/{name}/manifests/{digest}").as_str());
    for i in 0..5 {
        assert_eq!(reg.push_manifest(name, &format!("t{i}"), OCI_MANIFEST, &good).await.status(), 201);
    }

    // Tag listing: sorted, paginated with Link.
    let res = reg.req(Method::GET, &format!("/v2/{name}/tags/list?n=2")).send().await.unwrap();
    let link = res.headers()[header::LINK].to_str().unwrap().to_string();
    assert_eq!(link, format!("</v2/{name}/tags/list?n=2&last=t1>; rel=\"next\""));
    let body: Value = res.json().await.unwrap();
    assert_eq!(body, json!({"name": name, "tags": ["t0", "t1"]}));
    let res = reg.req(Method::GET, &format!("/v2/{name}/tags/list?n=2&last=t3")).send().await.unwrap();
    assert!(res.headers().get(header::LINK).is_none());
    assert_eq!(res.json::<Value>().await.unwrap()["tags"], json!(["t4"]));
    let all: Value = reg.req(Method::GET, &format!("/v2/{name}/tags/list")).send().await.unwrap().json().await.unwrap();
    assert_eq!(all["tags"].as_array().unwrap().len(), 5);
    srv.assert_audit(ADMIN, "tag.list", Some(name), None, "ok").await;

    // An index over the manifest; its child must exist in the repository.
    let index = serde_json::to_vec(&json!({
        "schemaVersion": 2, "mediaType": OCI_INDEX,
        "manifests": [{"mediaType": OCI_MANIFEST, "digest": digest, "size": good.len(), "platform": {"os": "windows", "architecture": "amd64", "os.version": "10.0.20348.2655"}}]
    }))
    .unwrap();
    assert_eq!(reg.push_manifest(name, "multi", OCI_INDEX, &index).await.status(), 201);
    let other_repo_index = reg.push_manifest("m/other", "multi", OCI_INDEX, &index).await;
    assert_eq!(other_repo_index.status(), 400);

    // The repository detail shows platforms (from the index and from the config blob).
    let repo = srv.repo_id(name).await;
    let (_, detail) = srv.admin.call(Method::GET, &format!("/repositories/{repo}"), None).await;
    let by_digest =
        |d: &str| detail["manifests"].as_array().unwrap().iter().find(|m| m["digest"] == d).unwrap().clone();
    assert_eq!(
        by_digest(&digest)["platforms"],
        json!([{"os": "linux", "architecture": "arm64", "variant": "v8", "os_version": null}])
    );
    assert_eq!(
        by_digest(&sha256(&index))["platforms"],
        json!([{"os": "windows", "architecture": "amd64", "variant": null, "os_version": "10.0.20348.2655"}])
    );
    // Indexes list the child manifests stored in the repository.
    assert_eq!(by_digest(&sha256(&index))["child_digests"], json!([digest]));
    assert_eq!(by_digest(&digest)["child_digests"], json!([]));
    assert_eq!(by_digest(&digest)["tags"].as_array().unwrap().len(), 5);

    // Tag delete removes only the tag; digest delete removes the manifest and its tags.
    assert_eq!(reg.status(Method::DELETE, &format!("/v2/{name}/manifests/t0")).await, 202);
    assert_eq!(reg.status(Method::GET, &format!("/v2/{name}/manifests/t0")).await, 404);
    assert_eq!(reg.status(Method::GET, &format!("/v2/{name}/manifests/{digest}")).await, 200);
    srv.assert_audit(ADMIN, "tag.delete", Some(name), Some("t0"), "ok").await;
    assert_eq!(reg.status(Method::DELETE, &format!("/v2/{name}/manifests/{digest}")).await, 202);
    assert_eq!(reg.status(Method::GET, &format!("/v2/{name}/manifests/t1")).await, 404);
    assert_eq!(reg.status(Method::DELETE, &format!("/v2/{name}/manifests/{digest}")).await, 404);
    srv.assert_audit(ADMIN, "manifest.delete", Some(name), Some(&digest), "ok").await;
    let all: Value = reg.req(Method::GET, &format!("/v2/{name}/tags/list")).send().await.unwrap().json().await.unwrap();
    assert_eq!(all["tags"], json!(["multi"]));
}

#[tokio::test]
async fn referrers() {
    let srv = TestServer::start().await;
    let reg = srv.admin_registry().await;
    let name = "refs/app";
    let (subject, subject_bytes, _) = reg.push_image(name, "latest", "amd64").await;
    let empty = b"{}";
    assert_eq!(reg.push_blob(name, empty).await.status(), 201);
    let sbom = b"sbom contents";
    assert_eq!(reg.push_blob(name, sbom).await.status(), 201);

    let artifact = |artifact_type: &str, note: &str| {
        serde_json::to_vec(&json!({
            "schemaVersion": 2, "mediaType": OCI_MANIFEST, "artifactType": artifact_type,
            "config": {"mediaType": "application/vnd.oci.empty.v1+json", "digest": sha256(empty), "size": 2},
            "layers": [{"mediaType": "text/plain", "digest": sha256(sbom), "size": sbom.len()}],
            "subject": {"mediaType": OCI_MANIFEST, "digest": subject, "size": subject_bytes.len()},
            "annotations": {"org.example.note": note}
        }))
        .unwrap()
    };
    let a = artifact("application/vnd.example.sbom", "a");
    let b = artifact("application/vnd.example.sig", "b");
    for m in [&a, &b] {
        let res = reg.push_manifest(name, &sha256(m), OCI_MANIFEST, m).await;
        assert_eq!(res.status(), 201);
        assert_eq!(res.headers()["oci-subject"], subject.as_str());
    }

    let res = reg.req(Method::GET, &format!("/v2/{name}/referrers/{subject}")).send().await.unwrap();
    assert_eq!(res.headers()[header::CONTENT_TYPE], OCI_INDEX);
    assert!(res.headers().get("oci-filters-applied").is_none());
    let index: Value = res.json().await.unwrap();
    let manifests = index["manifests"].as_array().unwrap();
    assert_eq!(manifests.len(), 2);
    assert_eq!(manifests[0]["artifactType"], "application/vnd.example.sbom");
    assert_eq!(manifests[0]["annotations"]["org.example.note"], "a");
    assert_eq!(manifests[0]["size"], a.len());

    let res = reg
        .req(Method::GET, &format!("/v2/{name}/referrers/{subject}?artifactType=application/vnd.example.sig"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.headers()["oci-filters-applied"], "artifactType");
    let index: Value = res.json().await.unwrap();
    assert_eq!(index["manifests"].as_array().unwrap().len(), 1);
    assert_eq!(index["manifests"][0]["digest"], sha256(&b));

    // Unknown subject: empty index, not 404.
    let res = reg.req(Method::GET, &format!("/v2/{name}/referrers/{}", sha256(b"none"))).send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.json::<Value>().await.unwrap()["manifests"], json!([]));
    srv.assert_audit(ADMIN, "referrers.list", Some(name), None, "ok").await;
}

#[tokio::test]
async fn cross_repository_mount() {
    let srv = TestServer::start().await;
    let (alice_id, alice) = srv.identity("alice").await;
    let reg = srv.registry("alice", &alice);
    let layer = b"shared base layer".to_vec();
    let digest = sha256(&layer);
    assert_eq!(reg.push_blob("base/a", &layer).await.status(), 201);

    // Mount into a new repository (auto-created): 201, no upload.
    let res =
        reg.req(Method::POST, &format!("/v2/base/b/blobs/uploads/?mount={digest}&from=base/a")).send().await.unwrap();
    assert_eq!(res.status(), 201);
    assert_eq!(res.headers()[header::LOCATION], format!("/v2/base/b/blobs/{digest}").as_str());
    assert_eq!(reg.status(Method::GET, &format!("/v2/base/b/blobs/{digest}")).await, 200);
    srv.assert_audit("alice", "blob.mount", Some("base/b"), None, "ok").await;

    // Unknown blob or no `from`: a regular upload session (202).
    let res = reg
        .req(Method::POST, &format!("/v2/base/b/blobs/uploads/?mount={}&from=base/a", sha256(b"x")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 202);
    let res = reg.req(Method::POST, &format!("/v2/base/b/blobs/uploads/?mount={digest}")).send().await.unwrap();
    assert_eq!(res.status(), 202);

    // Without read access to the source, the mount falls through (and is audited).
    let (bob_id, bob) = srv.identity("bob").await;
    let bob_reg = srv.registry("bob", &bob);
    let res = bob_reg
        .req(Method::POST, &format!("/v2/bob/repo/blobs/uploads/?mount={digest}&from=base/a"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 202);
    assert_eq!(bob_reg.status(Method::GET, &format!("/v2/bob/repo/blobs/{digest}")).await, 404);
    srv.assert_audit("bob", "blob.mount", Some("bob/repo"), None, "denied").await;
    srv.grant("base/a", &bob_id, "read").await;
    let res = bob_reg
        .req(Method::POST, &format!("/v2/bob/repo/blobs/uploads/?mount={digest}&from=base/a"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let _ = alice_id;
}

#[tokio::test]
async fn auth_matrix() {
    let srv = TestServer::start().await;
    let (_, owner) = srv.identity("owner").await;
    let owner_reg = srv.registry("owner", &owner);
    owner_reg.push_image("secret/app", "v1", "amd64").await;

    let (reader_id, reader) = srv.identity("reader").await;
    let (writer_id, writer) = srv.identity("writer").await;
    let (_, stranger) = srv.identity("stranger").await;
    srv.grant("secret/app", &reader_id, "read").await;
    srv.grant("secret/app", &writer_id, "write").await;
    let reader_reg = srv.registry("reader", &reader);
    let writer_reg = srv.registry("writer", &writer);
    let stranger_reg = srv.registry("stranger", &stranger);

    // read: pull yes, push/delete no (403, audited as denied).
    assert_eq!(reader_reg.status(Method::GET, "/v2/secret/app/manifests/v1").await, 200);
    assert_eq!(reader_reg.status(Method::POST, "/v2/secret/app/blobs/uploads/").await, 403);
    assert_eq!(reader_reg.status(Method::DELETE, "/v2/secret/app/manifests/v1").await, 403);
    srv.assert_audit("reader", "blob.upload", Some("secret/app"), None, "denied").await;
    srv.assert_audit("reader", "tag.delete", Some("secret/app"), None, "denied").await;

    // No grant on an existing repository: 403 for reads and writes.
    let res = stranger_reg.req(Method::GET, "/v2/secret/app/manifests/v1").send().await.unwrap();
    assert_eq!(res.status(), 403);
    assert_eq!(res.json::<Value>().await.unwrap()["errors"][0]["code"], "DENIED");
    assert_eq!(stranger_reg.status(Method::POST, "/v2/secret/app/blobs/uploads/").await, 403);
    srv.assert_audit("stranger", "manifest.pull", Some("secret/app"), None, "denied").await;
    // A missing repository reads as empty (404) so clients can check before
    // their first push (docs/adr/0004).
    assert_eq!(stranger_reg.status(Method::GET, "/v2/does/not-exist/tags/list").await, 404);
    assert_eq!(stranger_reg.status(Method::HEAD, &format!("/v2/does/not-exist/blobs/{}", sha256(b"x"))).await, 404);
    let admin = srv.admin_registry().await;
    assert_eq!(admin.status(Method::GET, "/v2/does/not-exist/tags/list").await, 404);
    assert_eq!(admin.status(Method::GET, "/v2/secret/app/manifests/v1").await, 200);

    // write: push and delete.
    writer_reg.push_image("secret/app", "v2", "arm64").await;
    assert_eq!(writer_reg.status(Method::DELETE, "/v2/secret/app/manifests/v2").await, 202);

    // Revoked token → 401.
    let (_, detail) = srv.admin.call(Method::GET, &format!("/principals/{reader_id}"), None).await;
    let token_id = detail["tokens"][0]["id"].as_str().unwrap().to_string();
    let (status, _) = srv.admin.call(Method::DELETE, &format!("/principals/{reader_id}/tokens/{token_id}"), None).await;
    assert_eq!(status, 204);
    assert_eq!(reader_reg.status(Method::GET, "/v2/secret/app/manifests/v1").await, 401);

    // Disabled identity → 401; re-enabled → works.
    let (status, _) =
        srv.admin.call(Method::PATCH, &format!("/principals/{writer_id}"), Some(json!({"enabled": false}))).await;
    assert_eq!(status, 200);
    assert_eq!(writer_reg.status(Method::GET, "/v2/").await, 401);
    srv.admin.call(Method::PATCH, &format!("/principals/{writer_id}"), Some(json!({"enabled": true}))).await;
    assert_eq!(writer_reg.status(Method::GET, "/v2/").await, 200);
    srv.assert_audit("writer", "login", None, None, "denied").await;
}
