//! `blobs`, `repository_blobs` and the GC bookkeeping table `gc_sweep`.
//! The coordination protocol between uploads and GC is in docs/adr/0003.

use sqlx::SqliteExecutor;

use super::Tx;

/// Size of `digest` if it is linked into (visible from) the repository.
pub(crate) async fn linked_size(
    db: impl SqliteExecutor<'_>,
    repository_id: i64,
    digest: &str,
) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar!(
        "SELECT b.size FROM repository_blobs rb JOIN blobs b ON b.digest = rb.digest
         WHERE rb.repository_id = ? AND rb.digest = ?",
        repository_id,
        digest
    )
    .fetch_optional(db)
    .await
}

pub(crate) struct BlobState {
    /// Chosen for deletion by GC; the storage object may vanish at any time.
    pub pending_sweep: bool,
}

pub(crate) async fn state(db: impl SqliteExecutor<'_>, digest: &str) -> sqlx::Result<Option<BlobState>> {
    sqlx::query_as!(
        BlobState,
        r#"SELECT EXISTS (SELECT 1 FROM gc_sweep g WHERE g.digest = b.digest) AS "pending_sweep!: bool"
           FROM blobs b WHERE b.digest = ?"#,
        digest
    )
    .fetch_optional(db)
    .await
}

pub(crate) async fn row_exists(db: impl SqliteExecutor<'_>, digest: &str) -> sqlx::Result<bool> {
    sqlx::query_scalar!(r#"SELECT EXISTS (SELECT 1 FROM blobs WHERE digest = ?) AS "e!: bool""#, digest)
        .fetch_one(db)
        .await
}

pub(crate) async fn insert(tx: &mut Tx, digest: &str, size: i64, now: &str) -> sqlx::Result<()> {
    sqlx::query!("INSERT INTO blobs (digest, size, created_at) VALUES (?, ?, ?)", digest, size, now)
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

/// Rescues a blob from a pending GC sweep.
pub(crate) async fn unmark(tx: &mut Tx, digest: &str) -> sqlx::Result<()> {
    sqlx::query!("DELETE FROM gc_sweep WHERE digest = ?", digest).execute(&mut **tx).await.map(|_| ())
}

/// Links a blob into a repository; re-linking refreshes the link time.
pub(crate) async fn link(tx: &mut Tx, repository_id: i64, digest: &str, now: &str) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT INTO repository_blobs (repository_id, digest, created_at) VALUES (?, ?, ?)
         ON CONFLICT (repository_id, digest) DO UPDATE SET created_at = excluded.created_at",
        repository_id,
        digest,
        now
    )
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

pub(crate) async fn unlink(db: impl SqliteExecutor<'_>, repository_id: i64, digest: &str) -> sqlx::Result<bool> {
    let r = sqlx::query!("DELETE FROM repository_blobs WHERE repository_id = ? AND digest = ?", repository_id, digest)
        .execute(db)
        .await?;
    Ok(r.rows_affected() == 1)
}

pub(crate) struct BlobStats {
    pub count: i64,
    pub bytes: i64,
}

pub(crate) async fn stats(db: impl SqliteExecutor<'_>) -> sqlx::Result<BlobStats> {
    sqlx::query_as!(
        BlobStats,
        r#"SELECT COUNT(*) AS "count!: i64", COALESCE(SUM(size), 0) AS "bytes!: i64" FROM blobs"#
    )
    .fetch_one(db)
    .await
}

pub(crate) struct GcBlobRow {
    pub digest: String,
    pub size: i64,
    pub created_at: String,
    pub last_linked_at: Option<String>,
}

pub(crate) async fn all_for_gc(tx: &mut Tx) -> sqlx::Result<Vec<GcBlobRow>> {
    sqlx::query_as!(
        GcBlobRow,
        r#"SELECT b.digest, b.size, b.created_at,
                  (SELECT MAX(rb.created_at) FROM repository_blobs rb WHERE rb.digest = b.digest) AS "last_linked_at?: String"
           FROM blobs b WHERE NOT EXISTS (SELECT 1 FROM gc_sweep g WHERE g.digest = b.digest)"#
    )
    .fetch_all(&mut **tx)
    .await
}

/// Hides a blob from every repository and queues its storage object for deletion.
pub(crate) async fn mark_for_sweep(tx: &mut Tx, digest: &str, now: &str) -> sqlx::Result<()> {
    sqlx::query!("DELETE FROM repository_blobs WHERE digest = ?", digest).execute(&mut **tx).await?;
    sqlx::query!("INSERT OR IGNORE INTO gc_sweep (digest, marked_at) VALUES (?, ?)", digest, now)
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

pub(crate) async fn pending_sweep(db: impl SqliteExecutor<'_>) -> sqlx::Result<Vec<(String, i64)>> {
    let rows = sqlx::query!(
        r#"SELECT g.digest, COALESCE(b.size, 0) AS "size!: i64" FROM gc_sweep g LEFT JOIN blobs b ON b.digest = g.digest
           ORDER BY g.digest"#
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|r| (r.digest, r.size)).collect())
}

/// Claims a queued sweep; false if an upload rescued the blob meanwhile.
pub(crate) async fn take_sweep(tx: &mut Tx, digest: &str) -> sqlx::Result<bool> {
    let r = sqlx::query!("DELETE FROM gc_sweep WHERE digest = ?", digest).execute(&mut **tx).await?;
    Ok(r.rows_affected() == 1)
}

pub(crate) async fn delete_row(tx: &mut Tx, digest: &str) -> sqlx::Result<()> {
    sqlx::query!("DELETE FROM repository_blobs WHERE digest = ?", digest).execute(&mut **tx).await?;
    sqlx::query!("DELETE FROM blobs WHERE digest = ?", digest).execute(&mut **tx).await.map(|_| ())
}
