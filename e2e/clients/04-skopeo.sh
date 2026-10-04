#!/usr/bin/env bash
# skopeo: copy into MinRegistry and out of it (both directions), inspect, delete.
source "$(dirname "$0")/../lib.sh"

user=$(unique e2e-skopeo)
new_identity "$user"
creds="$user:$SECRET"
repo="e2e/$(unique skopeo)/pause"
layout="$E2E_WORK/skopeo-layout"

skopeo copy -q --dest-tls-verify=false --dest-creds "$creds" \
  docker://registry.k8s.io/pause:3.10 "docker://$REGISTRY/$repo:in" || fail "skopeo copy into MinRegistry failed"
assert_audit "$user" manifest.push "$repo" in
pass "skopeo copy docker://registry.k8s.io → MinRegistry"

digest=$(skopeo inspect --tls-verify=false --creds "$creds" "docker://$REGISTRY/$repo:in" | jq -r .Digest)
[[ "$digest" == sha256:* ]] || fail "skopeo inspect returned no digest"
assert_audit "$user" manifest.pull "$repo" in
pass "skopeo inspect ($digest)"

skopeo copy -q --src-tls-verify=false --src-creds "$creds" \
  "docker://$REGISTRY/$repo:in" "oci:$layout:pause" || fail "skopeo copy out of MinRegistry failed"
[[ -f "$layout/index.json" ]] || fail "no OCI layout written"
skopeo copy -q --dest-tls-verify=false --dest-creds "$creds" \
  "oci:$layout:pause" "docker://$REGISTRY/$repo:roundtrip" || fail "skopeo copy of the OCI layout back failed"
assert_audit "$user" manifest.push "$repo" roundtrip
pass "skopeo copy MinRegistry → OCI layout → MinRegistry"

skopeo delete --tls-verify=false --creds "$creds" "docker://$REGISTRY/$repo:in" || fail "skopeo delete failed"
expect_fail skopeo inspect --tls-verify=false --creds "$creds" "docker://$REGISTRY/$repo:in"
assert_audit "$user" manifest.delete "$repo" "$digest"
pass "skopeo delete"
