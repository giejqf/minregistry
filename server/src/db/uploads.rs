//! `uploads`: in-flight blob upload sessions (data is staged on local disk).

use sqlx::SqliteExecutor;

#[derive(Clone, Debug)]
pub(crate) struct UploadRow {
    pub repository_id: i64,
    pub principal_id: i64,
    pub offset: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct UploadSummaryRow {
    pub uuid: String,
    pub repository: String,
    pub principal: String,
    pub offset: i64,
    pub started_at: String,
    pub last_activity_at: String,
}

pub(crate) async fn insert(
    db: impl SqliteExecutor<'_>,
    uuid: &str,
    repository_id: i64,
    principal_id: i64,
    now: &str,
) -> sqlx::Result<()> {
    sqlx::query!(
        r#"INSERT INTO uploads (uuid, repository_id, principal_id, "offset", started_at, last_activity_at)
           VALUES (?, ?, ?, 0, ?, ?)"#,
        uuid,
        repository_id,
        principal_id,
        now,
        now
    )
    .execute(db)
    .await
    .map(|_| ())
}

pub(crate) async fn get(db: impl SqliteExecutor<'_>, uuid: &str) -> sqlx::Result<Option<UploadRow>> {
    sqlx::query_as!(
        UploadRow,
        r#"SELECT repository_id, principal_id, "offset" AS "offset!: i64" FROM uploads WHERE uuid = ?"#,
        uuid
    )
    .fetch_optional(db)
    .await
}

pub(crate) async fn set_offset(db: impl SqliteExecutor<'_>, uuid: &str, offset: i64, now: &str) -> sqlx::Result<()> {
    sqlx::query!(r#"UPDATE uploads SET "offset" = ?, last_activity_at = ? WHERE uuid = ?"#, offset, now, uuid)
        .execute(db)
        .await
        .map(|_| ())
}

pub(crate) async fn delete(db: impl SqliteExecutor<'_>, uuid: &str) -> sqlx::Result<bool> {
    let r = sqlx::query!("DELETE FROM uploads WHERE uuid = ?", uuid).execute(db).await?;
    Ok(r.rows_affected() == 1)
}

/// Deletes sessions idle since before `before`, returning their ids.
pub(crate) async fn delete_expired(db: impl SqliteExecutor<'_>, before: &str) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar!("DELETE FROM uploads WHERE last_activity_at < ? RETURNING uuid", before).fetch_all(db).await
}

pub(crate) async fn all_ids(db: impl SqliteExecutor<'_>) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar!("SELECT uuid FROM uploads").fetch_all(db).await
}

pub(crate) async fn list(db: impl SqliteExecutor<'_>) -> sqlx::Result<Vec<UploadSummaryRow>> {
    sqlx::query_as!(
        UploadSummaryRow,
        r#"SELECT u.uuid, r.name AS repository, p.name AS principal, u."offset" AS "offset!: i64",
                  u.started_at, u.last_activity_at
           FROM uploads u JOIN repositories r ON r.id = u.repository_id JOIN principals p ON p.id = u.principal_id
           ORDER BY u.last_activity_at DESC"#
    )
    .fetch_all(db)
    .await
}
