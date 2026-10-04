#!/usr/bin/env bash
# oras: push an artifact with --artifact-type, attach to an image, discover
# (referrers API), pull.
source "$(dirname "$0")/../lib.sh"

user=$(unique e2e-oras)
new_identity "$user"
oras login --plain-http -u "$user" -p "$SECRET" "$REGISTRY" >/dev/null 2>&1 || fail "oras login failed"
crane auth login "$REGISTRY" -u "$user" -p "$SECRET" >/dev/null 2>&1 || fail "crane auth login failed"
repo="e2e/$(unique oras)"
dir=$(mktemp -d "$E2E_WORK/oras.XXXXXX")
cd "$dir"

echo "hello from $E2E_RUN_ID" >artifact.txt
oras push --plain-http --artifact-type application/vnd.minregistry.e2e.sample \
  "$REGISTRY/$repo/artifact:v1" artifact.txt:text/plain >/dev/null || fail "oras push failed"
assert_audit "$user" manifest.push "$repo/artifact" v1
oras manifest fetch --plain-http "$REGISTRY/$repo/artifact:v1" |
  jq -e '.artifactType == "application/vnd.minregistry.e2e.sample"' >/dev/null || fail "artifactType not preserved"
pass "oras push --artifact-type"

crane copy --platform linux/amd64 registry.k8s.io/pause:3.10 "$REGISTRY/$repo/image:v1" >/dev/null 2>&1 ||
  fail "could not seed the subject image"
echo '{"sbom":"e2e"}' >sbom.json
oras attach --plain-http --artifact-type application/vnd.minregistry.e2e.sbom \
  "$REGISTRY/$repo/image:v1" sbom.json:application/json >/dev/null || fail "oras attach failed"
subject=$(crane digest "$REGISTRY/$repo/image:v1")
pass "oras attach"

oras discover --plain-http --format json "$REGISTRY/$repo/image:v1" >discover.json || fail "oras discover failed"
grep -q 'application/vnd.minregistry.e2e.sbom' discover.json || fail "oras discover did not find the attached artifact"
assert_audit "$user" referrers.list "$repo/image"
api GET "/audit?action=referrers.list&repository=$repo/image&limit=50" |
  jq -e --arg d "$subject" '[.items[] | select(.digest == $d)] | length > 0' >/dev/null ||
  fail "the referrers audit event does not name the subject"
pass "oras discover (referrers API)"

mkdir out
oras pull --plain-http -o out "$REGISTRY/$repo/artifact:v1" >/dev/null || fail "oras pull failed"
cmp -s artifact.txt out/artifact.txt || fail "the pulled artifact differs"
assert_audit "$user" manifest.pull "$repo/artifact" v1
pass "oras pull"
