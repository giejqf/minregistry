//! Background jobs of `serve`: expiring upload sessions, pruning web
//! sessions, audit retention and scheduled garbage collection.

use std::{collections::HashSet, str::FromStr, time::Duration};

use crate::{
    app::AppState,
    audit::{action, AuditEvent, Outcome},
    auth::session::SqliteSessionStore,
    db,
    gc::{self, GcOptions},
};

pub(crate) fn parse_cron(expr: &str) -> Result<croner::Cron, String> {
    croner::Cron::from_str(expr).map_err(|e| format!("invalid cron expression: {e}"))
}

pub(crate) fn spawn_all(state: &AppState) {
    let s = state.clone();
    tokio::spawn(async move {
        let ttl = s.cfg.core.upload_ttl;
        let period = (ttl / 4).clamp(Duration::from_secs(10), Duration::from_secs(300));
        let mut tick = tokio::time::interval(period);
        loop {
            tick.tick().await;
            if let Err(e) = expire_uploads(&s, ttl).await {
                tracing::warn!(error = %e, "upload cleanup failed");
            }
        }
    });

    let s = state.clone();
    tokio::spawn(async move {
        let store = SqliteSessionStore::new(s.db.clone());
        let mut tick = tokio::time::interval(Duration::from_secs(3600));
        loop {
            tick.tick().await;
            if let Err(e) = store.delete_expired().await {
                tracing::warn!(error = %e, "session cleanup failed");
            }
        }
    });

    if state.cfg.core.audit_retention_days > 0 {
        let s = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(6 * 3600));
            loop {
                tick.tick().await;
                prune_audit(&s).await;
            }
        });
    }

    if let Some(expr) = state.cfg.core.gc_cron.clone() {
        match parse_cron(&expr) {
            Ok(cron) => {
                let s = state.clone();
                tokio::spawn(async move { scheduled_gc(s, cron).await });
            }
            Err(e) => tracing::error!(error = %e, "MINREGISTRY_GC_CRON is invalid; scheduled GC disabled"),
        }
    }
}

/// Removes idle upload sessions and stray staging files.
pub(crate) async fn expire_uploads(state: &AppState, ttl: Duration) -> anyhow::Result<()> {
    let cutoff = crate::time::before_now(ttl);
    for uuid in db::uploads::delete_expired(&state.db.write, &cutoff).await? {
        tracing::info!(%uuid, "expired upload session");
        state.uploads.discard(&uuid).await;
    }
    let active: HashSet<String> = db::uploads::all_ids(&state.db.read).await?.into_iter().collect();
    let mut dir = tokio::fs::read_dir(state.uploads.dir()).await?;
    while let Some(entry) = dir.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if active.contains(&name) {
            continue;
        }
        let stale = entry
            .metadata()
            .await
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age >= ttl);
        if stale {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
    Ok(())
}

async fn prune_audit(state: &AppState) {
    let days = state.cfg.core.audit_retention_days;
    let cutoff = crate::time::before_now(Duration::from_secs(u64::from(days) * 86_400));
    match db::audit::prune(&state.db.write, &cutoff).await {
        Ok(0) => {}
        Ok(n) => {
            state
                .audit
                .record(
                    AuditEvent::new(action::AUDIT_PRUNE)
                        .principal_name("system")
                        .detail("deleted", n)
                        .detail("before", cutoff)
                        .detail("retention_days", days),
                )
                .await
        }
        Err(e) => tracing::warn!(error = %e, "audit retention failed"),
    }
}

async fn scheduled_gc(state: AppState, cron: croner::Cron) {
    loop {
        let now = chrono::Utc::now();
        let Ok(next) = cron.find_next_occurrence(&now, false) else {
            tracing::error!("GC schedule has no future occurrence; scheduled GC stopped");
            return;
        };
        let wait = (next - now).to_std().unwrap_or(Duration::from_secs(1));
        tokio::time::sleep(wait).await;
        let opts = GcOptions { dry_run: false, delete_untagged: false, min_age: state.cfg.core.gc_min_age };
        let _guard = state.gc_lock.lock().await;
        let event = AuditEvent::new(action::GC_RUN).principal_name("system").detail("trigger", "schedule");
        match gc::run(&state.db, state.storage.as_ref(), &opts).await {
            Ok(report) => {
                tracing::info!(?report, "scheduled garbage collection finished");
                state.audit.record(event.detail("report", serde_json::to_value(&report).unwrap_or_default())).await;
            }
            Err(e) => {
                tracing::error!(error = %e, "scheduled garbage collection failed");
                state.audit.record(event.outcome(Outcome::Error).detail("error", e.to_string())).await;
            }
        }
    }
}
