//! `sessions`: server-side store for admin web sessions.

use sqlx::SqliteExecutor;

pub(crate) struct SessionRow {
    pub data: String,
    pub expiry_date: i64,
}

pub(crate) async fn load(db: impl SqliteExecutor<'_>, id: &str, now: i64) -> sqlx::Result<Option<SessionRow>> {
    sqlx::query_as!(SessionRow, "SELECT data, expiry_date FROM sessions WHERE id = ? AND expiry_date > ?", id, now)
        .fetch_optional(db)
        .await
}

/// Inserts a new session; fails with a unique violation on id collision.
pub(crate) async fn insert(db: impl SqliteExecutor<'_>, id: &str, data: &str, expiry: i64) -> sqlx::Result<()> {
    sqlx::query!("INSERT INTO sessions (id, data, expiry_date) VALUES (?, ?, ?)", id, data, expiry)
        .execute(db)
        .await
        .map(|_| ())
}

pub(crate) async fn upsert(db: impl SqliteExecutor<'_>, id: &str, data: &str, expiry: i64) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT INTO sessions (id, data, expiry_date) VALUES (?, ?, ?)
         ON CONFLICT (id) DO UPDATE SET data = excluded.data, expiry_date = excluded.expiry_date",
        id,
        data,
        expiry
    )
    .execute(db)
    .await
    .map(|_| ())
}

pub(crate) async fn delete(db: impl SqliteExecutor<'_>, id: &str) -> sqlx::Result<()> {
    sqlx::query!("DELETE FROM sessions WHERE id = ?", id).execute(db).await.map(|_| ())
}

pub(crate) async fn delete_expired(db: impl SqliteExecutor<'_>, now: i64) -> sqlx::Result<u64> {
    let r = sqlx::query!("DELETE FROM sessions WHERE expiry_date <= ?", now).execute(db).await?;
    Ok(r.rows_affected())
}
