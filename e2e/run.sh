#!/usr/bin/env bash
# End-to-end tests: real registry clients (docker, crane, skopeo, oras) and
# the official OCI distribution-spec conformance suite, against a MinRegistry
# started from the Dockerfile.
#
#   ./e2e/run.sh                       # everything, filesystem storage
#   MINREGISTRY_STORAGE=s3 ./e2e/run.sh  # same, against MinIO
#   ./e2e/run.sh 03-crane conformance  # only some scenarios
#
# Environment:
#   MINREGISTRY_STORAGE  fs (default) | s3
#   E2E_REGISTRY         compose (default: build the image) | host (run
#                        MINREGISTRY_BIN, default target/release/minregistry)
#   E2E_BUILD=0          use an existing minregistry:e2e image instead of
#                        building it (CI builds it with layer caching)
#   E2E_KEEP=1           leave the stack running afterwards
#
# Needs a Docker daemon (with buildx) and ports 5000, 5555 (and 9000/9001 for
# s3) free on localhost. Clients that are not installed run from their
# official images.

set -euo pipefail
E2E_DIR="$(cd "$(dirname "$0")" && pwd)"
export E2E_WORK="${E2E_WORK:-$E2E_DIR/.work}"
rm -rf "${E2E_WORK:?}"
mkdir -p "$E2E_WORK"
# shellcheck source=lib.sh
source "$E2E_DIR/lib.sh"

: "${E2E_KEEP:=0}"
E2E_SESSION_SECRET="$(head -c 32 /dev/urandom | base64)"
export E2E_SESSION_SECRET

cleanup() {
  local status=$?
  if [[ "$E2E_KEEP" == 1 ]]; then
    log "E2E_KEEP=1: leaving the stack running (stop it with: docker compose -f e2e/docker-compose.yml --profile registry --profile s3 down -v)"
    return
  fi
  log "tearing down"
  if [[ "$E2E_REGISTRY" == compose ]]; then
    compose logs registry >"$E2E_WORK/registry.log" 2>&1 || true
  elif [[ -f "$E2E_WORK/registry.pid" ]]; then
    kill "$(cat "$E2E_WORK/registry.pid")" 2>/dev/null || true
  fi
  compose down -v --remove-orphans >/dev/null 2>&1 || true
  docker images --format '{{.Repository}}:{{.Tag}}' | grep "^$REGISTRY/e2e/" | xargs -r docker rmi -f >/dev/null 2>&1 || true
  exit "$status"
}
trap cleanup EXIT

for port in 5000 5555 $([[ "$MINREGISTRY_STORAGE" == s3 ]] && echo 9000 9001); do
  if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
    fail "port $port is already in use"
  fi
done

log "starting the stack (storage: $MINREGISTRY_STORAGE, registry: $E2E_REGISTRY)"
build=(--build)
[[ "${E2E_BUILD:-1}" == 0 ]] && build=()
compose up -d "${build[@]}" --wait >"$E2E_WORK/compose-up.log" 2>&1 || {
  cat "$E2E_WORK/compose-up.log" >&2
  fail "docker compose up failed"
}

if [[ "$E2E_REGISTRY" == host ]]; then
  [[ -x "$MINREGISTRY_BIN" ]] || fail "$MINREGISTRY_BIN not found (cargo build --release)"
  (
    eval "$(registry_env)"
    exec "$MINREGISTRY_BIN" serve
  ) >"$E2E_WORK/registry.log" 2>&1 &
  echo $! >"$E2E_WORK/registry.pid"
fi

for _ in $(seq 1 120); do
  curl -sf "$REGISTRY_URL/readyz" >/dev/null 2>&1 && break
  sleep 0.5
done
curl -sf "$REGISTRY_URL/readyz" >/dev/null || fail "registry did not become ready"
pass "registry ready at $REGISTRY_URL"

status=$(admin_login)
[[ "$status" == 303 ]] || fail "admin sign-in through the fake GitHub failed ($status)"
pass "admin signed in through GitHub OAuth"

scenarios=("$@")
if [[ ${#scenarios[@]} -eq 0 ]]; then
  for f in "$E2E_DIR"/clients/[0-9]*.sh; do scenarios+=("$(basename "$f" .sh)"); done
  scenarios+=(conformance)
fi

failed=()
for s in "${scenarios[@]}"; do
  if [[ "$s" == conformance ]]; then
    script="$E2E_DIR/conformance/run.sh"
  else
    script=$(ls "$E2E_DIR"/clients/"$s"*.sh 2>/dev/null | head -1) || true
  fi
  [[ -n "${script:-}" && -f "$script" ]] || fail "unknown scenario $s"
  log "scenario: $s"
  if bash "$script"; then
    pass "scenario $s"
  else
    failed+=("$s")
    printf '\033[1;31m  ✗ scenario %s failed\033[0m\n' "$s" >&2
  fi
done

if [[ ${#failed[@]} -gt 0 ]]; then
  fail "failed scenarios: ${failed[*]}"
fi
log "all scenarios passed (storage: $MINREGISTRY_STORAGE)"
