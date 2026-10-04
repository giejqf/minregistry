//! `audit_events`: append-only. The only DELETE is the retention job.

use sqlx::SqliteExecutor;

pub(crate) struct NewAuditEvent<'a> {
    pub ts: &'a str,
    pub principal_id: Option<i64>,
    pub principal_name: Option<&'a str>,
    pub action: &'a str,
    pub repository: Option<&'a str>,
    pub reference: Option<&'a str>,
    pub digest: Option<&'a str>,
    pub client_ip: Option<&'a str>,
    pub user_agent: Option<&'a str>,
    pub outcome: &'a str,
    pub detail: &'a str,
}

pub(crate) async fn insert(db: impl SqliteExecutor<'_>, e: &NewAuditEvent<'_>) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT INTO audit_events (ts, principal_id, principal_name, action, repository, reference, digest,
                                   client_ip, user_agent, outcome, detail)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        e.ts,
        e.principal_id,
        e.principal_name,
        e.action,
        e.repository,
        e.reference,
        e.digest,
        e.client_ip,
        e.user_agent,
        e.outcome,
        e.detail
    )
    .execute(db)
    .await
    .map(|_| ())
}

#[derive(Clone, Debug)]
pub(crate) struct AuditRow {
    pub id: i64,
    pub ts: String,
    pub principal_id: Option<i64>,
    pub principal_name: Option<String>,
    pub action: String,
    pub repository: Option<String>,
    pub reference: Option<String>,
    pub digest: Option<String>,
    pub client_ip: Option<String>,
    pub user_agent: Option<String>,
    pub outcome: String,
    pub detail: String,
}

#[derive(Default)]
pub(crate) struct AuditFilter<'a> {
    pub principal: Option<&'a str>,
    pub repository: Option<&'a str>,
    pub action: Option<&'a str>,
    pub outcome: Option<&'a str>,
    pub from: Option<&'a str>,
    pub to: Option<&'a str>,
    pub before_id: Option<i64>,
}

/// Newest first.
pub(crate) async fn list(db: impl SqliteExecutor<'_>, f: &AuditFilter<'_>, limit: i64) -> sqlx::Result<Vec<AuditRow>> {
    sqlx::query_as!(
        AuditRow,
        "SELECT id, ts, principal_id, principal_name, action, repository, reference, digest, client_ip, user_agent,
                outcome, detail
         FROM audit_events
         WHERE (?1 IS NULL OR principal_name = ?1)
           AND (?2 IS NULL OR repository = ?2)
           AND (?3 IS NULL OR action = ?3)
           AND (?4 IS NULL OR outcome = ?4)
           AND (?5 IS NULL OR ts >= ?5)
           AND (?6 IS NULL OR ts < ?6)
           AND (?7 IS NULL OR id < ?7)
         ORDER BY id DESC LIMIT ?8",
        f.principal,
        f.repository,
        f.action,
        f.outcome,
        f.from,
        f.to,
        f.before_id,
        limit
    )
    .fetch_all(db)
    .await
}

/// Retention: deletes events older than `before`.
pub(crate) async fn prune(db: impl SqliteExecutor<'_>, before: &str) -> sqlx::Result<u64> {
    let r = sqlx::query!("DELETE FROM audit_events WHERE ts < ?", before).execute(db).await?;
    Ok(r.rows_affected())
}
