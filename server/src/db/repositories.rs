//! `repositories`. Deleting a repository is a soft delete: content rows are
//! removed (GC reclaims blobs) and the row is kept for history; pushing to the
//! same name later revives it.

use sqlx::SqliteExecutor;

use super::Tx;

#[derive(Clone, Debug)]
pub(crate) struct RepositoryRow {
    pub id: i64,
    pub name: String,
    pub created_by: Option<i64>,
    pub created_at: String,
}

#[derive(Clone, Debug)]
pub(crate) struct RepositorySummaryRow {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub created_by: Option<String>,
    pub tag_count: i64,
    pub manifest_count: i64,
    pub last_push_at: Option<String>,
}

pub(crate) async fn by_name(db: impl SqliteExecutor<'_>, name: &str) -> sqlx::Result<Option<RepositoryRow>> {
    sqlx::query_as!(
        RepositoryRow,
        "SELECT id, name, created_by, created_at FROM repositories WHERE name = ? AND deleted_at IS NULL",
        name
    )
    .fetch_optional(db)
    .await
}

pub(crate) async fn by_id(db: impl SqliteExecutor<'_>, id: i64) -> sqlx::Result<Option<RepositoryRow>> {
    sqlx::query_as!(
        RepositoryRow,
        "SELECT id, name, created_by, created_at FROM repositories WHERE id = ? AND deleted_at IS NULL",
        id
    )
    .fetch_optional(db)
    .await
}

pub(crate) enum Created {
    /// The repository was created (or revived) by this call.
    New(i64),
    /// Someone else created it concurrently.
    Existing,
}

pub(crate) async fn create_or_revive(tx: &mut Tx, name: &str, created_by: i64, now: &str) -> sqlx::Result<Created> {
    let existing = sqlx::query!(r#"SELECT id AS "id!: i64", deleted_at FROM repositories WHERE name = ?"#, name)
        .fetch_optional(&mut **tx)
        .await?;
    match existing {
        Some(row) if row.deleted_at.is_none() => Ok(Created::Existing),
        Some(row) => {
            sqlx::query!(
                "UPDATE repositories SET deleted_at = NULL, created_by = ?, created_at = ? WHERE id = ?",
                created_by,
                now,
                row.id
            )
            .execute(&mut **tx)
            .await?;
            Ok(Created::New(row.id))
        }
        None => {
            let id = sqlx::query_scalar!(
                r#"INSERT INTO repositories (name, created_by, created_at) VALUES (?, ?, ?) RETURNING id AS "id!: i64""#,
                name,
                created_by,
                now
            )
            .fetch_one(&mut **tx)
            .await?;
            Ok(Created::New(id))
        }
    }
}

pub(crate) async fn list(
    db: impl SqliteExecutor<'_>,
    query: Option<&str>,
    limit: i64,
    offset: i64,
) -> sqlx::Result<Vec<RepositorySummaryRow>> {
    sqlx::query_as!(
        RepositorySummaryRow,
        r#"SELECT r.id, r.name, r.created_at, p.name AS "created_by?",
                  (SELECT COUNT(*) FROM tags t WHERE t.repository_id = r.id) AS "tag_count!: i64",
                  (SELECT COUNT(*) FROM manifests m WHERE m.repository_id = r.id) AS "manifest_count!: i64",
                  (SELECT MAX(m.created_at) FROM manifests m WHERE m.repository_id = r.id) AS "last_push_at?: String"
           FROM repositories r LEFT JOIN principals p ON p.id = r.created_by
           WHERE r.deleted_at IS NULL AND (?1 IS NULL OR instr(r.name, ?1) > 0)
           ORDER BY r.name LIMIT ?2 OFFSET ?3"#,
        query,
        limit,
        offset
    )
    .fetch_all(db)
    .await
}

pub(crate) async fn count(db: impl SqliteExecutor<'_>, query: Option<&str>) -> sqlx::Result<i64> {
    sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "count!: i64" FROM repositories
           WHERE deleted_at IS NULL AND (?1 IS NULL OR instr(name, ?1) > 0)"#,
        query
    )
    .fetch_one(db)
    .await
}

/// Removes all content of a repository and marks it deleted. Returns the
/// upload sessions that were cancelled (their staging files must be removed).
pub(crate) async fn soft_delete(tx: &mut Tx, id: i64, now: &str) -> sqlx::Result<Vec<String>> {
    sqlx::query!("DELETE FROM tags WHERE repository_id = ?", id).execute(&mut **tx).await?;
    sqlx::query!("DELETE FROM manifests WHERE repository_id = ?", id).execute(&mut **tx).await?;
    sqlx::query!("DELETE FROM repository_blobs WHERE repository_id = ?", id).execute(&mut **tx).await?;
    sqlx::query!("DELETE FROM permissions WHERE repository_id = ?", id).execute(&mut **tx).await?;
    let uploads = sqlx::query_scalar!("DELETE FROM uploads WHERE repository_id = ? RETURNING uuid", id)
        .fetch_all(&mut **tx)
        .await?;
    sqlx::query!("UPDATE repositories SET deleted_at = ? WHERE id = ?", now, id).execute(&mut **tx).await?;
    Ok(uploads)
}
