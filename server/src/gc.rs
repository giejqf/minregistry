//! Mark-and-sweep garbage collection of unreferenced blobs (docs/adr/0003).
//!
//! 1. **Mark** (one write transaction, i.e. the global write lock): roots are
//!    tagged manifests, every untagged manifest unless `delete_untagged`, and
//!    anything younger than `min_age`. Reachability follows index children and
//!    referrers (`subject`). Unreferenced blobs older than `min_age` are hidden
//!    from all repositories and queued in `gc_sweep`.
//! 2. **Sweep**: each queued digest is claimed, deleted from storage and from
//!    `blobs` under the write lock. An upload of the same digest in between
//!    removes it from the queue, so it is never deleted underneath a push.
//! 3. **Orphans**: storage objects with no `blobs` row are deleted.
//!
//! The queue makes GC resumable; every step is idempotent.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::{Duration, Instant},
};

use futures::StreamExt;
use serde::Serialize;

use crate::{
    db::{self, Db},
    digest::Digest,
    storage::Storage,
};

#[derive(Clone, Debug)]
pub(crate) struct GcOptions {
    pub dry_run: bool,
    pub delete_untagged: bool,
    /// Content younger than this is never collected (protects in-flight pushes).
    pub min_age: Duration,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct GcReport {
    pub dry_run: bool,
    pub delete_untagged: bool,
    pub min_age_seconds: u64,
    /// Untagged manifests deleted (or that would be).
    pub manifests_deleted: u64,
    /// Blobs whose storage was deleted (or would be).
    pub blobs_deleted: u64,
    pub bytes_freed: u64,
    /// Storage objects without a database record, deleted.
    pub orphans_deleted: u64,
    /// Unreferenced blobs kept because they are younger than `min_age`.
    pub blobs_kept_young: u64,
    /// Sweeps that failed (retried on the next run).
    pub errors: u64,
    pub duration_ms: u64,
}

type ManifestKey = (i64, String);

pub(crate) async fn run(db: &Db, storage: &dyn Storage, opts: &GcOptions) -> anyhow::Result<GcReport> {
    let started = Instant::now();
    let mut report = GcReport {
        dry_run: opts.dry_run,
        delete_untagged: opts.delete_untagged,
        min_age_seconds: opts.min_age.as_secs(),
        ..Default::default()
    };
    let cutoff = crate::time::before_now(opts.min_age);

    // --- Mark -----------------------------------------------------------------
    let mut tx = db.begin_write().await?;
    let manifests = db::manifests::all_for_gc(&mut tx).await?;
    let refs = db::manifests::refs_for_gc(&mut tx).await?;
    let tag_roots = db::tags::roots_for_gc(&mut tx).await?;

    let kept: HashSet<ManifestKey> = if opts.delete_untagged {
        let mut children: HashMap<ManifestKey, Vec<String>> = HashMap::new();
        for r in refs.iter().filter(|r| r.kind == "manifest") {
            children.entry((r.repository_id, r.manifest_digest.clone())).or_default().push(r.child_digest.clone());
        }
        let mut referrers: HashMap<ManifestKey, Vec<String>> = HashMap::new();
        for m in &manifests {
            if let Some(subject) = &m.subject_digest {
                referrers.entry((m.repository_id, subject.clone())).or_default().push(m.digest.clone());
            }
        }
        let mut queue: VecDeque<ManifestKey> = tag_roots.into_iter().collect();
        queue.extend(manifests.iter().filter(|m| m.created_at >= cutoff).map(|m| (m.repository_id, m.digest.clone())));
        let mut kept = HashSet::new();
        while let Some(key) = queue.pop_front() {
            if !kept.insert(key.clone()) {
                continue;
            }
            for next in children.get(&key).into_iter().chain(referrers.get(&key)).flatten() {
                queue.push_back((key.0, next.clone()));
            }
        }
        kept
    } else {
        manifests.iter().map(|m| (m.repository_id, m.digest.clone())).collect()
    };

    for m in manifests.iter().filter(|m| !kept.contains(&(m.repository_id, m.digest.clone()))) {
        report.manifests_deleted += 1;
        if !opts.dry_run {
            db::manifests::delete(&mut *tx, m.repository_id, &m.digest).await?;
        }
    }

    let mut live: HashSet<&str> = HashSet::new();
    for m in manifests.iter().filter(|m| kept.contains(&(m.repository_id, m.digest.clone()))) {
        live.insert(m.digest.as_str());
    }
    for r in refs.iter().filter(|r| r.kind == "blob" && kept.contains(&(r.repository_id, r.manifest_digest.clone()))) {
        live.insert(r.child_digest.as_str());
    }

    let mut to_sweep: Vec<(String, i64)> = Vec::new();
    for b in db::blobs::all_for_gc(&mut tx).await? {
        if live.contains(b.digest.as_str()) {
            continue;
        }
        let young = b.created_at >= cutoff || b.last_linked_at.as_deref().is_some_and(|t| t >= cutoff.as_str());
        if young {
            report.blobs_kept_young += 1;
        } else {
            to_sweep.push((b.digest, b.size));
        }
    }
    if opts.dry_run {
        tx.rollback().await?;
        for (_, size) in &to_sweep {
            report.blobs_deleted += 1;
            report.bytes_freed += u64::try_from(*size).unwrap_or(0);
        }
        // Leftovers of an interrupted run would be swept too.
        for (_, size) in db::blobs::pending_sweep(&db.read).await? {
            report.blobs_deleted += 1;
            report.bytes_freed += u64::try_from(size).unwrap_or(0);
        }
        report.orphans_deleted = count_orphans(db, storage).await?;
        report.duration_ms = started.elapsed().as_millis() as u64;
        return Ok(report);
    }
    let now = crate::time::now();
    for (digest, _) in &to_sweep {
        db::blobs::mark_for_sweep(&mut tx, digest, &now).await?;
    }
    tx.commit().await?;

    // --- Sweep ----------------------------------------------------------------
    for (digest, size) in db::blobs::pending_sweep(&db.read).await? {
        match sweep_one(db, storage, &digest).await {
            Ok(true) => {
                report.blobs_deleted += 1;
                report.bytes_freed += u64::try_from(size).unwrap_or(0);
            }
            Ok(false) => {}
            Err(e) => {
                report.errors += 1;
                tracing::warn!(%digest, error = %e, "failed to sweep blob; it stays queued");
            }
        }
    }

    // --- Orphans --------------------------------------------------------------
    let mut stored = storage.list_blobs().await?;
    while let Some(item) = stored.next().await {
        let digest = item?;
        if db::blobs::row_exists(&db.read, digest.as_str()).await? {
            continue;
        }
        let mut tx = db.begin_write().await?;
        if db::blobs::row_exists(&mut *tx, digest.as_str()).await? {
            continue;
        }
        // Holding the write lock: an upload committing this digest re-checks
        // storage after acquiring it and re-uploads if needed.
        storage.delete_blob(&digest).await?;
        tx.commit().await?;
        report.orphans_deleted += 1;
    }

    report.duration_ms = started.elapsed().as_millis() as u64;
    Ok(report)
}

/// Deletes one queued blob; false if an upload rescued it meanwhile.
async fn sweep_one(db: &Db, storage: &dyn Storage, digest: &str) -> anyhow::Result<bool> {
    let parsed = Digest::parse(digest).map_err(|e| anyhow::anyhow!("{digest}: {e}"))?;
    let mut tx = db.begin_write().await?;
    if !db::blobs::take_sweep(&mut tx, digest).await? {
        return Ok(false);
    }
    storage.delete_blob(&parsed).await?;
    db::blobs::delete_row(&mut tx, digest).await?;
    tx.commit().await?;
    Ok(true)
}

async fn count_orphans(db: &Db, storage: &dyn Storage) -> anyhow::Result<u64> {
    let mut n = 0;
    let mut stored = storage.list_blobs().await?;
    while let Some(item) = stored.next().await {
        if !db::blobs::row_exists(&db.read, item?.as_str()).await? {
            n += 1;
        }
    }
    Ok(n)
}
