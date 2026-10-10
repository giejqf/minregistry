#!/usr/bin/env bash
# crane: copy from a public registry, ls, catalog, manifest of an index, delete by digest.
source "$(dirname "$0")/../lib.sh"

user=$(unique e2e-crane)
new_identity "$user"
crane auth login "$REGISTRY" -u "$user" -p "$SECRET" >/dev/null 2>&1 || fail "crane auth login failed"

repo="e2e/$(unique crane)/pause"
crane copy registry.k8s.io/pause:3.10 "$REGISTRY/$repo:3.10" >/dev/null 2>&1 || fail "crane copy from registry.k8s.io failed"
assert_audit "$user" manifest.push "$repo" 3.10
pass "crane copy registry.k8s.io/pause:3.10 (all platforms)"

crane ls "$REGISTRY/$repo" | grep -qx '3.10' || fail "crane ls does not list 3.10"
assert_audit "$user" tag.list "$repo"
pass "crane ls"

# The catalog lists only what this identity may pull: its own repository,
# none of those the other scenarios pushed.
catalog=$(crane catalog "$REGISTRY") || fail "crane catalog failed"
[[ "$catalog" == "$repo" ]] || fail "crane catalog listed: $(echo "$catalog" | tr '\n' ' ')"
assert_audit "$user" catalog.list ""
pass "crane catalog lists only readable repositories"

crane manifest "$REGISTRY/$repo:3.10" |
  jq -e '(.mediaType | test("index|manifest.list")) and (.manifests | length) > 1' >/dev/null ||
  fail "crane manifest did not return an index"
assert_audit "$user" manifest.pull "$repo" 3.10
pass "crane manifest on an index"

crane copy --platform linux/amd64 registry.k8s.io/pause:3.10 "$REGISTRY/$repo:single" >/dev/null 2>&1 ||
  fail "crane copy of a single platform failed"
digest=$(crane digest "$REGISTRY/$repo:single")
crane delete "$REGISTRY/$repo@$digest" || fail "crane delete by digest failed"
expect_fail crane manifest "$REGISTRY/$repo@$digest"
assert_audit "$user" manifest.delete "$repo" "$digest"
crane manifest "$REGISTRY/$repo:3.10" >/dev/null || fail "an unrelated tag disappeared"
pass "crane delete by digest"
