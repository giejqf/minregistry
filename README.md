# MinRegistry

A self-hosted, single-tenant **OCI / Docker container registry** with a real
management UI, shipped as one binary.

- **OCI Distribution Spec v1.1**, complete: push/pull, chunked and monolithic
  uploads (resumable), range requests, tag listing, manifest/tag/blob delete,
  the referrers API, cross-repository blob mount — plus garbage collection.
  Passes the official conformance suite and is tested with `docker`, `crane`,
  `skopeo` and `oras`.
- **No anonymous access.** Registry clients authenticate with HTTP Basic:
  a named identity (`ci-deploy`, `alice`) and a token.
- **Per-repository permissions** (`read`, `write`, `owner`); pushing to a new
  repository creates it and makes the pusher its owner.
- **Admins sign in with GitHub** (an allowlist of logins) to manage
  repositories, identities, tokens and grants, and to read the audit log.
- **Audit log** of who did what to which repository/reference, from where.
- **Storage:** local filesystem or any S3-compatible store; metadata in one
  SQLite file.

## Quick start

1. Create a GitHub OAuth App with the callback URL
   `https://registry.example.com/auth/github/callback`.
2. Run the image (multi-platform: `linux/amd64`, `linux/arm64`) behind a
   TLS-terminating reverse proxy:

```sh
docker run -d --name minregistry -p 5000:5000 -v minregistry-data:/data \
  -e MINREGISTRY_PUBLIC_URL=https://registry.example.com \
  -e MINREGISTRY_TRUST_PROXY=true \
  -e MINREGISTRY_GITHUB_CLIENT_ID=... \
  -e MINREGISTRY_GITHUB_CLIENT_SECRET=... \
  -e MINREGISTRY_ADMIN_GITHUB_LOGINS=your-github-login \
  -e MINREGISTRY_SESSION_SECRET="$(openssl rand -base64 48)" \
  giejqf/minregistry:1
```

   To build it yourself instead: `docker build -t minregistry .`

3. Open `https://registry.example.com`, sign in with GitHub, go to
   *Principals*, and create a token for yourself (or an identity for CI). The
   token is shown once, with the `docker login` command:

```sh
echo "$TOKEN" | docker login registry.example.com -u your-github-login --password-stdin
docker push registry.example.com/team/app:1.0
```

Every setting is an environment variable; see [docs/config.md](docs/config.md).
The server speaks plain HTTP: TLS belongs to the reverse proxy, which must
pass request bodies through unbuffered and without size limits for large
layers.

### Command line

```
minregistry serve                     # registry + management API + web UI
minregistry migrate                   # apply database migrations
minregistry gc [--dry-run] [--delete-untagged] [--min-age 1h]
minregistry openapi                   # print the management API's OpenAPI document
```

## Development

The repository layout, conventions and the definition of done are in
[AGENTS.md](AGENTS.md); design decisions are in [docs/adr](docs/adr). Common
tasks are [just](https://just.systems) recipes (`just --list`):

```sh
cargo build && cargo test                 # backend (uses the committed .sqlx/ data)
just build-live                           # build against a live dev DB after editing queries
just sqlx-prepare                         # regenerate .sqlx/
just openapi                              # regenerate web/openapi.json and web/src/sdk
pnpm --dir web install && pnpm --dir web dev   # UI on :5173, proxying to :5000
./e2e/run.sh                              # real clients + OCI conformance (needs Docker)
```

### Releasing

1. Bump `version` in `server/Cargo.toml` and `web/package.json`, then run
   `just openapi` (the version is part of `web/openapi.json`) and commit
   `chore(release): X.Y.Z`.
2. Tag and push: `git tag -a vX.Y.Z -m "MinRegistry X.Y.Z" && git push origin main vX.Y.Z`.
3. `.github/workflows/release.yml` builds `linux/amd64` and `linux/arm64`
   natively and publishes `giejqf/minregistry` as `X.Y.Z`, `X.Y`, `X` and
   `latest`. It needs the repository secrets `DOCKERHUB_USERNAME` and
   `DOCKERHUB_TOKEN`; the image name can be overridden with the repository
   variable `DOCKERHUB_IMAGE`. Run it by hand (`gh workflow run release -f tag=vX.Y.Z`)
   to republish an existing tag.
4. The Docker Hub overview is `docs/docker-hub.md`; changes to it on `main`
   are synced by `.github/workflows/dockerhub-overview.yml` (the token needs
   read, write & delete scope for this).

## License

Apache-2.0 — see [LICENSE](LICENSE) and [NOTICE](NOTICE).
