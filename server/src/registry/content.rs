//! Making content durable without racing garbage collection (docs/adr/0003).
//!
//! The storage object is written first. Then, inside a write transaction (the
//! global SQLite write lock), the `blobs` row is created if missing — after
//! checking that the object still exists, because a GC sweep may have deleted
//! it in between — and any pending sweep of the digest is cancelled. GC deletes
//! a storage object only while holding the same lock and only if the digest is
//! still queued for sweeping, so the two can never interleave badly.

use std::path::Path;

use crate::{
    app::AppState,
    db::{self, Tx},
    digest::Digest,
    error::{AppError, AppResult},
};

/// How often a commit retries when GC removes the object underneath it.
const ATTEMPTS: usize = 3;

/// Inside `tx`: ensures the `blobs` row for `digest` exists and is not queued
/// for sweeping. Returns `false` if the storage object is gone (re-upload it
/// and retry).
pub(crate) async fn ensure_blob_row(state: &AppState, tx: &mut Tx, digest: &Digest, size: u64) -> AppResult<bool> {
    let size = i64::try_from(size).map_err(AppError::internal)?;
    if !db::blobs::row_exists(&mut **tx, digest.as_str()).await? {
        if !state.storage.blob_exists(digest).await? {
            return Ok(false);
        }
        db::blobs::insert(tx, digest.as_str(), size, &crate::time::now()).await?;
    }
    db::blobs::unmark(tx, digest.as_str()).await?;
    Ok(true)
}

/// Whether the storage object must be (re-)written before committing.
pub(crate) async fn needs_put(state: &AppState, digest: &Digest) -> AppResult<bool> {
    Ok(match db::blobs::state(&state.db.read, digest.as_str()).await? {
        Some(s) => s.pending_sweep,
        None => true,
    })
}

/// Stores the verified file `staged` as `digest` and links it into `repository_id`.
pub(crate) async fn commit_blob(
    state: &AppState,
    digest: &Digest,
    size: u64,
    staged: &Path,
    repository_id: i64,
) -> AppResult<()> {
    let mut put = needs_put(state, digest).await?;
    for _ in 0..ATTEMPTS {
        if put {
            state.storage.put_blob_from_file(digest, staged).await?;
        }
        let mut tx = state.db.begin_write().await?;
        if !ensure_blob_row(state, &mut tx, digest, size).await? {
            tx.rollback().await?;
            put = true;
            continue;
        }
        db::blobs::link(&mut tx, repository_id, digest.as_str(), &crate::time::now()).await?;
        tx.commit().await?;
        return Ok(());
    }
    Err(AppError::internal(format_args!("blob {digest} kept disappearing from storage during commit")))
}
