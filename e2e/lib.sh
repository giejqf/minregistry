# shellcheck shell=bash
# Shared helpers for the end-to-end scenarios. Sourced by run.sh and every
# script in clients/ and conformance/.

set -euo pipefail

E2E_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$E2E_DIR/.." && pwd)"
: "${REGISTRY:=localhost:5000}"
: "${REGISTRY_URL:=http://$REGISTRY}"
: "${E2E_WORK:=$E2E_DIR/.work}"
: "${MINREGISTRY_STORAGE:=fs}"
: "${E2E_ADMIN_LOGIN:=e2e-admin}"
: "${E2E_RUN_ID:=$(date +%s)}"
export REGISTRY REGISTRY_URL E2E_WORK MINREGISTRY_STORAGE E2E_ADMIN_LOGIN E2E_RUN_ID
mkdir -p "$E2E_WORK"

# Registry clients never touch the user's own Docker configuration.
if [[ -z "${E2E_DOCKER_CONFIG_READY:-}" ]]; then
  export DOCKER_CONFIG="$E2E_WORK/docker"
  mkdir -p "$DOCKER_CONFIG"
  [[ -f "$DOCKER_CONFIG/config.json" ]] || echo '{}' >"$DOCKER_CONFIG/config.json"
  if [[ -d "$HOME/.docker/cli-plugins" && ! -e "$DOCKER_CONFIG/cli-plugins" ]]; then
    ln -s "$HOME/.docker/cli-plugins" "$DOCKER_CONFIG/cli-plugins"
  fi
  export E2E_DOCKER_CONFIG_READY=1
fi

ADMIN_JAR="$E2E_WORK/admin.cookies"

log() { printf '\033[1;34m==>\033[0m %s\n' "$*" >&2; }
pass() { printf '\033[1;32m  ✓\033[0m %s\n' "$*" >&2; }
fail() {
  printf '\033[1;31m  ✗ %s\033[0m\n' "$*" >&2
  exit 1
}

: "${E2E_REGISTRY:=compose}"
: "${MINREGISTRY_BIN:=$REPO_ROOT/target/release/minregistry}"
export E2E_REGISTRY MINREGISTRY_BIN

# compose: the e2e stack (registry, fake GitHub, MinIO) with the active profiles.
compose() {
  local profiles=()
  [[ "$E2E_REGISTRY" == compose ]] && profiles+=(--profile registry)
  [[ "$MINREGISTRY_STORAGE" == s3 ]] && profiles+=(--profile s3)
  docker compose -f "$E2E_DIR/docker-compose.yml" "${profiles[@]}" "$@"
}

# registry_env: server configuration for E2E_REGISTRY=host (mirrors the compose file).
registry_env() {
  local data="$E2E_WORK/host-data"
  cat <<EOF
export MINREGISTRY_LISTEN=127.0.0.1:5000
export MINREGISTRY_PUBLIC_URL=http://localhost:5000
export MINREGISTRY_DB_PATH=$data/minregistry.db
export MINREGISTRY_FS_ROOT=$data/blobs
export MINREGISTRY_UPLOAD_DIR=$data/uploads
export MINREGISTRY_GITHUB_CLIENT_ID=e2e-client
export MINREGISTRY_GITHUB_CLIENT_SECRET=e2e-secret
export MINREGISTRY_GITHUB_URL=http://127.0.0.1:5555
export MINREGISTRY_GITHUB_API_URL=http://127.0.0.1:5555
export MINREGISTRY_ADMIN_GITHUB_LOGINS=e2e-admin
export MINREGISTRY_SESSION_SECRET=${E2E_SESSION_SECRET:-}
export MINREGISTRY_STORAGE=$MINREGISTRY_STORAGE
export MINREGISTRY_S3_ENDPOINT=http://127.0.0.1:9000
export MINREGISTRY_S3_BUCKET=minregistry
export MINREGISTRY_S3_ACCESS_KEY=minioadmin
export MINREGISTRY_S3_SECRET_KEY=minioadmin
export MINREGISTRY_S3_PATH_STYLE=true
export MINREGISTRY_LOG=info
EOF
}

# registry_cli ARGS...: runs the minregistry CLI next to the running server.
registry_cli() {
  if [[ "$E2E_REGISTRY" == compose ]]; then
    compose exec -T registry minregistry "$@"
  else
    (
      eval "$(registry_env)"
      "$MINREGISTRY_BIN" "$@"
    )
  fi
}

# --- Admin session (GitHub OAuth against the fake GitHub) --------------------

# admin_login [login] [jar]: signs in through /auth/github/login.
admin_login() {
  local login="${1:-$E2E_ADMIN_LOGIN}" jar="${2:-$ADMIN_JAR}" authorize callback result
  rm -f "$jar"
  authorize=$(curl -sS -o /dev/null -w '%{redirect_url}' -c "$jar" -b "$jar" "$REGISTRY_URL/auth/github/login")
  [[ -n "$authorize" ]] || fail "no redirect to GitHub from /auth/github/login"
  callback=$(curl -sS -o /dev/null -w '%{redirect_url}' "$authorize&login=$login")
  [[ "$callback" == "$REGISTRY_URL/auth/github/callback?"* ]] || fail "unexpected OAuth redirect: $callback"
  result=$(curl -sS -o /dev/null -w '%{http_code}' -c "$jar" -b "$jar" "$callback")
  echo "$result"
}

# api METHOD PATH [JSON]: management API call as the admin; prints the body,
# fails on non-2xx.
api() {
  local method="$1" path="$2" body="${3:-}" out status
  out=$(mktemp)
  local args=(-sS -o "$out" -w '%{http_code}' -b "$ADMIN_JAR" -X "$method" -H 'X-Requested-With: XMLHttpRequest')
  [[ -n "$body" ]] && args+=(-H 'Content-Type: application/json' --data "$body")
  status=$(curl "${args[@]}" "$REGISTRY_URL/api/v1$path")
  if [[ "$status" != 2* ]]; then
    echo "API $method $path -> $status: $(cat "$out")" >&2
    rm -f "$out"
    return 1
  fi
  cat "$out"
  rm -f "$out"
}

# api_status METHOD PATH [JSON]: prints only the HTTP status.
api_status() {
  local method="$1" path="$2" body="${3:-}"
  local args=(-sS -o /dev/null -w '%{http_code}' -b "$ADMIN_JAR" -X "$method" -H 'X-Requested-With: XMLHttpRequest')
  [[ -n "$body" ]] && args+=(-H 'Content-Type: application/json' --data "$body")
  curl "${args[@]}" "$REGISTRY_URL/api/v1$path"
}

my_principal_id() { api GET /me | jq -r .principal.id; }

# create_identity NAME: prints the new principal id.
create_identity() { api POST /principals "$(jq -nc --arg n "$1" '{name: $n}')" | jq -r .id; }

# create_token PRINCIPAL_ID [NAME]: prints the secret.
create_token() {
  api POST "/principals/$1/tokens" "$(jq -nc --arg n "${2:-e2e}" '{name: $n}')" | jq -r .secret
}

# repo_id NAME: prints the repository id.
repo_id() {
  api GET "/repositories?q=$(jq -rn --arg n "$1" '$n|@uri')&limit=500" | jq -r --arg n "$1" '.items[] | select(.name == $n) | .id'
}

# grant REPO_NAME PRINCIPAL_ID LEVEL
grant() {
  local rid
  rid=$(repo_id "$1")
  [[ -n "$rid" ]] || fail "repository $1 not found"
  api PUT "/repositories/$rid/permissions/$2" "{\"level\":\"$3\"}" >/dev/null
}

# assert_audit PRINCIPAL ACTION REPOSITORY [REFERENCE] [OUTCOME]: an audit
# event with these fields exists (REFERENCE and OUTCOME may be empty = any).
assert_audit() {
  local principal="$1" action="$2" repository="$3" reference="${4:-}" outcome="${5:-ok}" query matches
  query="principal=$(jq -rn --arg v "$principal" '$v|@uri')&action=$action&limit=500"
  [[ -n "$repository" ]] && query+="&repository=$(jq -rn --arg v "$repository" '$v|@uri')"
  [[ -n "$outcome" ]] && query+="&outcome=$outcome"
  matches=$(api GET "/audit?$query" | jq --arg r "$reference" '[.items[] | select($r == "" or .reference == $r)] | length')
  if [[ "$matches" -lt 1 ]]; then
    fail "missing audit event ($principal, $action, ${repository:-*}, ${reference:-*}, ${outcome:-*})"
  fi
  pass "audit: ($principal, $action, ${repository:-*}, ${reference:-*}, ${outcome:-any})"
}

# audit_count PRINCIPAL ACTION REPOSITORY [DIGEST]
audit_count() {
  local query="principal=$(jq -rn --arg v "$1" '$v|@uri')&action=$2&repository=$(jq -rn --arg v "$3" '$v|@uri')&limit=500"
  api GET "/audit?$query" | jq --arg d "${4:-}" '[.items[] | select($d == "" or .digest == $d)] | length'
}

# --- Registry clients ----------------------------------------------------------

# Clients that are not installed run from their official images.
skopeo() {
  if type -P skopeo >/dev/null 2>&1 && [[ -z "${E2E_SKOPEO_IMAGE:-}" ]]; then
    command skopeo "$@"
  else
    docker run --rm --network host -u "$(id -u):$(id -g)" -e HOME=/tmp -v "$E2E_WORK:$E2E_WORK" -e DOCKER_CONFIG \
      -e REGISTRY_AUTH_FILE="$DOCKER_CONFIG/config.json" -v "$DOCKER_CONFIG:$DOCKER_CONFIG" \
      "${E2E_SKOPEO_IMAGE:-quay.io/skopeo/stable:latest}" "$@"
  fi
}

crane() {
  if type -P crane >/dev/null 2>&1; then
    command crane "$@"
  else
    docker run --rm --network host -u "$(id -u):$(id -g)" -e HOME=/tmp -v "$E2E_WORK:$E2E_WORK" -e DOCKER_CONFIG \
      -v "$DOCKER_CONFIG:$DOCKER_CONFIG" gcr.io/go-containerregistry/crane:latest "$@"
  fi
}

oras() {
  if type -P oras >/dev/null 2>&1; then
    command oras "$@"
  else
    docker run --rm --network host -u "$(id -u):$(id -g)" -e HOME=/tmp -v "$E2E_WORK:$E2E_WORK" -w "$PWD" -e DOCKER_CONFIG \
      -v "$DOCKER_CONFIG:$DOCKER_CONFIG" ghcr.io/oras-project/oras:v1.3.0 "$@"
  fi
}

# docker_login USER SECRET: logs the Docker CLI in (credentials stay in $DOCKER_CONFIG).
docker_login() {
  printf '%s' "$2" | docker login "$REGISTRY" -u "$1" --password-stdin >/dev/null 2>&1
}

# build_image TAG [PLATFORM]: a tiny FROM-scratch image with a unique layer.
build_image() {
  local tag="$1" platform="${2:-}" dir
  dir=$(mktemp -d "$E2E_WORK/build.XXXXXX")
  head -c 4096 /dev/urandom >"$dir/payload"
  printf 'FROM scratch\nCOPY payload /payload\n' >"$dir/Dockerfile"
  local args=(build -q -t "$tag")
  [[ -n "$platform" ]] && args+=(--platform "$platform")
  docker "${args[@]}" "$dir" >/dev/null
  rm -rf "$dir"
}

# expect_fail CMD...: the command must fail.
expect_fail() {
  if "$@" >/dev/null 2>&1; then
    fail "expected failure: $*"
  fi
}

# v2_status USER SECRET METHOD PATH: raw registry API status code.
v2_status() {
  curl -sS -o /dev/null -w '%{http_code}' -u "$1:$2" -X "$3" "$REGISTRY_URL$4"
}

# storage_has_blob DIGEST: whether the storage backend holds the blob.
storage_has_blob() {
  local hex="${1#sha256:}" key
  key="blobs/sha256/${hex:0:2}/$hex/data"
  if [[ "$MINREGISTRY_STORAGE" == s3 ]]; then
    compose exec -T minio sh -c \
      "mc alias set e2e http://127.0.0.1:9000 minioadmin minioadmin >/dev/null && mc stat e2e/minregistry/$key >/dev/null 2>&1"
  elif [[ "$E2E_REGISTRY" == compose ]]; then
    compose exec -T registry test -f "/data/blobs/$key"
  else
    test -f "$E2E_WORK/host-data/blobs/$key"
  fi
}

# unique PREFIX: a name unique to this run.
unique() { echo "$1-$E2E_RUN_ID"; }

# new_identity NAME: creates an identity with a token; sets ID and SECRET.
new_identity() {
  ID=$(create_identity "$1")
  SECRET=$(create_token "$ID" "$1")
  [[ -n "$ID" && -n "$SECRET" && "$SECRET" != null ]] || fail "could not create identity $1"
}
