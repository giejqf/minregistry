# End-to-end tests

Real registry clients and the official OCI conformance suite against a real
MinRegistry, for both storage backends.

```sh
./e2e/run.sh                          # filesystem storage
MINREGISTRY_STORAGE=s3 ./e2e/run.sh   # S3 (MinIO)
./e2e/run.sh 05-oras conformance      # selected scenarios
```

`run.sh` builds the image from the root `Dockerfile` and starts
`docker-compose.yml` (all host networking): the registry on `localhost:5000`,
a fake GitHub on `localhost:5555` (`fake-github/fake_github.py`) and, for S3,
MinIO on `localhost:9000`. It signs in as the admin `e2e-admin` through the
real OAuth redirect flow, then runs each scenario. Every scenario creates its
own identities and tokens through the management API, drives a real client,
checks exit codes and asserts the expected audit events
(`principal, action, repository, reference, outcome`) through
`GET /api/v1/audit`.

| Scenario | Covers |
|---|---|
| `clients/01-docker.sh` | `docker login` success and failure; push to a new repository (auto-create, pusher becomes owner); pull by tag and by digest; multi-arch push with `docker buildx imagetools create` and pull |
| `clients/02-permissions.sh` | `read` cannot push (403, audited `denied`); no grant gets 403 on pull and push; revoked token → 401 |
| `clients/03-crane.sh` | `crane copy` from `registry.k8s.io`; `crane ls`; `crane catalog` (only readable repositories); `crane manifest` on an index; `crane delete` by digest |
| `clients/04-skopeo.sh` | `skopeo copy` into MinRegistry and out of it (to an OCI layout and back); `skopeo inspect`; `skopeo delete` |
| `clients/05-oras.sh` | `oras push --artifact-type`; `oras attach` to an image; `oras discover` (referrers API); `oras pull` |
| `clients/06-chunked-resume.sh` | chunked upload of a 128 MiB layer, interrupted mid-chunk and resumed from the reported `Range`; ranged download |
| `clients/07-mount.sh` | pushing the same image to a second repository mounts the layer instead of re-uploading it (docker, crane), verified via `blob.mount` events |
| `clients/08-gc.sh` | delete a tag, `minregistry gc --dry-run`, then `gc`: the blob is gone from storage and the remaining image still pulls |
| `conformance/run.sh` | opencontainers/distribution-spec v1.1.1 conformance, all workflows (pull, push, content discovery, content management) |

## Requirements

- Docker with buildx and compose v2, `curl`, `jq`, `git`, `sha256sum`.
- Ports 5000 and 5555 (and 9000/9001 for S3) free.
- Network access to `registry.k8s.io` (crane, skopeo) and GitHub (the
  conformance sources).
- `crane`, `oras` and `skopeo` are used when installed; otherwise they run
  from their official images. The conformance suite is built with the local Go
  toolchain, or in the `golang` image.

Registry clients use a private `DOCKER_CONFIG` under `e2e/.work/`; your own
Docker credentials are never read or changed. Images tagged
`localhost:5000/e2e/*` are removed at the end.

## Switches

| Variable | Default | Meaning |
|---|---|---|
| `MINREGISTRY_STORAGE` | `fs` | `fs` or `s3` |
| `E2E_REGISTRY` | `compose` | `host` runs `MINREGISTRY_BIN` (default `target/release/minregistry`) on the host instead of the container — faster when iterating |
| `E2E_BUILD` | `1` | `0` reuses an existing `minregistry:e2e` image |
| `E2E_KEEP` | `0` | `1` leaves the stack running (inspect it at `http://localhost:5000`, sign in as `e2e-admin`) |
| `OCI_CONFORMANCE_VERSION` | `v1.1.1` | distribution-spec tag to build the suite from |

Outputs (logs, conformance JUnit/HTML reports) land in `e2e/.work/`.
