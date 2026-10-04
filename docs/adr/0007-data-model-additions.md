# 7. Additions to the AGENTS.md data model

Date: 2026-10-03 · Status: accepted

## Context

AGENTS.md §5.2 lists the tables. Implementing the spec and the UI needed a few
more columns and tables.

## Decision

- `manifests.annotations` (JSON object): the referrers API must return each
  referrer's annotations; storing them avoids reading every referrer manifest
  back from storage.
- `manifests.platforms` (JSON array of `{os, architecture, variant, os.version}`,
  absent fields omitted): from an index's descriptors, or from the config blob
  of an image manifest (read once at push time, if ≤ 1 MiB). Shown on the
  repository page; `os.version` tells apart e.g. Windows Server releases.
  (The comment in `0001_init.sql` predates `os.version`; merged migrations are
  checksummed and are not edited.) The repository page also nests an index's
  platform images under it, using the `manifest_refs` of kind `manifest`.
- `manifests.artifact_type` holds the *effective* artifact type: `artifactType`
  or, for image manifests without it, `config.mediaType` (as the referrers API
  defines it).
- `repository_blobs.created_at`: when the blob was (re)linked; the GC grace
  period uses it (ADR 3).
- `gc_sweep`: the queue of blobs chosen for deletion (ADR 3).
- `sessions`: the server-side store for admin web sessions. The
  `tower-sessions-sqlx-store` crate depends on an older `sqlx` (two versions of
  `libsqlite3-sys` cannot link), so the store is implemented in
  `auth/session.rs` on the application's own pools.
- Repositories are soft-deleted (`deleted_at`): tags, manifests, links, grants
  and uploads are removed, the row stays for history, and a later push to the
  same name revives it with the pusher as owner.
- Every TEXT primary key is declared `NOT NULL` (SQLite otherwise allows NULL
  in non-integer primary keys). Timestamps are fixed-width RFC 3339 UTC strings
  with milliseconds so they sort lexically.

## Consequences

Migrations stay append-only; these are part of `0001_init.sql`.
