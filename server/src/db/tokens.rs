//! `tokens`: registry credentials. Only the SHA-256 of the secret is stored.

use sqlx::SqliteExecutor;

#[derive(Clone, Debug)]
pub(crate) struct TokenRow {
    pub id: i64,
    pub name: String,
    pub prefix: String,
    pub created_by: Option<String>,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
    pub last_used_at: Option<String>,
}

/// A credential candidate for authentication.
pub(crate) struct ActiveToken {
    pub id: i64,
    pub hash: String,
}

/// Non-revoked, non-expired tokens of a principal.
pub(crate) async fn active_for(
    db: impl SqliteExecutor<'_>,
    principal_id: i64,
    now: &str,
) -> sqlx::Result<Vec<ActiveToken>> {
    sqlx::query_as!(
        ActiveToken,
        "SELECT id, hash FROM tokens
         WHERE principal_id = ? AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > ?)",
        principal_id,
        now
    )
    .fetch_all(db)
    .await
}

pub(crate) async fn list_for(db: impl SqliteExecutor<'_>, principal_id: i64) -> sqlx::Result<Vec<TokenRow>> {
    sqlx::query_as!(
        TokenRow,
        r#"SELECT t.id, t.name, t.prefix, p.name AS "created_by?", t.created_at,
                  t.expires_at, t.revoked_at, t.last_used_at
           FROM tokens t LEFT JOIN principals p ON p.id = t.created_by
           WHERE t.principal_id = ? ORDER BY t.id DESC"#,
        principal_id
    )
    .fetch_all(db)
    .await
}

pub(crate) async fn get(db: impl SqliteExecutor<'_>, principal_id: i64, id: i64) -> sqlx::Result<Option<TokenRow>> {
    sqlx::query_as!(
        TokenRow,
        r#"SELECT t.id, t.name, t.prefix, p.name AS "created_by?", t.created_at,
                  t.expires_at, t.revoked_at, t.last_used_at
           FROM tokens t LEFT JOIN principals p ON p.id = t.created_by
           WHERE t.principal_id = ? AND t.id = ?"#,
        principal_id,
        id
    )
    .fetch_optional(db)
    .await
}

/// Number of usable tokens per principal.
pub(crate) async fn active_counts(db: impl SqliteExecutor<'_>, now: &str) -> sqlx::Result<Vec<(i64, i64)>> {
    let rows = sqlx::query!(
        r#"SELECT principal_id, COUNT(*) AS "count!: i64" FROM tokens
           WHERE revoked_at IS NULL AND (expires_at IS NULL OR expires_at > ?)
           GROUP BY principal_id"#,
        now
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|r| (r.principal_id, r.count)).collect())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert(
    db: impl SqliteExecutor<'_>,
    principal_id: i64,
    name: &str,
    hash: &str,
    prefix: &str,
    created_by: i64,
    now: &str,
    expires_at: Option<&str>,
) -> sqlx::Result<i64> {
    sqlx::query_scalar!(
        r#"INSERT INTO tokens (principal_id, name, hash, prefix, created_by, created_at, expires_at)
           VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id AS "id!: i64""#,
        principal_id,
        name,
        hash,
        prefix,
        created_by,
        now,
        expires_at
    )
    .fetch_one(db)
    .await
}

/// Revokes a token; returns false if it was unknown or already revoked.
pub(crate) async fn revoke(db: impl SqliteExecutor<'_>, principal_id: i64, id: i64, now: &str) -> sqlx::Result<bool> {
    let r = sqlx::query!(
        "UPDATE tokens SET revoked_at = ? WHERE id = ? AND principal_id = ? AND revoked_at IS NULL",
        now,
        id,
        principal_id
    )
    .execute(db)
    .await?;
    Ok(r.rows_affected() == 1)
}

pub(crate) async fn touch(db: impl SqliteExecutor<'_>, id: i64, now: &str) -> sqlx::Result<()> {
    sqlx::query!("UPDATE tokens SET last_used_at = ? WHERE id = ?", now, id).execute(db).await.map(|_| ())
}
