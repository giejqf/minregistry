//! Blob storage. Content is addressed by digest under the key layout
//! `blobs/sha256/<first two hex>/<full hex>/data`, identical on every backend.
//! This layout is a public contract once deployed: do not change it without an ADR.

use std::{io, path::Path, sync::Arc};

use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::BoxStream;

use crate::{config::StorageConfig, digest::Digest};

mod fs;
mod s3;
#[cfg(test)]
mod tests;

pub(crate) use fs::FsStorage;
pub(crate) use s3::S3Storage;

/// An inclusive byte range, as in `Range: bytes=start-end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Range {
    pub start: u64,
    pub end: u64,
}

impl Range {
    pub(crate) fn len(&self) -> u64 {
        self.end - self.start + 1
    }
}

pub(crate) type BlobStream = BoxStream<'static, io::Result<Bytes>>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum StorageError {
    #[error("blob not found")]
    NotFound,
    #[error("requested range is not satisfiable")]
    InvalidRange,
    #[error("storage I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("storage backend error: {0}")]
    Backend(String),
}

pub(crate) type Result<T> = std::result::Result<T, StorageError>;

#[async_trait]
pub(crate) trait Storage: Send + Sync {
    async fn blob_exists(&self, digest: &Digest) -> Result<bool>;
    async fn blob_size(&self, digest: &Digest) -> Result<Option<u64>>;
    /// Streams the blob, or the inclusive `range` of it. The range must lie
    /// within the blob (callers clamp it against the known size).
    async fn get_blob(&self, digest: &Digest, range: Option<Range>) -> Result<BlobStream>;
    /// Stores the file at `path` as `digest`. The file is left in place; the
    /// caller verified the digest while staging it.
    async fn put_blob_from_file(&self, digest: &Digest, path: &Path) -> Result<()>;
    /// Deletes the blob; deleting a missing blob succeeds.
    async fn delete_blob(&self, digest: &Digest) -> Result<()>;
    /// Every blob present in storage (for garbage collection).
    async fn list_blobs(&self) -> Result<BoxStream<'static, Result<Digest>>>;
    /// Human-readable backend description for the System page.
    fn describe(&self) -> StorageInfo;
}

#[derive(Clone, Debug)]
pub(crate) struct StorageInfo {
    pub backend: &'static str,
    pub location: String,
}

/// The object key of a blob, shared by every backend.
pub(crate) fn blob_key(digest: &Digest) -> String {
    let hex = digest.hex();
    format!("blobs/{}/{}/{}/data", digest.algorithm(), &hex[..2], hex)
}

/// Parses a key produced by [`blob_key`] back into a digest.
pub(crate) fn digest_from_key(key: &str) -> Option<Digest> {
    let mut parts = key.split('/');
    let (Some("blobs"), Some(algo), Some(prefix), Some(hex), Some("data"), None) =
        (parts.next(), parts.next(), parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let digest = Digest::parse(&format!("{algo}:{hex}")).ok()?;
    (hex.starts_with(prefix) && prefix.len() == 2).then_some(digest)
}

pub(crate) async fn from_config(cfg: &StorageConfig) -> anyhow::Result<Arc<dyn Storage>> {
    Ok(match cfg {
        StorageConfig::Fs { root } => Arc::new(FsStorage::new(root.clone()).await?),
        StorageConfig::S3(s3) => Arc::new(S3Storage::new(s3)?),
    })
}
