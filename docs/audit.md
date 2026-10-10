# Audit log

Every meaningful registry and management action is recorded in the
append-only `audit_events` table: **who** did **what** to **which**
repository/reference, **from where**, and with what **outcome**. Admins read it
on the *Audit log* page or through `GET /api/v1/audit`.

The table layout and the action names below are a public contract (scripts and
SIEM pipelines depend on them): change them only through an ADR
([0008](adr/0008-audit-event-vocabulary.md)).

## Fields

| Field | Meaning |
|---|---|
| `id` | Monotonic id; the API pages with it (`cursor`). |
| `ts` | RFC 3339 UTC timestamp, millisecond precision. |
| `principal_id`, `principal_name` | The actor. `principal_name` is denormalized so events survive renames. For failed logins it is the username that was *claimed* and `principal_id` is empty. Background jobs use `system`. |
| `action` | See below. |
| `repository` | Repository name, when the action concerns one. |
| `reference` | Tag or digest exactly as the client addressed it. |
| `digest` | The content digest involved (resolved from the tag when possible). |
| `client_ip` | Peer address, or the right-most `X-Forwarded-For` entry with `MINREGISTRY_TRUST_PROXY=true`. |
| `user_agent` | Client user agent (truncated to 512 characters). |
| `outcome` | `ok`, `denied` (authentication or authorization refused) or `error` (e.g. digest mismatch). |
| `detail` | JSON object with action-specific context (below). Management actions carry `"via": "api"`. |

Events are never updated (a database trigger rejects `UPDATE`). The only
deletion is retention (`MINREGISTRY_AUDIT_RETENTION_DAYS`), itself audited.

## Actions

### Registry API (`/v2/`)

| Action | Recorded when | `reference` / `digest` | `detail` |
|---|---|---|---|
| `login` | `GET /v2/` with valid credentials (`ok`); any `/v2/` request with invalid credentials (`denied`). Missing credentials (the normal challenge) are not recorded. | — | `denied`: `reason`, `path` |
| `repository.create` | A push auto-creates (or revives a deleted) repository; the pusher becomes `owner`. | — | `owner` |
| `blob.upload` | An upload completes (`ok`), its digest does not match (`error`), or the principal may not push (`denied`). | digest | `size`; `error`: `reason`, `actual` |
| `blob.mount` | A cross-repository mount succeeds (`ok`) or is refused because the source is not readable (`denied`, the client falls back to uploading). | digest | `from` |
| `blob.pull` | A blob download, only with `MINREGISTRY_AUDIT_BLOB_READS=true`. | digest | `range` |
| `blob.delete` | A blob is unlinked from a repository. | digest | — |
| `upload.cancel` | An upload session is cancelled. | — | `uuid` |
| `manifest.push` | A manifest is pushed (by tag or digest). | tag or digest / digest | `mediaType`, `subject` |
| `manifest.pull` | A manifest is fetched (`GET`) or resolved (`HEAD`). Clients resolve a tag with `HEAD` and then fetch the digest, so a pull by tag shows both. | tag or digest / digest | `method` |
| `manifest.delete` | A manifest (and the tags pointing at it) is deleted by digest. | digest | — |
| `tag.delete` | A tag is deleted (the manifest stays). | tag / digest | — |
| `tag.list` | Tags are listed. | — | `count` |
| `referrers.list` | The referrers of a manifest are listed. | — / subject digest | `count`, `artifactType` |
| `catalog.list` | The repository list (`GET /v2/_catalog`) is read. It shows only the repositories the principal may pull from. | — | `count` |

Any of these may also appear with `outcome = denied` when the principal lacks
the required permission; `detail.required` names it (`pull`, `push`, `delete`,
`mount`) and `detail.path` the request path.

### Management API, sign-in and maintenance

| Action | Recorded when | `detail` |
|---|---|---|
| `admin.login` | GitHub sign-in succeeds (`ok`) or the login is not an admin (`denied`). | `github_id`, `reason` |
| `admin.logout` | An admin signs out. | — |
| `principal.create` | An identity is created. | `principal` |
| `principal.update` | An identity is enabled/disabled or renamed. | `principal`, `enabled`, `display_name` |
| `token.create` | A token is issued (the secret is never recorded). | `principal`, `token_id`, `token_name`, `prefix`, `expires_at` |
| `token.revoke` | A token is revoked. | `principal`, `token_id` |
| `permission.grant` | A grant is created or changed. | `principal`, `level` |
| `permission.revoke` | A grant is removed. | `principal` |
| `repository.delete` | A repository is deleted. | — |
| `tag.delete`, `manifest.delete` | Deleted through the management API. | `via: api` |
| `gc.run` | Garbage collection ran (`trigger`: `api`, `cli` or `schedule`). | `report` (or `error`) |
| `audit.prune` | Retention deleted old events. | `deleted`, `before`, `retention_days` |
