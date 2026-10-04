# AGENTS.md

Guidance for AI coding agents (and humans) working in this repository.
Read this file fully before making changes. When a decision here conflicts
with your instinct, follow this file and raise the conflict in your summary.

---

## 1. What this project is

**MinRegistry** — a self-hosted, single-tenant **OCI / Docker container
registry** with a real management UI, built as one deployable binary.

- **Backend:** Rust, `axum`. Serves the registry API (`/v2/`), the management
  API (`/api/v1/`), and the built frontend (`/`).
- **Frontend:** React + TypeScript + Vite + Tailwind + shadcn/ui. SDK is
  generated from the backend's `openapi.json`; never hand-write API clients.
- **Metadata DB:** SQLite (via `sqlx`). One file, WAL mode.
- **Blob storage:** local filesystem **or** S3-compatible (AWS S3, MinIO,
  R2, …), selected by config. Both go through one `Storage` trait.
- **Admin auth:** GitHub OAuth only. No other OAuth provider, ever.
- **Registry-client auth:** HTTP Basic with named identities + tokens.
- **Audit:** every meaningful registry and management action is recorded
  with *who* did *what* to *which* repository/reference, and from where.

**Name:** MinRegistry. Crate, binary, npm package and env-var prefix all use
`minregistry` / `MINREGISTRY_`. Use "MinRegistry" in prose and UI,
`minregistry` in code and identifiers.

**License:** Apache-2.0. Every crate/package manifest must declare
`license = "Apache-2.0"`. Keep `LICENSE` (full text) and `NOTICE` at the
repo root. Do not add dependencies with licenses incompatible with
Apache-2.0 (notably GPL/AGPL); `cargo deny check licenses` runs in CI.
No per-file license headers are required.

---

## 2. Settled product decisions (do not re-litigate)

These were decided with the project owner. Implement them as written.

| Topic | Decision |
|---|---|
| Tenancy | **Single registry, multiple users.** No namespaces-as-tenants, no org concept. |
| Admins | GitHub OAuth. An admin is any GitHub login listed in `MINREGISTRY_ADMIN_GITHUB_LOGINS` (comma-separated). Nobody else can sign in to the web UI. |
| Non-admin users | **Token-only identities.** Admins create a named identity (e.g. `ci-deploy`, `alice`) and issue one or more tokens for it. Identities have **no web login**. |
| Admin CLI access | Admins are also principals. An admin mints a token for their own GitHub-backed principal to use with `docker login`. |
| Anonymous access | **Never.** Every `/v2/` request except `GET /v2/` (version check) must carry valid credentials. There is no "public repository" flag. |
| Spec level | **OCI Distribution Spec v1.1, full.** Push/pull, tag list, manifest & tag delete, blob delete, referrers API (`/v2/<name>/referrers/<digest>`), cross-repo blob mount, chunked + monolithic uploads. Plus garbage collection of unreferenced blobs. |
| Repository creation | **Auto-create on push.** Any authenticated principal may push to a non-existent repository; it is created and the pusher becomes its `owner`. Admins may reassign/adjust afterwards. |
| Permissions | Per-repository, per-principal: `read`, `write`, `owner`. `write` implies `read`. `owner` implies `write` plus managing that repo's permissions and deleting it. Global admins bypass all checks. |
| Compatibility proof | End-to-end tests with **real clients** (`docker`, `crane`, `skopeo`, `oras`) and the **official OCI distribution-spec conformance suite** must pass in CI. |
| TLS | Not handled by the server. A reverse proxy terminates TLS. Server speaks plain HTTP and trusts `X-Forwarded-*` only when `MINREGISTRY_TRUST_PROXY=true`. |

### Assumptions made by the agent (flag to owner if they look wrong)

- Admin set is config-driven (env var), not a DB table, so a fresh install
  is bootstrappable without a seed step.
- Registry-API auth is **Basic** (`WWW-Authenticate: Basic realm="minregistry"`),
  not the Docker token-auth (`Bearer`) flow. Every mainstream client supports
  Basic. The Bearer/JWT flow may be added later behind the same identity
  model; do not implement it unless asked.
- In-flight chunked uploads are staged on local disk
  (`MINREGISTRY_UPLOAD_DIR`) and copied to the storage backend on completion.
  This applies even when the backend is S3 (avoids multipart part-size
  constraints). S3 deployments therefore need a small scratch volume.
- Blob **downloads** are not audited by default (`MINREGISTRY_AUDIT_BLOB_READS=false`)
  because one image pull fans out into N blob GETs. Manifest pulls are the
  canonical "who pulled what" record. Blob uploads/completions *are* audited.
- Tokens are shown once at creation, stored as SHA-256 of a 32-byte random
  secret. No expiry by default; optional `expires_at`. Revocation is instant.

---

## 3. Repository layout

```
.
├── AGENTS.md                 ← you are here
├── Cargo.toml                ← workspace
├── LICENSE                   ← Apache-2.0 full text
├── NOTICE
├── server/                   ← Rust crate: the whole backend binary
│   ├── migrations/           ← sqlx migrations (NNNN_name.sql), append-only
│   ├── src/
│   │   ├── main.rs           ← CLI: `serve`, `openapi`, `gc`, `migrate`
│   │   ├── config.rs         ← env/TOML → Config (validated at startup)
│   │   ├── app.rs            ← router assembly, state, middleware stack
│   │   ├── registry/         ← /v2/ handlers (OCI distribution)
│   │   ├── api/              ← /api/v1/ handlers (management, utoipa-annotated)
│   │   ├── auth/             ← Basic auth, sessions, GitHub OAuth, authz
│   │   ├── storage/          ← Storage trait + fs + s3 impls
│   │   ├── db/               ← sqlx queries, grouped by table
│   │   ├── audit.rs          ← AuditEvent + writer
│   │   ├── gc.rs             ← mark-and-sweep
│   │   └── ui.rs             ← rust-embed of web/dist, SPA fallback
│   └── tests/                ← integration tests (in-process server)
├── web/                      ← Vite + React + TS app
│   ├── openapi.json          ← GENERATED, committed, CI-checked
│   ├── src/sdk/              ← GENERATED by @hey-api/openapi-ts, committed
│   ├── src/components/ui/    ← shadcn components (generated, lightly edited)
│   ├── src/pages/
│   └── src/lib/
├── e2e/                      ← real-client tests + conformance harness
│   ├── docker-compose.yml    ← registry + minio for S3 mode
│   ├── run.sh                ← entrypoint used by CI
│   ├── clients/              ← docker/crane/skopeo/oras scenarios (bash)
│   └── conformance/          ← wrapper around opencontainers/distribution-spec tests
├── docs/
│   ├── config.md             ← every env var, documented
│   └── adr/                  ← architecture decision records
└── .github/workflows/ci.yml
```

---

## 4. Commands

Always run from repo root unless noted. Prefer `just` recipes when they
exist; add one if you find yourself typing the same multi-step command.

```sh
# Backend
cargo build
cargo run -p minregistry -- serve                 # dev server on :5000
cargo run -p minregistry -- openapi > web/openapi.json
cargo run -p minregistry -- migrate               # apply migrations
cargo run -p minregistry -- gc [--dry-run]
cargo test -p minregistry                         # unit + integration
cargo sqlx prepare --workspace                # after changing queries; commit .sqlx/
cargo clippy --all-targets -- -D warnings
cargo fmt --check

# Frontend (pnpm only)
cd web && pnpm install
pnpm --dir web dev                            # proxies /api and /v2 to :5000
pnpm --dir web gen:sdk                        # regenerate src/sdk from openapi.json
pnpm --dir web build                          # outputs web/dist (embedded by server)
pnpm --dir web lint && pnpm --dir web typecheck

# End-to-end (needs Docker daemon)
./e2e/run.sh                                  # all clients + conformance, fs backend
MINREGISTRY_STORAGE=s3 ./e2e/run.sh               # same, against MinIO
```

**Order of operations after changing any `/api/v1/` handler or DTO:**
1. `cargo run -p minregistry -- openapi > web/openapi.json`
2. `pnpm --dir web gen:sdk`
3. Commit both outputs. CI fails if they are stale.

---

## 5. Architecture

### 5.1 Request routing

| Prefix | Purpose | Auth |
|---|---|---|
| `GET /v2/` | API version check | none (returns 401 challenge + `Docker-Distribution-API-Version` header, per spec) |
| `/v2/**` | OCI Distribution API | Basic: `username = identity name`, `password = token` |
| `/api/v1/**` | Management API (JSON, utoipa-documented) | Session cookie (GitHub OAuth). Admin only. |
| `/api/v1/openapi.json` | Live OpenAPI doc | none |
| `/auth/github/login`, `/auth/github/callback`, `/auth/logout` | OAuth flow | none |
| `/healthz`, `/readyz` | probes | none |
| `/**` | SPA (embedded `web/dist`) | none |

Registry routes **must not** be behind the session middleware, and
management routes **must not** accept Basic auth. Keep the two auth
extractors separate (`RegistryPrincipal` vs `AdminSession`).

### 5.2 Data model (SQLite)

```
principals      id, kind('github'|'identity'), name (unique), github_id?, 
                display_name, enabled, created_at
tokens          id, principal_id, name, hash (sha256 hex), prefix (first 8),
                created_by, created_at, expires_at?, revoked_at?, last_used_at?
repositories    id, name (unique, validated per spec regex), created_by,
                created_at, deleted_at?
permissions     principal_id, repository_id, level('read'|'write'|'owner'),
                granted_by, granted_at        PK(principal_id, repository_id)
blobs           digest (PK), size, created_at
repository_blobs repository_id, digest, PK(both)   ← per-repo visibility
manifests       repository_id, digest, media_type, size, subject_digest?,
                artifact_type?, created_at, pushed_by   PK(repository_id, digest)
manifest_refs   repository_id, manifest_digest, child_digest, kind('blob'|'manifest')
tags            repository_id, name, manifest_digest, updated_at, updated_by
                PK(repository_id, name)
uploads         uuid (PK), repository_id, principal_id, offset, started_at,
                last_activity_at
audit_events    id, ts, principal_id?, principal_name (denormalized),
                action, repository?, reference?, digest?, client_ip,
                user_agent, outcome('ok'|'denied'|'error'), detail (JSON)
```

Rules:
- `audit_events` is **append-only**. No UPDATE/DELETE paths in app code
  except the retention job (`MINREGISTRY_AUDIT_RETENTION_DAYS`, 0 = forever).
- Blobs are content-addressed and global; `repository_blobs` gates which
  repos may serve them (required for correct cross-repo mount semantics and
  for not leaking existence across repos).
- Repository `name` is validated against the distribution-spec regex:
  `[a-z0-9]+((\.|_|__|-+)[a-z0-9]+)*(\/[a-z0-9]+((\.|_|__|-+)[a-z0-9]+)*)*`.
- Enable `PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;`
  on every connection (set in the pool's `after_connect`).
- Writes go through a single-writer connection pool (`max_connections=1`
  for the write pool, separate read pool). Do not open ad-hoc connections.

### 5.3 Storage

```rust
#[async_trait]
pub trait Storage: Send + Sync {
    async fn blob_exists(&self, digest: &Digest) -> Result<bool>;
    async fn blob_size(&self, digest: &Digest) -> Result<Option<u64>>;
    async fn get_blob(&self, digest: &Digest, range: Option<Range>) -> Result<BlobStream>;
    async fn put_blob_from_file(&self, digest: &Digest, path: &Path) -> Result<()>;
    async fn delete_blob(&self, digest: &Digest) -> Result<()>;
    async fn list_blobs(&self) -> Result<BoxStream<Digest>>; // for GC
}
```

- Implementations: `FsStorage` (`MINREGISTRY_STORAGE=fs`, root at
  `MINREGISTRY_FS_ROOT`) and `S3Storage` (`MINREGISTRY_STORAGE=s3`, built on the
  `object_store` crate with endpoint/region/bucket/credentials/path-style env).
- Key layout: `blobs/sha256/<first two hex>/<full hex>/data`. Same layout on
  both backends.
- Manifests are blobs too (stored by digest); the DB holds metadata only.
- Digest verification happens **once**, while streaming the upload to the
  staging file. Never read a blob back to re-hash it on the hot path.
- Both backends must pass the same `storage/tests.rs` suite; S3 tests run
  against MinIO (skipped if `MINREGISTRY_TEST_S3_ENDPOINT` unset).

### 5.4 Authentication & authorization

- **Registry (Basic):** `Authorization: Basic base64(name:token)`. Look up
  principal by `name`, then constant-time compare `sha256(token)` against
  non-revoked, non-expired tokens of that principal. Update `last_used_at`
  at most once per minute per token (avoid write amplification).
- **Admin (web):** GitHub OAuth (`oauth2` crate), scope `read:user` only.
  After callback, if `login` ∉ admin allowlist → 403 page, no session.
  Session = signed+encrypted cookie (`tower-sessions` with SQLite store),
  `SameSite=Lax`, `HttpOnly`, `Secure` when `MINREGISTRY_TRUST_PROXY=true` or
  `MINREGISTRY_PUBLIC_URL` is https. CSRF: all mutating `/api/v1/` requests must
  carry `X-Requested-With: XMLHttpRequest` (SDK sets it); reject otherwise.
- **Authz:** one function, `authorize(principal, repo_name, Action) -> Result<(), Denied>`,
  used by every registry handler. Actions: `Pull`, `Push`, `Delete`, `Mount`.
  Admin → allow. Repo missing + `Push` → allow (auto-create). Otherwise look
  up `permissions`. **Denied returns 401 if unauthenticated, 403 if
  authenticated** — do not leak repo existence to unauthorized principals
  (403 for both "no permission" and "repo does not exist" when
  authenticated but not permitted).
- Every denial is audited with `outcome='denied'`.

### 5.5 OCI Distribution specifics (easy to get wrong)

- Error bodies use the spec JSON shape `{"errors":[{"code","message","detail"}]}`
  with the canonical codes (`BLOB_UNKNOWN`, `MANIFEST_UNKNOWN`,
  `NAME_UNKNOWN`, `DIGEST_INVALID`, `UNAUTHORIZED`, `DENIED`, `UNSUPPORTED`,
  `TOOMANYREQUESTS`, …). Centralize in `registry/error.rs`.
- Always set `Docker-Content-Digest` on manifest/blob responses, `Location`
  + `Docker-Upload-UUID` + `Range` on upload responses, `OCI-Chunk-Min-Length`
  if you enforce a minimum.
- `HEAD` must return the same headers as `GET` with no body.
- Manifest PUT: parse enough to (a) validate referenced blobs/manifests
  exist in **this** repo, (b) record `subject` for referrers, (c) store
  `mediaType` from the `Content-Type` header (fall back to body `mediaType`).
  Store the **exact bytes** received; digest = sha256 of those bytes.
- Referrers: `GET /v2/<name>/referrers/<digest>` returns an
  `application/vnd.oci.image.index.v1+json` listing manifests whose
  `subject` matches; support `?artifactType=` filter and set
  `OCI-Filters-Applied`. When a manifest with `subject` is pushed, respond
  with `OCI-Subject` header.
- Tag list: `?n=` and `?last=` pagination with `Link` header, lexically
  sorted.
- Blob mount: `POST /v2/<name>/blobs/uploads/?mount=<digest>&from=<repo>`
  succeeds (201) only if principal has `Pull` on `from` **and** `Push` on
  `name`; otherwise fall through to a normal upload session (202) — do not
  error, per spec.
- Deletes: manifest delete by digest or tag (tag delete removes only the
  tag). Blob delete removes the `repository_blobs` row only; storage
  deletion is GC's job.
- Range requests on blob GET (`Range: bytes=`) must work; `docker pull`
  resume and `oras` rely on it.
- Upload sessions expire after `MINREGISTRY_UPLOAD_TTL` (default 24h);
  a background task cleans staging files.

### 5.6 Garbage collection

Mark-and-sweep, run on demand (`minregistry gc`, `POST /api/v1/gc`) or on a
schedule (`MINREGISTRY_GC_CRON`). Marks every blob reachable from any tag or
untagged manifest (untagged manifests are kept unless `--delete-untagged`),
sweeps `blobs` rows + storage objects with zero references. Takes a global
write lock on uploads completion to avoid racing a concurrent push. Must be
idempotent and resumable. Always audited with a summary in `detail`.

### 5.7 OpenAPI → SDK

- Backend uses `utoipa` + `utoipa-axum`. Every `/api/v1/` handler has a
  `#[utoipa::path]` and every DTO derives `ToSchema`. The `/v2/` routes are
  **not** in the OpenAPI doc (they're defined by the OCI spec).
- `minregistry openapi` prints the doc; it must be byte-identical to
  `web/openapi.json` in CI.
- Frontend SDK: `@hey-api/openapi-ts` with the `@tanstack/react-query`
  plugin, output to `web/src/sdk/`. Pages call generated hooks only.
- DTO naming: `*Request` / `*Response` / `*Summary` / `*Detail`. Timestamps
  are RFC 3339 strings. IDs are integers in DB, strings in API.

### 5.8 Frontend

- Vite, React 19, TypeScript strict, Tailwind v4, shadcn/ui (add components
  with `pnpm dlx shadcn@latest add <name>`; commit the generated file).
- Routing: `react-router`. Data: TanStack Query via generated hooks. Forms:
  `react-hook-form` + `zod`. No global state library.
- Pages (MVP):
  1. **Sign in** — single "Continue with GitHub" button.
  2. **Repositories** — list with search; detail shows tags, manifests
     (digest, media type, size, platform(s)), referrers, delete actions.
  3. **Principals** — list GitHub admins (read-only, from config) and
     identities; create/disable identity; issue/revoke tokens (token shown
     once in a copyable dialog with a `docker login` snippet).
  4. **Permissions** — per repository: grant/revoke `read|write|owner`
     per principal. Also reachable from repository detail.
  5. **Audit log** — paginated table; filters by principal, repository,
     action, outcome, date range; row expands to JSON `detail`.
  6. **System** — storage backend info, DB size, run GC (dry-run first),
     upload sessions in flight.
- Dark mode via shadcn's `ThemeProvider`. Keep UI minimal; no marketing
  pages, no charts unless asked.

---

## 6. Coding conventions

### Rust
- Edition 2021, MSRV = current stable. `#![deny(unsafe_code)]`.
- Errors: `thiserror` for typed errors in modules; a single `AppError`
  implementing `IntoResponse` that maps to OCI error JSON on `/v2/` and to
  `{"error": {"code","message"}}` on `/api/v1/`. Never `unwrap()` outside
  tests and `main`-level startup validation.
- Async: `tokio`. Never block the runtime; hashing large uploads happens on
  the stream as bytes arrive (`sha2`), file IO via `tokio::fs`.
- Logging: `tracing` with `tracing-subscriber` JSON in prod, pretty in dev.
  Attach `request_id`, `principal`, `repo` as span fields. **Never log
  tokens, Authorization headers, or OAuth secrets.** Redact in extractors.
- Config: all `MINREGISTRY_*` env vars parsed into one `Config` struct, validated
  at startup with clear error messages; documented in `docs/config.md`.
  Adding a config value without documenting it is a bug.
- SQL: `sqlx::query!`/`query_as!` macros (compile-time checked) with
  committed `.sqlx/` offline data. Migrations are append-only; never edit a
  migration that has been merged.
- Module visibility: `pub(crate)` by default. The crate is a binary; there
  is no public library API to preserve.

### TypeScript
- `strict: true`, `noUncheckedIndexedAccess: true`. ESLint + Prettier
  (config in repo); no inline `eslint-disable` without a comment why.
- Components are function components; colocate `*.test.tsx` with Vitest +
  Testing Library for anything with logic.
- Never edit `web/src/sdk/**` by hand.

### Git
- Conventional Commits (`feat(registry): …`, `fix(ui): …`, `test(e2e): …`).
- One logical change per PR. Include the regenerated `openapi.json`/SDK in
  the same PR as the backend change that caused it.
- Record non-obvious architectural choices as `docs/adr/NNNN-title.md`.

---

## 7. Testing — definition of done

A change is done when **all** of the following pass locally and in CI:

1. `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo deny check licenses`
2. `pnpm --dir web lint && pnpm --dir web typecheck && pnpm --dir web test && pnpm --dir web build`
3. `openapi.json` and `src/sdk` are up to date (CI regenerates and diffs).
4. `./e2e/run.sh` passes for **both** `fs` and `s3` backends.

### Test layers

| Layer | Where | What |
|---|---|---|
| Unit | `server/src/**` `#[cfg(test)]` | digest parsing, name regex, authz matrix, manifest parsing, pagination |
| Storage contract | `server/src/storage/tests.rs` | same suite against fs and MinIO |
| Integration | `server/tests/` | in-process axum app + temp SQLite + temp fs; exercise `/v2/` and `/api/v1/` with `reqwest`; audit rows asserted after each action |
| Conformance | `e2e/conformance/` | official `opencontainers/distribution-spec` Go conformance tests, all workflows (`pull`, `push`, `content_discovery`, `content_management`) enabled |
| Real clients | `e2e/clients/*.sh` | scripted scenarios, each asserting exit codes and expected audit events via the management API |

### Required real-client scenarios (keep this list in sync with `e2e/clients/`)

- `docker login` success/failure; `docker push` new repo (auto-create,
  pusher becomes owner); `docker pull` by tag and by digest; multi-arch
  (`docker buildx imagetools create`) push + pull.
- Permission matrix: identity with `read` cannot push (403, audited
  `denied`); identity with no grant gets 403 on pull; revoked token → 401.
- `crane copy` from a public registry into MinRegistry; `crane ls`; `crane
  delete` by digest; `crane manifest` on index.
- `skopeo copy` both directions, `skopeo inspect`, `skopeo delete`.
- `oras push` an arbitrary artifact with `--artifact-type`, `oras attach`
  to an image, `oras discover` (exercises referrers), `oras pull`.
- Chunked upload with a large layer (≥ 100 MiB), interrupted and resumed.
- Cross-repo blob mount (push same base image to two repos; second push
  must mount, not re-upload; verify via audit `mount` event).
- GC: delete a tag, run `gc --dry-run` then `gc`, verify blob gone from
  storage and remaining images still pull.
- Audit: for each scenario above, query `/api/v1/audit` and assert the
  expected `(principal, action, repository, reference)` tuples exist.

Docker clients talk to `localhost:5000` as an insecure registry in CI
(`"insecure-registries"` in daemon config); the e2e harness sets this up.

---

## 8. Things agents must NOT do

- Do not add a second OAuth provider, password login for admins, or
  anonymous/public pull. These are explicit non-goals.
- Do not introduce Postgres/MySQL support or an ORM. SQLite + `sqlx` only.
- Do not implement the Docker Bearer token-auth flow, Docker Hub proxying /
  pull-through cache, image scanning, webhooks, or notifications unless the
  owner asks. If you think one is needed, say so in your summary.
- Do not hand-edit generated files (`web/openapi.json`, `web/src/sdk/**`,
  `.sqlx/**`). Regenerate them.
- Do not weaken the e2e suite to get CI green. If a real client disagrees
  with our behavior, the client is right until proven otherwise with a spec
  citation.
- Do not log or persist secrets. Tokens exist in plaintext only in the
  creation response.
- Do not change the storage key layout or the audit schema without an ADR;
  both are effectively public contracts once deployed.
- Do not block on the Tokio runtime (sync file IO, sync hashing of whole
  buffers, `std::thread::sleep`).

---

## 9. Configuration reference (keep in sync with `docs/config.md`)

| Variable | Default | Notes |
|---|---|---|
| `MINREGISTRY_LISTEN` | `0.0.0.0:5000` | |
| `MINREGISTRY_PUBLIC_URL` | required | e.g. `https://registry.example.com`; used for OAuth callback + cookie flags |
| `MINREGISTRY_TRUST_PROXY` | `false` | honor `X-Forwarded-For/Proto` |
| `MINREGISTRY_DB_PATH` | `./data/minregistry.db` | |
| `MINREGISTRY_STORAGE` | `fs` | `fs` \| `s3` |
| `MINREGISTRY_FS_ROOT` | `./data/blobs` | fs only |
| `MINREGISTRY_S3_ENDPOINT`, `_REGION`, `_BUCKET`, `_ACCESS_KEY`, `_SECRET_KEY`, `_PATH_STYLE` | — | s3 only |
| `MINREGISTRY_UPLOAD_DIR` | `./data/uploads` | staging for in-flight uploads |
| `MINREGISTRY_UPLOAD_TTL` | `24h` | |
| `MINREGISTRY_GITHUB_CLIENT_ID`, `_CLIENT_SECRET` | required | |
| `MINREGISTRY_ADMIN_GITHUB_LOGINS` | required | comma-separated GitHub logins |
| `MINREGISTRY_SESSION_SECRET` | required | ≥ 32 bytes, base64 |
| `MINREGISTRY_AUDIT_BLOB_READS` | `false` | audit individual blob GETs |
| `MINREGISTRY_AUDIT_RETENTION_DAYS` | `0` | 0 = keep forever |
| `MINREGISTRY_GC_CRON` | unset | cron expr; unset = manual only |
| `MINREGISTRY_LOG` | `info` | `tracing` filter |

---

## 10. Suggested implementation order

1. Workspace skeleton, config, SQLite pool + migrations, `/healthz`.
2. `Storage` trait + fs impl + contract tests.
3. `/v2/` blobs: upload (monolithic, chunked), HEAD/GET with ranges, delete.
4. `/v2/` manifests: PUT with validation, GET/HEAD, tags list, delete.
5. Basic auth + principals/tokens + authz + audit writer (wire into 3–4).
6. Conformance suite green on fs.
7. S3 storage impl; conformance green on s3.
8. Referrers API, blob mount, GC.
9. GitHub OAuth, sessions, management API with utoipa; export openapi.json.
10. Frontend: scaffold, SDK gen, pages in the order listed in §5.8.
11. Real-client e2e scenarios; CI workflow; embed UI into binary; Dockerfile.

Ship each step with tests. Do not start the frontend before the management
API's OpenAPI doc is stable enough to generate from.
