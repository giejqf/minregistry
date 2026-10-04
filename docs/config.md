# Configuration

MinRegistry is configured entirely through `MINREGISTRY_*` environment
variables. They are parsed and validated once at startup; every problem is
reported at once and the process exits with status 2. This file documents
every variable — a variable that is read but not listed here is a bug.

Which commands read what:

| Command | Reads |
|---|---|
| `minregistry serve` | everything below |
| `minregistry gc`, `minregistry migrate` | database, storage, upload, audit, GC and log settings (not HTTP, GitHub or session settings) |
| `minregistry openapi` | nothing |

Durations use [humantime](https://docs.rs/humantime) syntax: `30s`, `15m`,
`24h`, `7days`, `1h 30m`. Booleans accept `true`/`false` (also `1`/`0`,
`yes`/`no`, `on`/`off`).

## HTTP

| Variable | Default | Description |
|---|---|---|
| `MINREGISTRY_LISTEN` | `0.0.0.0:5000` | Address and port to listen on. The server speaks plain HTTP; terminate TLS in a reverse proxy. |
| `MINREGISTRY_PUBLIC_URL` | **required** | The URL clients and browsers use, e.g. `https://registry.example.com`. The OAuth callback is `<public url>/auth/github/callback`; an `https` URL makes the session cookie `Secure`. The registry API must be served at the root of this host (`/v2/`). |
| `MINREGISTRY_TRUST_PROXY` | `false` | Trust `X-Forwarded-For` (the right-most address, i.e. the one your proxy appended, becomes the audited client IP). Also forces `Secure` cookies. Enable only behind a proxy that sets the header. |

## Database

| Variable | Default | Description |
|---|---|---|
| `MINREGISTRY_DB_PATH` | `./data/minregistry.db` | SQLite database file (WAL mode; `-wal` and `-shm` files live next to it). Created, and migrated, on start. |

## Blob storage

| Variable | Default | Description |
|---|---|---|
| `MINREGISTRY_STORAGE` | `fs` | `fs` (local directory) or `s3` (any S3-compatible object store). |
| `MINREGISTRY_FS_ROOT` | `./data/blobs` | `fs` only: root directory. Blobs live at `blobs/sha256/<xx>/<hex>/data`; `.tmp/` holds files being moved into place. |
| `MINREGISTRY_S3_ENDPOINT` | AWS | `s3` only: endpoint URL for non-AWS stores, e.g. `http://minio:9000`, `https://<account>.r2.cloudflarestorage.com`. `http://` endpoints are allowed. |
| `MINREGISTRY_S3_REGION` | `us-east-1` | `s3` only: region. |
| `MINREGISTRY_S3_BUCKET` | required for `s3` | `s3` only: bucket (must exist; `/readyz` answers 503 while it does not). Keys use the same `blobs/sha256/<xx>/<hex>/data` layout as `fs`. |
| `MINREGISTRY_S3_ACCESS_KEY` | unset | `s3` only: access key id. Set together with the secret key; when both are unset the standard `AWS_*` variables / instance credentials are used. |
| `MINREGISTRY_S3_SECRET_KEY` | unset | `s3` only: secret access key (never logged). |
| `MINREGISTRY_S3_PATH_STYLE` | `false` | `s3` only: `true` for path-style requests (`<endpoint>/<bucket>/<key>`, needed by MinIO and most self-hosted stores); `false` for virtual-hosted style (`<bucket>.<endpoint host>`). |

## Uploads

| Variable | Default | Description |
|---|---|---|
| `MINREGISTRY_UPLOAD_DIR` | `./data/uploads` | Local staging directory for in-flight uploads, also with the `s3` backend (uploads are hashed while staged and copied to the store on completion). Size it for the largest concurrent layers. Putting it on the same filesystem as `MINREGISTRY_FS_ROOT` makes completion a hard link instead of a copy. |
| `MINREGISTRY_UPLOAD_TTL` | `24h` | Upload sessions idle for longer are cancelled and their staging files removed (checked every `TTL/4`, at most every 5 minutes). |

## GitHub sign-in (admins)

| Variable | Default | Description |
|---|---|---|
| `MINREGISTRY_GITHUB_CLIENT_ID` | **required** | Client id of a GitHub OAuth App whose callback URL is `<MINREGISTRY_PUBLIC_URL>/auth/github/callback`. Only the `read:user` scope is requested. |
| `MINREGISTRY_GITHUB_CLIENT_SECRET` | **required** | Its client secret (never logged). |
| `MINREGISTRY_GITHUB_URL` | `https://github.com` | Base URL of the authorize and token endpoints (`/login/oauth/...`). Set it for GitHub Enterprise Server, or to a fake GitHub in tests. |
| `MINREGISTRY_GITHUB_API_URL` | `https://api.github.com` | REST API base URL (`GET /user`). GitHub Enterprise Server: `https://<host>/api/v3`. |
| `MINREGISTRY_ADMIN_GITHUB_LOGINS` | **required** | Comma-separated GitHub logins (case-insensitive) allowed to sign in. Nobody else can use the web UI. Removing a login revokes that admin's web access and registry tokens on the next request. |
| `MINREGISTRY_SESSION_SECRET` | **required** | At least 32 bytes, base64-encoded (`openssl rand -base64 48`). Encrypts and signs the session cookie. Changing it signs everyone out. |

Admin sessions expire after 12 hours of inactivity.

## Audit log

| Variable | Default | Description |
|---|---|---|
| `MINREGISTRY_AUDIT_BLOB_READS` | `false` | Also audit every blob download (`blob.pull`). One image pull fans out into many blob reads, so manifest pulls (`manifest.pull`) are the default record of who pulled what. |
| `MINREGISTRY_AUDIT_RETENTION_DAYS` | `0` | Delete audit events older than this many days (checked every 6 hours; the deletion itself is audited as `audit.prune`). `0` keeps events forever. |

## Garbage collection

| Variable | Default | Description |
|---|---|---|
| `MINREGISTRY_GC_CRON` | unset | Run GC on this schedule (UTC cron expression, five fields, e.g. `0 3 * * *`). Scheduled runs keep untagged manifests. Unset: GC runs only via `minregistry gc` or the System page. |
| `MINREGISTRY_GC_MIN_AGE` | `1h` | GC never deletes manifests or blobs younger than this, so it cannot remove the layers of a push that is still in progress. `minregistry gc --min-age` and the API's `min_age_seconds` override it per run. |

## Logging

| Variable | Default | Description |
|---|---|---|
| `MINREGISTRY_LOG` | `info` | [`tracing` filter](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html), e.g. `info,minregistry=debug`. |
| `MINREGISTRY_LOG_FORMAT` | `auto` | `json` (one object per line), `pretty`, or `auto`: pretty when stderr is a terminal, JSON otherwise. The Docker image sets `json`. |

Logs go to stderr. Tokens, `Authorization` headers, OAuth codes and secrets are
never logged; request logs contain the method, path, status, latency, request
id, principal and repository.

## Test-only variables

Read by `cargo test`, never by the server:

| Variable | Default | Description |
|---|---|---|
| `MINREGISTRY_TEST_S3_ENDPOINT` | unset | Run the storage contract suite against this S3 endpoint (e.g. MinIO at `http://127.0.0.1:9000`). Skipped when unset. |
| `MINREGISTRY_TEST_S3_BUCKET` | `minregistry-test` | Existing bucket for that suite. |
| `MINREGISTRY_TEST_S3_ACCESS_KEY`, `MINREGISTRY_TEST_S3_SECRET_KEY` | `minioadmin` | Credentials for that suite. |
| `MINREGISTRY_TEST_S3_REGION` | `us-east-1` | Region for that suite. |

The end-to-end harness has its own switches (`MINREGISTRY_STORAGE`,
`E2E_REGISTRY`, `E2E_KEEP`, …); see [`e2e/README.md`](../e2e/README.md).
