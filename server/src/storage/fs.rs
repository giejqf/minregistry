//! Local filesystem storage rooted at `MINREGISTRY_FS_ROOT`.

use std::{
    io::{self, SeekFrom},
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use futures::{stream::BoxStream, SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

use super::{blob_key, BlobStream, Range, Result, Storage, StorageError, StorageInfo};
use crate::digest::Digest;

pub(crate) struct FsStorage {
    root: PathBuf,
}

const READ_BUFFER: usize = 256 * 1024;

impl FsStorage {
    pub(crate) async fn new(root: PathBuf) -> io::Result<Self> {
        tokio::fs::create_dir_all(root.join("blobs")).await?;
        tokio::fs::create_dir_all(root.join(".tmp")).await?;
        Ok(FsStorage { root })
    }

    fn path(&self, digest: &Digest) -> PathBuf {
        self.root.join(blob_key(digest))
    }
}

fn not_found_is_none<T>(r: io::Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[async_trait]
impl Storage for FsStorage {
    async fn blob_exists(&self, digest: &Digest) -> Result<bool> {
        Ok(self.blob_size(digest).await?.is_some())
    }

    async fn blob_size(&self, digest: &Digest) -> Result<Option<u64>> {
        Ok(not_found_is_none(tokio::fs::metadata(self.path(digest)).await)?.map(|m| m.len()))
    }

    async fn get_blob(&self, digest: &Digest, range: Option<Range>) -> Result<BlobStream> {
        let mut file =
            not_found_is_none(tokio::fs::File::open(self.path(digest)).await)?.ok_or(StorageError::NotFound)?;
        match range {
            None => Ok(ReaderStream::with_capacity(file, READ_BUFFER).boxed()),
            Some(r) => {
                let size = file.metadata().await?.len();
                if r.start > r.end || r.end >= size {
                    return Err(StorageError::InvalidRange);
                }
                file.seek(SeekFrom::Start(r.start)).await?;
                Ok(ReaderStream::with_capacity(file.take(r.len()), READ_BUFFER).boxed())
            }
        }
    }

    async fn put_blob_from_file(&self, digest: &Digest, path: &Path) -> Result<()> {
        let dest = self.path(digest);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        // Stage next to the destination, then rename: readers never see a
        // partial blob. Hard-linking is free when the upload dir shares the
        // filesystem; otherwise copy.
        let tmp = self.root.join(".tmp").join(uuid::Uuid::new_v4().to_string());
        if tokio::fs::hard_link(path, &tmp).await.is_err() {
            if let Err(e) = tokio::fs::copy(path, &tmp).await {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err(e.into());
            }
        }
        let renamed = tokio::fs::rename(&tmp, &dest).await;
        if renamed.is_err() {
            let _ = tokio::fs::remove_file(&tmp).await;
        }
        Ok(renamed?)
    }

    async fn delete_blob(&self, digest: &Digest) -> Result<()> {
        let path = self.path(digest);
        not_found_is_none(tokio::fs::remove_file(&path).await)?;
        // Best effort: drop the now-empty per-digest directory.
        if let Some(dir) = path.parent() {
            let _ = tokio::fs::remove_dir(dir).await;
        }
        Ok(())
    }

    async fn list_blobs(&self) -> Result<BoxStream<'static, Result<Digest>>> {
        let root = self.root.join("blobs");
        let (mut tx, rx) = futures::channel::mpsc::channel::<Result<Digest>>(256);
        tokio::spawn(async move {
            if let Err(e) = walk(&root, &mut tx).await {
                let _ = tx.send(Err(e)).await;
            }
        });
        Ok(rx.boxed())
    }

    async fn check(&self) -> Result<()> {
        if !tokio::fs::metadata(self.root.join("blobs")).await?.is_dir() {
            return Err(StorageError::Backend("the blobs directory is not a directory".into()));
        }
        Ok(())
    }

    fn describe(&self) -> StorageInfo {
        StorageInfo { backend: "fs", location: self.root.display().to_string() }
    }
}

/// Walks `blobs/<algo>/<xx>/<hex>/data`, sending each digest found.
async fn walk(root: &Path, tx: &mut futures::channel::mpsc::Sender<Result<Digest>>) -> Result<()> {
    let Some(mut algos) = not_found_is_none(tokio::fs::read_dir(root).await)? else {
        return Ok(());
    };
    while let Some(algo) = algos.next_entry().await? {
        let mut prefixes = tokio::fs::read_dir(algo.path()).await?;
        while let Some(prefix) = prefixes.next_entry().await? {
            let mut blobs = tokio::fs::read_dir(prefix.path()).await?;
            while let Some(blob) = blobs.next_entry().await? {
                let key = format!(
                    "blobs/{}/{}/{}/data",
                    algo.file_name().to_string_lossy(),
                    prefix.file_name().to_string_lossy(),
                    blob.file_name().to_string_lossy()
                );
                let Some(digest) = super::digest_from_key(&key) else { continue };
                if tokio::fs::metadata(blob.path().join("data")).await.is_ok() && tx.send(Ok(digest)).await.is_err() {
                    return Ok(()); // receiver dropped
                }
            }
        }
    }
    Ok(())
}
