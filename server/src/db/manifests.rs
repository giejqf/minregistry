//! `manifests` and `manifest_refs`.

use sqlx::SqliteExecutor;

use super::Tx;

#[derive(Clone, Debug)]
pub(crate) struct ManifestRow {
    pub digest: String,
    pub media_type: String,
    pub size: i64,
    pub subject_digest: Option<String>,
    pub artifact_type: Option<String>,
    pub annotations: Option<String>,
    pub platforms: Option<String>,
    pub created_at: String,
    pub pushed_by: Option<String>,
}

pub(crate) struct NewManifest<'a> {
    pub repository_id: i64,
    pub digest: &'a str,
    pub media_type: &'a str,
    pub size: i64,
    pub subject_digest: Option<&'a str>,
    pub artifact_type: Option<&'a str>,
    pub annotations: Option<&'a str>,
    pub platforms: Option<&'a str>,
    pub pushed_by: i64,
    pub now: &'a str,
}

pub(crate) struct ManifestHead {
    pub media_type: String,
    pub size: i64,
}

pub(crate) async fn head(
    db: impl SqliteExecutor<'_>,
    repository_id: i64,
    digest: &str,
) -> sqlx::Result<Option<ManifestHead>> {
    sqlx::query_as!(
        ManifestHead,
        "SELECT media_type, size FROM manifests WHERE repository_id = ? AND digest = ?",
        repository_id,
        digest
    )
    .fetch_optional(db)
    .await
}

pub(crate) async fn list(db: impl SqliteExecutor<'_>, repository_id: i64) -> sqlx::Result<Vec<ManifestRow>> {
    sqlx::query_as!(
        ManifestRow,
        r#"SELECT m.digest, m.media_type, m.size, m.subject_digest, m.artifact_type, m.annotations, m.platforms,
                  m.created_at, p.name AS "pushed_by?"
           FROM manifests m LEFT JOIN principals p ON p.id = m.pushed_by
           WHERE m.repository_id = ? ORDER BY m.created_at DESC, m.digest"#,
        repository_id
    )
    .fetch_all(db)
    .await
}

/// (index, child manifest) pairs of a repository.
pub(crate) async fn children(db: impl SqliteExecutor<'_>, repository_id: i64) -> sqlx::Result<Vec<(String, String)>> {
    let rows = sqlx::query!(
        "SELECT manifest_digest, child_digest FROM manifest_refs
         WHERE repository_id = ? AND kind = 'manifest' ORDER BY manifest_digest, child_digest",
        repository_id
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|r| (r.manifest_digest, r.child_digest)).collect())
}

/// Inserts a manifest; false if this repository already had it.
pub(crate) async fn insert(tx: &mut Tx, m: &NewManifest<'_>) -> sqlx::Result<bool> {
    let r = sqlx::query!(
        "INSERT INTO manifests (repository_id, digest, media_type, size, subject_digest, artifact_type,
                                annotations, platforms, created_at, pushed_by)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (repository_id, digest) DO NOTHING",
        m.repository_id,
        m.digest,
        m.media_type,
        m.size,
        m.subject_digest,
        m.artifact_type,
        m.annotations,
        m.platforms,
        m.now,
        m.pushed_by
    )
    .execute(&mut **tx)
    .await?;
    Ok(r.rows_affected() == 1)
}

pub(crate) async fn insert_ref(
    tx: &mut Tx,
    repository_id: i64,
    manifest: &str,
    child: &str,
    kind: &str,
) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT OR IGNORE INTO manifest_refs (repository_id, manifest_digest, child_digest, kind) VALUES (?, ?, ?, ?)",
        repository_id,
        manifest,
        child,
        kind
    )
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

pub(crate) async fn exists(db: impl SqliteExecutor<'_>, repository_id: i64, digest: &str) -> sqlx::Result<bool> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM manifests WHERE repository_id = ? AND digest = ?) AS "e!: bool""#,
        repository_id,
        digest
    )
    .fetch_one(db)
    .await
}

/// Deletes a manifest (its tags and refs cascade); false if it did not exist.
pub(crate) async fn delete(db: impl SqliteExecutor<'_>, repository_id: i64, digest: &str) -> sqlx::Result<bool> {
    let r = sqlx::query!("DELETE FROM manifests WHERE repository_id = ? AND digest = ?", repository_id, digest)
        .execute(db)
        .await?;
    Ok(r.rows_affected() == 1)
}

pub(crate) struct ReferrerRow {
    pub digest: String,
    pub media_type: String,
    pub size: i64,
    pub artifact_type: Option<String>,
    pub annotations: Option<String>,
}

pub(crate) async fn referrers(
    db: impl SqliteExecutor<'_>,
    repository_id: i64,
    subject: &str,
    artifact_type: Option<&str>,
) -> sqlx::Result<Vec<ReferrerRow>> {
    sqlx::query_as!(
        ReferrerRow,
        "SELECT digest, media_type, size, artifact_type, annotations FROM manifests
         WHERE repository_id = ?1 AND subject_digest = ?2 AND (?3 IS NULL OR artifact_type = ?3)
         ORDER BY created_at, digest",
        repository_id,
        subject,
        artifact_type
    )
    .fetch_all(db)
    .await
}

pub(crate) async fn count(db: impl SqliteExecutor<'_>) -> sqlx::Result<i64> {
    sqlx::query_scalar!(r#"SELECT COUNT(*) AS "c!: i64" FROM manifests"#).fetch_one(db).await
}

pub(crate) struct GcManifestRow {
    pub repository_id: i64,
    pub digest: String,
    pub subject_digest: Option<String>,
    pub created_at: String,
}

pub(crate) async fn all_for_gc(tx: &mut Tx) -> sqlx::Result<Vec<GcManifestRow>> {
    sqlx::query_as!(GcManifestRow, "SELECT repository_id, digest, subject_digest, created_at FROM manifests")
        .fetch_all(&mut **tx)
        .await
}

pub(crate) struct GcRefRow {
    pub repository_id: i64,
    pub manifest_digest: String,
    pub child_digest: String,
    pub kind: String,
}

pub(crate) async fn refs_for_gc(tx: &mut Tx) -> sqlx::Result<Vec<GcRefRow>> {
    sqlx::query_as!(GcRefRow, "SELECT repository_id, manifest_digest, child_digest, kind FROM manifest_refs")
        .fetch_all(&mut **tx)
        .await
}
