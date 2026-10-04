//! The storage contract. Every backend must pass the same suite; the S3 run
//! targets MinIO and is skipped unless `MINREGISTRY_TEST_S3_ENDPOINT` is set.

use std::{collections::HashSet, path::Path};

use futures::TryStreamExt;

use super::*;
use crate::config::S3Config;

fn random_bytes(len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    getrandom::fill(&mut buf).expect("random bytes");
    buf
}

async fn stage(dir: &Path, content: &[u8]) -> (Digest, std::path::PathBuf) {
    let digest = Digest::of(content);
    let path = dir.join(digest.hex());
    tokio::fs::write(&path, content).await.unwrap();
    (digest, path)
}

async fn read_all(storage: &dyn Storage, digest: &Digest, range: Option<Range>) -> Vec<u8> {
    let chunks: Vec<bytes::Bytes> = storage.get_blob(digest, range).await.unwrap().try_collect().await.unwrap();
    chunks.concat()
}

async fn listed(storage: &dyn Storage) -> HashSet<Digest> {
    storage.list_blobs().await.unwrap().try_collect().await.unwrap()
}

async fn contract(storage: &dyn Storage) {
    let scratch = tempfile::tempdir().unwrap();
    let small = random_bytes(300 * 1024 + 7);
    let (digest, staged) = stage(scratch.path(), &small).await;

    storage.check().await.expect("the backend is ready");
    assert!(!storage.blob_exists(&digest).await.unwrap());
    assert_eq!(storage.blob_size(&digest).await.unwrap(), None);
    assert!(matches!(storage.get_blob(&digest, None).await, Err(StorageError::NotFound)));
    storage.delete_blob(&digest).await.expect("deleting a missing blob succeeds");

    storage.put_blob_from_file(&digest, &staged).await.unwrap();
    assert!(staged.exists(), "the staged file is left for the caller");
    assert!(storage.blob_exists(&digest).await.unwrap());
    assert_eq!(storage.blob_size(&digest).await.unwrap(), Some(small.len() as u64));
    assert_eq!(read_all(storage, &digest, None).await, small);
    assert_eq!(read_all(storage, &digest, Some(Range { start: 10, end: 99 })).await, &small[10..100]);
    let last = small.len() as u64 - 1;
    assert_eq!(read_all(storage, &digest, Some(Range { start: last, end: last })).await, &small[small.len() - 1..]);

    // Idempotent re-put.
    storage.put_blob_from_file(&digest, &staged).await.unwrap();
    assert_eq!(read_all(storage, &digest, None).await, small);

    // Large enough for the multipart path of object stores.
    let large = random_bytes(20 * 1024 * 1024 + 123);
    let (large_digest, large_staged) = stage(scratch.path(), &large).await;
    storage.put_blob_from_file(&large_digest, &large_staged).await.unwrap();
    assert_eq!(storage.blob_size(&large_digest).await.unwrap(), Some(large.len() as u64));
    let tail = Range { start: large.len() as u64 - 1000, end: large.len() as u64 - 1 };
    assert_eq!(read_all(storage, &large_digest, Some(tail)).await, &large[large.len() - 1000..]);

    let all = listed(storage).await;
    assert!(all.contains(&digest) && all.contains(&large_digest));

    storage.delete_blob(&digest).await.unwrap();
    storage.delete_blob(&large_digest).await.unwrap();
    assert!(!storage.blob_exists(&digest).await.unwrap());
    assert!(matches!(storage.get_blob(&large_digest, None).await, Err(StorageError::NotFound)));
    let all = listed(storage).await;
    assert!(!all.contains(&digest) && !all.contains(&large_digest));
}

#[test]
fn key_layout() {
    let d = Digest::of(b"layout");
    let key = blob_key(&d);
    assert_eq!(key, format!("blobs/sha256/{}/{}/data", &d.hex()[..2], d.hex()));
    assert_eq!(digest_from_key(&key), Some(d));
    assert_eq!(digest_from_key("blobs/sha256/zz/abc/data"), None);
    assert_eq!(digest_from_key("uploads/x"), None);
}

#[tokio::test]
async fn fs_backend() {
    let root = tempfile::tempdir().unwrap();
    let storage = FsStorage::new(root.path().to_path_buf()).await.unwrap();
    contract(&storage).await;
    assert!(matches!(
        storage.get_blob(&Digest::of(b"missing"), Some(Range { start: 0, end: 1 })).await,
        Err(StorageError::NotFound)
    ));

    tokio::fs::remove_dir_all(root.path().join("blobs")).await.unwrap();
    assert!(storage.check().await.is_err(), "a missing blobs directory is not ready");
}

/// The MinIO test configuration, or `None` to skip.
fn s3_test_config() -> Option<S3Config> {
    let Ok(endpoint) = std::env::var("MINREGISTRY_TEST_S3_ENDPOINT") else {
        eprintln!("skipping: MINREGISTRY_TEST_S3_ENDPOINT is not set");
        return None;
    };
    let _ = rustls::crypto::ring::default_provider().install_default();
    let env = |k: &str, default: &str| std::env::var(k).unwrap_or_else(|_| default.to_string());
    Some(S3Config {
        endpoint: Some(endpoint),
        region: env("MINREGISTRY_TEST_S3_REGION", "us-east-1"),
        bucket: env("MINREGISTRY_TEST_S3_BUCKET", "minregistry-test"),
        access_key: Some(env("MINREGISTRY_TEST_S3_ACCESS_KEY", "minioadmin")),
        secret_key: Some(env("MINREGISTRY_TEST_S3_SECRET_KEY", "minioadmin")),
        path_style: true,
    })
}

#[tokio::test]
async fn s3_backend() {
    let Some(cfg) = s3_test_config() else { return };
    let storage = S3Storage::new(&cfg).unwrap();
    contract(&storage).await;
}

/// S3 answers a HEAD with 404 for a missing object and a missing bucket
/// alike. A missing bucket must still fail the readiness check, and writing
/// to it is a backend error, not "blob not found".
#[tokio::test]
async fn s3_missing_bucket() {
    let Some(cfg) = s3_test_config() else { return };
    let bucket = format!("minregistry-missing-{}", uuid::Uuid::new_v4().simple());
    let storage = S3Storage::new(&S3Config { bucket, ..cfg }).unwrap();
    let digest = Digest::of(b"missing bucket");
    assert!(!storage.blob_exists(&digest).await.unwrap());

    let err = storage.check().await.expect_err("a missing bucket is not ready");
    assert!(matches!(err, StorageError::Backend(_)), "{err}");
    let scratch = tempfile::tempdir().unwrap();
    let (digest, staged) = stage(scratch.path(), b"missing bucket").await;
    let err = storage.put_blob_from_file(&digest, &staged).await.expect_err("no bucket to write to");
    assert!(matches!(err, StorageError::Backend(_)), "{err}");
    let listed: std::result::Result<Vec<Digest>, _> = storage.list_blobs().await.unwrap().try_collect().await;
    assert!(matches!(listed, Err(StorageError::Backend(_))));
}
