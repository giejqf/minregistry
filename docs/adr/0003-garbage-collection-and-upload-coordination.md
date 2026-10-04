# 3. Garbage collection and its coordination with uploads

Date: 2026-10-03 · Status: accepted

## Context

AGENTS.md §5.6 asks for mark-and-sweep GC that is idempotent, resumable,
audited, and that takes "a global write lock on uploads completion to avoid
racing a concurrent push". GC can run inside the server (API, schedule) and as
a separate process (`minregistry gc`) against the same SQLite database and
storage, so an in-process mutex is not enough. Storage operations (S3 PUT and
DELETE) cannot be part of a database transaction. Two races matter:

1. A push uploads layers, then its manifest some time later. A GC in between
   sees unreferenced layers.
2. GC decides to delete digest D while an upload of the same content commits.

## Decision

**The global lock is SQLite's write lock.** Every write transaction that
matters starts with `BEGIN IMMEDIATE` (`Db::begin_write`), which serializes
writers across processes.

**Grace period.** GC never collects blobs or manifests younger than
`MINREGISTRY_GC_MIN_AGE` (default 1 h): a blob is "young" if it was created or
(re)linked into any repository recently (`repository_blobs.created_at`, which
re-uploads and mounts refresh). Untagged manifests that are young are roots
too, so the per-platform manifests of a multi-arch push survive until the
index arrives. This handles race 1.

**Two-phase delete with a queue (`gc_sweep`).**

- *Mark* (one write transaction): compute live manifests (all of them, or
  with `--delete-untagged` those reachable from tags through index children and
  referrers, plus young ones), delete the unreachable manifests, compute live
  blobs (live manifest digests and their config/layer refs), and for every
  other old blob delete its `repository_blobs` links (it becomes invisible) and
  insert it into `gc_sweep`.
- *Sweep*, per queued digest, in its own write transaction: claim the queue
  row (`DELETE … RETURNING`); if it was still there, delete the storage object
  and the `blobs` row; commit.
- *Orphans*: storage objects with no `blobs` row are deleted under the write
  lock after re-checking the row.

**Upload commit protocol** (`registry/content.rs`): write the storage object
first (skipped when a non-queued `blobs` row already exists), then in a write
transaction: if the `blobs` row is missing, verify the object still exists
(a sweep may have deleted it after our write) and otherwise roll back,
re-upload and retry; insert the row; delete any `gc_sweep` entry (rescuing a
queued blob); link it to the repository. Manifest pushes do the same for the
manifest bytes and validate their references inside the same transaction.

Because sweeps delete an object only while the digest is still queued, and
commits dequeue the digest under the same lock, an object is never deleted
underneath a row that refers to it. A crash leaves at most queued digests,
which the next run sweeps (resumable); every step is idempotent.

## Consequences

- Holding the write lock while doing one storage HEAD/DELETE serializes those
  writes; for a single-tenant registry this is a small cost.
- `min_age = 0` (useful in tests) removes the protection for in-flight pushes;
  the API and CLI allow it explicitly per run.
- Deleting a blob through `/v2/` only unlinks it; storage is reclaimed by GC.
