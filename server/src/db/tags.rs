//! `tags`.

use sqlx::SqliteExecutor;

use super::Tx;

#[derive(Clone, Debug)]
pub(crate) struct TagRow {
    pub name: String,
    pub manifest_digest: String,
    pub updated_at: String,
    pub updated_by: Option<String>,
}

pub(crate) async fn resolve(
    db: impl SqliteExecutor<'_>,
    repository_id: i64,
    tag: &str,
) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar!("SELECT manifest_digest FROM tags WHERE repository_id = ? AND name = ?", repository_id, tag)
        .fetch_optional(db)
        .await
}

pub(crate) async fn upsert(
    tx: &mut Tx,
    repository_id: i64,
    tag: &str,
    digest: &str,
    principal_id: i64,
    now: &str,
) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT INTO tags (repository_id, name, manifest_digest, updated_at, updated_by) VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (repository_id, name) DO UPDATE SET
             manifest_digest = excluded.manifest_digest, updated_at = excluded.updated_at, updated_by = excluded.updated_by",
        repository_id,
        tag,
        digest,
        now,
        principal_id
    )
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

pub(crate) async fn delete(db: impl SqliteExecutor<'_>, repository_id: i64, tag: &str) -> sqlx::Result<bool> {
    let r =
        sqlx::query!("DELETE FROM tags WHERE repository_id = ? AND name = ?", repository_id, tag).execute(db).await?;
    Ok(r.rows_affected() == 1)
}

/// Tag names in lexical order, after `last`, at most `limit`.
pub(crate) async fn names(
    db: impl SqliteExecutor<'_>,
    repository_id: i64,
    last: Option<&str>,
    limit: i64,
) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar!(
        "SELECT name FROM tags WHERE repository_id = ?1 AND (?2 IS NULL OR name > ?2) ORDER BY name LIMIT ?3",
        repository_id,
        last,
        limit
    )
    .fetch_all(db)
    .await
}

pub(crate) async fn list(db: impl SqliteExecutor<'_>, repository_id: i64) -> sqlx::Result<Vec<TagRow>> {
    sqlx::query_as!(
        TagRow,
        r#"SELECT t.name, t.manifest_digest, t.updated_at, p.name AS "updated_by?"
           FROM tags t LEFT JOIN principals p ON p.id = t.updated_by
           WHERE t.repository_id = ? ORDER BY t.name"#,
        repository_id
    )
    .fetch_all(db)
    .await
}

pub(crate) async fn roots_for_gc(tx: &mut Tx) -> sqlx::Result<Vec<(i64, String)>> {
    let rows = sqlx::query!("SELECT DISTINCT repository_id, manifest_digest FROM tags").fetch_all(&mut **tx).await?;
    Ok(rows.into_iter().map(|r| (r.repository_id, r.manifest_digest)).collect())
}
