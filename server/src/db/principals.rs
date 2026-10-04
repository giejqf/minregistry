//! `principals`: GitHub-backed admins and token-only identities.

use sqlx::SqliteExecutor;

#[derive(Clone, Debug)]
pub(crate) struct PrincipalRow {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub display_name: String,
    pub enabled: bool,
    pub created_at: String,
}

pub(crate) const KIND_GITHUB: &str = "github";
pub(crate) const KIND_IDENTITY: &str = "identity";

pub(crate) async fn by_name(db: impl SqliteExecutor<'_>, name: &str) -> sqlx::Result<Option<PrincipalRow>> {
    sqlx::query_as!(
        PrincipalRow,
        r#"SELECT id, kind, name, display_name, enabled AS "enabled: bool", created_at
           FROM principals WHERE name = ?"#,
        name
    )
    .fetch_optional(db)
    .await
}

pub(crate) async fn by_id(db: impl SqliteExecutor<'_>, id: i64) -> sqlx::Result<Option<PrincipalRow>> {
    sqlx::query_as!(
        PrincipalRow,
        r#"SELECT id, kind, name, display_name, enabled AS "enabled: bool", created_at
           FROM principals WHERE id = ?"#,
        id
    )
    .fetch_optional(db)
    .await
}

pub(crate) async fn by_github_id(db: impl SqliteExecutor<'_>, github_id: i64) -> sqlx::Result<Option<PrincipalRow>> {
    sqlx::query_as!(
        PrincipalRow,
        r#"SELECT id, kind, name, display_name, enabled AS "enabled: bool", created_at
           FROM principals WHERE github_id = ?"#,
        github_id
    )
    .fetch_optional(db)
    .await
}

pub(crate) async fn list(db: impl SqliteExecutor<'_>) -> sqlx::Result<Vec<PrincipalRow>> {
    sqlx::query_as!(
        PrincipalRow,
        r#"SELECT id, kind, name, display_name, enabled AS "enabled: bool", created_at
           FROM principals ORDER BY kind, name"#
    )
    .fetch_all(db)
    .await
}

pub(crate) async fn insert(
    db: impl SqliteExecutor<'_>,
    kind: &str,
    name: &str,
    github_id: Option<i64>,
    display_name: &str,
    now: &str,
) -> sqlx::Result<i64> {
    sqlx::query_scalar!(
        r#"INSERT INTO principals (kind, name, github_id, display_name, enabled, created_at)
           VALUES (?, ?, ?, ?, 1, ?) RETURNING id AS "id!: i64""#,
        kind,
        name,
        github_id,
        display_name,
        now
    )
    .fetch_one(db)
    .await
}

/// Keeps a GitHub principal in sync with the account (logins can be renamed).
pub(crate) async fn update_github(
    db: impl SqliteExecutor<'_>,
    id: i64,
    name: &str,
    display_name: &str,
) -> sqlx::Result<()> {
    sqlx::query!("UPDATE principals SET name = ?, display_name = ? WHERE id = ?", name, display_name, id)
        .execute(db)
        .await
        .map(|_| ())
}

pub(crate) async fn update(
    db: impl SqliteExecutor<'_>,
    id: i64,
    enabled: Option<bool>,
    display_name: Option<&str>,
) -> sqlx::Result<u64> {
    sqlx::query!(
        "UPDATE principals SET enabled = COALESCE(?, enabled), display_name = COALESCE(?, display_name) WHERE id = ?",
        enabled,
        display_name,
        id
    )
    .execute(db)
    .await
    .map(|r| r.rows_affected())
}
