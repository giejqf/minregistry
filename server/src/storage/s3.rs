//! S3-compatible storage (AWS S3, MinIO, R2, …) built on `object_store`.

use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use bytes::BytesMut;
use futures::{stream::BoxStream, StreamExt, TryStreamExt};
use object_store::{
    aws::AmazonS3Builder, path::Path as ObjectPath, GetOptions, GetRange, ObjectStore, ObjectStoreExt, PutPayload,
    WriteMultipart,
};
use tokio::io::AsyncReadExt;

use super::{blob_key, digest_from_key, BlobStream, Range, Result, Storage, StorageError, StorageInfo};
use crate::{config::S3Config, digest::Digest};

/// Files up to this size are uploaded with one PUT; larger ones as multipart
/// uploads with parts of this size.
const PART_SIZE: usize = 16 * 1024 * 1024;
const MAX_PARALLEL_PARTS: usize = 4;

pub(crate) struct S3Storage {
    store: Arc<dyn ObjectStore>,
    location: String,
}

impl S3Storage {
    pub(crate) fn new(cfg: &S3Config) -> anyhow::Result<Self> {
        let mut builder = match (&cfg.access_key, &cfg.secret_key) {
            (Some(access), Some(secret)) => {
                AmazonS3Builder::new().with_access_key_id(access).with_secret_access_key(secret)
            }
            // Fall back to the standard AWS_* variables / instance credentials.
            _ => AmazonS3Builder::from_env(),
        };
        builder = builder
            .with_region(&cfg.region)
            .with_bucket_name(&cfg.bucket)
            .with_virtual_hosted_style_request(!cfg.path_style);
        let mut location = format!("s3://{}", cfg.bucket);
        if let Some(endpoint) = &cfg.endpoint {
            builder = builder.with_allow_http(endpoint.starts_with("http://"));
            let endpoint =
                if cfg.path_style { endpoint.clone() } else { virtual_hosted_endpoint(endpoint, &cfg.bucket)? };
            location = format!("{location} ({endpoint})");
            builder = builder.with_endpoint(endpoint);
        }
        Ok(S3Storage { store: Arc::new(builder.build()?), location })
    }

    fn path(digest: &Digest) -> ObjectPath {
        ObjectPath::from(blob_key(digest))
    }
}

/// object_store expects virtual-hosted endpoints to already name the bucket.
fn virtual_hosted_endpoint(endpoint: &str, bucket: &str) -> anyhow::Result<String> {
    let mut url = url::Url::parse(endpoint)?;
    let host = url.host_str().ok_or_else(|| anyhow::anyhow!("S3 endpoint has no host"))?.to_string();
    if !host.starts_with(&format!("{bucket}.")) {
        url.set_host(Some(&format!("{bucket}.{host}")))?;
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

fn map_err(e: object_store::Error) -> StorageError {
    match e {
        object_store::Error::NotFound { .. } => StorageError::NotFound,
        other => StorageError::Backend(other.to_string()),
    }
}

#[async_trait]
impl Storage for S3Storage {
    async fn blob_exists(&self, digest: &Digest) -> Result<bool> {
        Ok(self.blob_size(digest).await?.is_some())
    }

    async fn blob_size(&self, digest: &Digest) -> Result<Option<u64>> {
        match self.store.head(&Self::path(digest)).await {
            Ok(meta) => Ok(Some(meta.size)),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(map_err(e)),
        }
    }

    async fn get_blob(&self, digest: &Digest, range: Option<Range>) -> Result<BlobStream> {
        let options = GetOptions { range: range.map(|r| GetRange::Bounded(r.start..r.end + 1)), ..Default::default() };
        let result = self.store.get_opts(&Self::path(digest), options).await.map_err(map_err)?;
        Ok(result.into_stream().map_err(std::io::Error::other).boxed())
    }

    async fn put_blob_from_file(&self, digest: &Digest, path: &Path) -> Result<()> {
        let key = Self::path(digest);
        let mut file = tokio::fs::File::open(path).await?;
        let size = file.metadata().await?.len();
        if size <= PART_SIZE as u64 {
            let mut buf = Vec::with_capacity(size as usize);
            file.read_to_end(&mut buf).await?;
            self.store.put(&key, PutPayload::from(buf)).await.map_err(map_err)?;
            return Ok(());
        }
        let upload = self.store.put_multipart(&key).await.map_err(map_err)?;
        let mut writer = WriteMultipart::new_with_chunk_size(upload, PART_SIZE);
        let copied: Result<()> = async {
            loop {
                let mut chunk = BytesMut::with_capacity(PART_SIZE);
                while chunk.len() < PART_SIZE {
                    if file.read_buf(&mut chunk).await? == 0 {
                        break;
                    }
                }
                if chunk.is_empty() {
                    return Ok(());
                }
                writer.wait_for_capacity(MAX_PARALLEL_PARTS).await.map_err(map_err)?;
                writer.put(chunk.freeze());
            }
        }
        .await;
        match copied {
            Ok(()) => {
                writer.finish().await.map_err(map_err)?;
                Ok(())
            }
            Err(e) => {
                let _ = writer.abort().await;
                Err(e)
            }
        }
    }

    async fn delete_blob(&self, digest: &Digest) -> Result<()> {
        match self.store.delete(&Self::path(digest)).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(e) => Err(map_err(e)),
        }
    }

    async fn list_blobs(&self) -> Result<BoxStream<'static, Result<Digest>>> {
        let prefix = ObjectPath::from("blobs");
        Ok(self
            .store
            .list(Some(&prefix))
            .filter_map(|item| async move {
                match item {
                    Ok(meta) => digest_from_key(meta.location.as_ref()).map(Ok),
                    Err(e) => Some(Err(map_err(e))),
                }
            })
            .boxed())
    }

    fn describe(&self) -> StorageInfo {
        StorageInfo { backend: "s3", location: self.location.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_hosted_endpoints_name_the_bucket() {
        assert_eq!(
            virtual_hosted_endpoint("https://s3.eu-west-1.amazonaws.com", "images").unwrap(),
            "https://images.s3.eu-west-1.amazonaws.com"
        );
        assert_eq!(
            virtual_hosted_endpoint("https://images.example.com/", "images").unwrap(),
            "https://images.example.com"
        );
    }
}
