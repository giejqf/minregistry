# 5. Upload staging and resumable uploads

Date: 2026-10-03 · Status: accepted

## Context

Uploads are staged on local disk (`MINREGISTRY_UPLOAD_DIR`) for every backend,
and the digest must be verified once, while streaming (AGENTS.md §2, §5.3).
Chunked uploads span several requests, and clients may disconnect mid-chunk.

## Decision

- Each session has a staging file `<upload dir>/<uuid>` and a row in
  `uploads` holding the committed `offset`. Bytes are appended through a
  1 MiB buffer and fed to a running SHA-256 as they arrive.
- The running hash lives in memory, keyed by session, behind a per-session
  async mutex that also serializes concurrent requests for one session.
  `sha2` state is not serializable, so after a restart (or any I/O error) the
  hash is rebuilt by reading the staging file up to the committed offset —
  once, from local disk, never on the hot path and never from the storage
  backend.
- `PATCH` with `Content-Range` must start at the current offset (otherwise
  416 with the current `Range`). Without it the body is appended (streamed
  upload).
- **Interrupted chunks keep their bytes**: if the client disconnects, the
  bytes that reached the disk are committed to `offset`, and `GET` on the
  session reports them in `Range`, so the client can resume from there. On a
  server-side I/O error the request's bytes are discarded (the file is
  truncated back) and the hash is rebuilt on the next request.
- An empty session reports `Range: 0-0`, as the reference implementation
  does. `Location` headers are relative (`/v2/<name>/blobs/uploads/<uuid>`).
- Completion verifies the digest, copies (or hard-links, on the same
  filesystem) the file into storage, and commits per ADR 3. A digest
  mismatch ends the session. Only the session's creator (or an admin) can use
  it. Idle sessions expire after `MINREGISTRY_UPLOAD_TTL`; stray staging files
  older than the TTL are removed.

## Consequences

A restart during an upload costs one sequential read of the partial file. The
S3 backend needs local scratch space for the largest concurrent layers.
