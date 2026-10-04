//! `permissions`: per-repository, per-principal access levels.

use sqlx::SqliteExecutor;

#[derive(Clone, Debug)]
pub(crate) struct RepoPermissionRow {
    pub principal_id: i64,
    pub principal_name: String,
    pub principal_kind: String,
    pub level: String,
    pub granted_by: Option<String>,
    pub granted_at: String,
}

#[derive(Clone, Debug)]
pub(crate) struct PrincipalPermissionRow {
    pub repository_id: i64,
    pub repository_name: String,
    pub level: String,
    pub granted_by: Option<String>,
    pub granted_at: String,
}

pub(crate) async fn level(
    db: impl SqliteExecutor<'_>,
    principal_id: i64,
    repository_id: i64,
) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar!(
        "SELECT level FROM permissions WHERE principal_id = ? AND repository_id = ?",
        principal_id,
        repository_id
    )
    .fetch_optional(db)
    .await
}

pub(crate) async fn for_repository(
    db: impl SqliteExecutor<'_>,
    repository_id: i64,
) -> sqlx::Result<Vec<RepoPermissionRow>> {
    sqlx::query_as!(
        RepoPermissionRow,
        r#"SELECT pe.principal_id, p.name AS principal_name, p.kind AS principal_kind, pe.level,
                  g.name AS "granted_by?", pe.granted_at
           FROM permissions pe
           JOIN principals p ON p.id = pe.principal_id
           LEFT JOIN principals g ON g.id = pe.granted_by
           WHERE pe.repository_id = ? ORDER BY p.name"#,
        repository_id
    )
    .fetch_all(db)
    .await
}

pub(crate) async fn for_principal(
    db: impl SqliteExecutor<'_>,
    principal_id: i64,
) -> sqlx::Result<Vec<PrincipalPermissionRow>> {
    sqlx::query_as!(
        PrincipalPermissionRow,
        r#"SELECT pe.repository_id, r.name AS repository_name, pe.level, g.name AS "granted_by?", pe.granted_at
           FROM permissions pe
           JOIN repositories r ON r.id = pe.repository_id AND r.deleted_at IS NULL
           LEFT JOIN principals g ON g.id = pe.granted_by
           WHERE pe.principal_id = ? ORDER BY r.name"#,
        principal_id
    )
    .fetch_all(db)
    .await
}

pub(crate) async fn upsert(
    db: impl SqliteExecutor<'_>,
    principal_id: i64,
    repository_id: i64,
    level: &str,
    granted_by: i64,
    now: &str,
) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT INTO permissions (principal_id, repository_id, level, granted_by, granted_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (principal_id, repository_id)
         DO UPDATE SET level = excluded.level, granted_by = excluded.granted_by, granted_at = excluded.granted_at",
        principal_id,
        repository_id,
        level,
        granted_by,
        now
    )
    .execute(db)
    .await
    .map(|_| ())
}

pub(crate) async fn delete(db: impl SqliteExecutor<'_>, principal_id: i64, repository_id: i64) -> sqlx::Result<bool> {
    let r = sqlx::query!(
        "DELETE FROM permissions WHERE principal_id = ? AND repository_id = ?",
        principal_id,
        repository_id
    )
    .execute(db)
    .await?;
    Ok(r.rows_affected() == 1)
}
