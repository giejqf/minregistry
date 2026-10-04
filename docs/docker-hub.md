# MinRegistry

A self-hosted, single-tenant **OCI / Docker container registry** with a real
management UI, shipped as one small image (`linux/amd64`, `linux/arm64`).

![The repository page of the MinRegistry web UI: tags, multi-arch manifests with their platforms, and referrers](https://raw.githubusercontent.com/giejqf/minregistry/main/docs/images/repository.png)

- **Complete OCI Distribution Spec v1.1**: push and pull, resumable chunked
  uploads, range requests, the referrers API (signatures, SBOMs,
  attestations), cross-repository blob mounts, deletes and garbage
  collection. Passes the official conformance suite and is tested with
  `docker`, `crane`, `skopeo` and `oras`.
- **No anonymous access.** Clients authenticate with HTTP Basic: a named
  identity (`ci-deploy`, `alice`) and a revocable token.
- **Per-repository permissions**: `read`, `write`, `owner`. Pushing to a new
  repository creates it and makes the pusher its owner.
- **Web UI behind GitHub sign-in** for an allowlist of admins: repositories,
  identities and tokens, permissions, the audit log, garbage collection.
- **Audit log** of who did what to which repository and reference, and from
  where.
- **Storage**: a local volume or any S3-compatible store (AWS S3, MinIO,
  Cloudflare R2, …). Metadata lives in one SQLite file.

Source, issues and full documentation:
[github.com/giejqf/minregistry](https://github.com/giejqf/minregistry)

## Tags

| Tag | Meaning |
|---|---|
| `1.0.1`, `1.0`, `1`, `latest` | The current release. `1.0` follows 1.0.x patch releases, `1` follows every 1.x release. |

Every tag is a multi-platform image for `linux/amd64` and `linux/arm64`.

## Quick start

1. Create a [GitHub OAuth App](https://github.com/settings/developers) with
   the authorization callback URL `https://registry.example.com/auth/github/callback`.
2. Generate a session secret once and keep it (changing it signs everyone out):
   `openssl rand -base64 48`.
3. Run MinRegistry:

```sh
docker run -d --name minregistry --restart unless-stopped \
  -p 127.0.0.1:5000:5000 -v minregistry-data:/data \
  -e MINREGISTRY_PUBLIC_URL=https://registry.example.com \
  -e MINREGISTRY_TRUST_PROXY=true \
  -e MINREGISTRY_GITHUB_CLIENT_ID=your-oauth-app-client-id \
  -e MINREGISTRY_GITHUB_CLIENT_SECRET=your-oauth-app-client-secret \
  -e MINREGISTRY_ADMIN_GITHUB_LOGINS=your-github-login \
  -e MINREGISTRY_SESSION_SECRET=your-session-secret \
  giejqf/minregistry:1
```

4. Put it behind a TLS-terminating reverse proxy (see below): Docker only talks
   to registries other than `localhost` over HTTPS.
5. Open `https://registry.example.com`, sign in with GitHub, and on
   *Principals* create a token for yourself, or an identity for CI. The token
   is shown once, together with the login command:

```sh
echo "$TOKEN" | docker login registry.example.com -u your-github-login --password-stdin
docker tag myapp registry.example.com/team/myapp:1.0
docker push registry.example.com/team/myapp:1.0
```

## Docker Compose

```yaml
services:
  minregistry:
    image: giejqf/minregistry:1
    restart: unless-stopped
    ports:
      - "127.0.0.1:5000:5000"
    volumes:
      - minregistry-data:/data
    environment:
      MINREGISTRY_PUBLIC_URL: https://registry.example.com
      MINREGISTRY_TRUST_PROXY: "true"
      MINREGISTRY_GITHUB_CLIENT_ID: your-oauth-app-client-id
      MINREGISTRY_GITHUB_CLIENT_SECRET: your-oauth-app-client-secret
      MINREGISTRY_ADMIN_GITHUB_LOGINS: your-github-login,a-colleague
      MINREGISTRY_SESSION_SECRET: your-session-secret
      # Optional: run garbage collection every night at 03:00 UTC.
      MINREGISTRY_GC_CRON: "0 3 * * *"

volumes:
  minregistry-data:
```

To keep blobs in S3 instead of the volume, add:

```yaml
      MINREGISTRY_STORAGE: s3
      MINREGISTRY_S3_BUCKET: my-registry-bucket
      MINREGISTRY_S3_REGION: eu-west-1
      MINREGISTRY_S3_ACCESS_KEY: ...
      MINREGISTRY_S3_SECRET_KEY: ...
      # For MinIO, R2 and other S3-compatible stores:
      # MINREGISTRY_S3_ENDPOINT: https://minio.example.com
      # MINREGISTRY_S3_PATH_STYLE: "true"
```

The `/data` volume is still needed with S3: it holds the SQLite database and
stages uploads in progress.

## Reverse proxy

MinRegistry speaks plain HTTP on port 5000. Terminate TLS in a proxy that
streams request bodies without size limits (layers can be gigabytes), and
set `MINREGISTRY_TRUST_PROXY=true` so the audit log records the client's
address from `X-Forwarded-For`.

**Caddy** (obtains certificates automatically):

```
registry.example.com {
    reverse_proxy 127.0.0.1:5000
}
```

**nginx**:

```nginx
server {
    listen 443 ssl;
    server_name registry.example.com;
    # ssl_certificate ...; ssl_certificate_key ...;

    client_max_body_size 0;
    proxy_request_buffering off;
    proxy_buffering off;

    location / {
        proxy_pass http://127.0.0.1:5000;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_read_timeout 900s;
        proxy_send_timeout 900s;
    }
}
```

## Configuration

Everything is configured with environment variables; the server validates
them at startup and reports every problem at once.

| Variable | Default | |
|---|---|---|
| `MINREGISTRY_PUBLIC_URL` | required | The URL users and clients use, e.g. `https://registry.example.com` |
| `MINREGISTRY_GITHUB_CLIENT_ID`, `MINREGISTRY_GITHUB_CLIENT_SECRET` | required | The GitHub OAuth App (scope `read:user` only) |
| `MINREGISTRY_ADMIN_GITHUB_LOGINS` | required | Comma-separated GitHub logins allowed to sign in |
| `MINREGISTRY_SESSION_SECRET` | required | At least 32 bytes, base64 |
| `MINREGISTRY_TRUST_PROXY` | `false` | Trust `X-Forwarded-*` from your reverse proxy |
| `MINREGISTRY_STORAGE` | `fs` | `fs` (the `/data` volume) or `s3` |
| `MINREGISTRY_GC_CRON` | unset | Scheduled garbage collection (UTC cron expression) |
| `MINREGISTRY_AUDIT_RETENTION_DAYS` | `0` | Delete audit events older than this; `0` keeps them forever |
| `MINREGISTRY_LOG` | `info` | Log filter, e.g. `info,minregistry=debug` |

All variables, including S3, upload, GitHub Enterprise and logging settings:
[docs/config.md](https://github.com/giejqf/minregistry/blob/main/docs/config.md).

## Image details

- **Port** `5000`: the registry API (`/v2/`), the management API (`/api/v1/`)
  and the web UI. Health probes: `/healthz` (process) and `/readyz` (database
  and storage; when it answers 503, the log says why).
- **Volume** `/data`: the SQLite database, blobs (with `fs` storage) and
  upload staging. Back it up: stop the container while copying, or take a
  consistent copy of `minregistry.db` with SQLite's `.backup` command from the
  host (the image does not include `sqlite3`).
- **User**: runs as the non-root UID `10001`. Named volumes work as is; for a
  bind mount, `chown -R 10001 /path/to/data` first.
- **Logs**: JSON lines on stderr. Tokens and secrets are never logged.
- **Upgrades**: pull the new tag and recreate the container; database
  migrations run on start.
- **Maintenance**: the same binary runs maintenance commands next to the
  server:

```sh
docker exec minregistry minregistry gc --dry-run              # what would be deleted
docker exec minregistry minregistry gc --delete-untagged      # reclaim storage
```

## License

Apache-2.0. See [LICENSE](https://github.com/giejqf/minregistry/blob/main/LICENSE).
