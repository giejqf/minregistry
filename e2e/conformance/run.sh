#!/usr/bin/env bash
# The official OCI distribution-spec conformance suite, all four workflows
# (pull, push, content discovery, content management), run as a token-only
# identity that owns the repositories it creates.
#
# The suite is built from github.com/opencontainers/distribution-spec at
# OCI_CONFORMANCE_VERSION with a local Go toolchain, or inside the golang
# image when Go is not installed.
source "$(dirname "$0")/../lib.sh"

: "${OCI_CONFORMANCE_VERSION:=v1.1.1}"
cache="$E2E_DIR/.cache/distribution-spec-$OCI_CONFORMANCE_VERSION"
binary="$cache/conformance/conformance.test"
reports="${E2E_WORK:?}/conformance"
mkdir -p "$reports"

if [[ ! -x "$binary" ]]; then
  log "building the conformance suite ($OCI_CONFORMANCE_VERSION)"
  if [[ ! -d "$cache" ]]; then
    git clone -q --depth 1 --branch "$OCI_CONFORMANCE_VERSION" \
      https://github.com/opencontainers/distribution-spec.git "$cache" || fail "cloning distribution-spec failed"
  fi
  if command -v go >/dev/null 2>&1; then
    (cd "$cache/conformance" && go test -c -o conformance.test) || fail "building the suite failed"
  else
    docker run --rm -v "$cache:/src" -w /src/conformance -u "$(id -u):$(id -g)" -e HOME=/tmp -e GOCACHE=/tmp/go-cache \
      -e GOPATH=/tmp/go golang:1 go test -c -o conformance.test || fail "building the suite failed"
  fi
fi

user=$(unique conformance)
new_identity "$user"

log "running the OCI conformance suite as $user"
(
  cd "$reports"
  export OCI_ROOT_URL="$REGISTRY_URL"
  export OCI_NAMESPACE="conformance/$E2E_RUN_ID/main"
  export OCI_CROSSMOUNT_NAMESPACE="conformance/$E2E_RUN_ID/mount"
  export OCI_USERNAME="$user"
  export OCI_PASSWORD="$SECRET"
  export OCI_TEST_PULL=1 OCI_TEST_PUSH=1 OCI_TEST_CONTENT_DISCOVERY=1 OCI_TEST_CONTENT_MANAGEMENT=1
  # Mounting without `from` is not supported: it opens a regular upload session.
  export OCI_AUTOMATIC_CROSSMOUNT=false
  export OCI_HIDE_SKIPPED_WORKFLOWS=0
  export OCI_REPORT_DIR="$reports"
  "$binary" -test.v >"$reports/output.log" 2>&1
) || {
  tail -60 "$reports/output.log" >&2
  fail "OCI conformance suite failed (reports in $reports)"
}

grep -E 'Passed|Ran [0-9]+ of' "$reports/output.log" | tail -2 >&2
grep -q 'SUCCESS!' "$reports/output.log" || fail "OCI conformance suite did not report success"
if grep -qE '[1-9][0-9]* Failed' "$reports/output.log"; then
  fail "OCI conformance failures (see $reports/report.html)"
fi
assert_audit "$user" repository.create "conformance/$E2E_RUN_ID/main"
assert_audit "$user" blob.mount "conformance/$E2E_RUN_ID/mount"
pass "OCI distribution-spec $OCI_CONFORMANCE_VERSION conformance: pull, push, content discovery, content management"
