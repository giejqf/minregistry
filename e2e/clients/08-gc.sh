#!/usr/bin/env bash
# Garbage collection: delete a tag, run `minregistry gc --dry-run` then `gc`;
# the unreferenced blobs are gone from storage and the remaining image pulls.
source "$(dirname "$0")/../lib.sh"

user=$(unique e2e-gc)
new_identity "$user"
docker_login "$user" "$SECRET" || fail "docker login failed"
crane auth login "$REGISTRY" -u "$user" -p "$SECRET" >/dev/null 2>&1 || fail "crane auth login failed"
repo="e2e/$(unique gc)"
image="$REGISTRY/$repo"
build_image "$image:old"
build_image "$image:keep"
docker push -q "$image:old" >/dev/null || fail "push of :old failed"
docker push -q "$image:keep" >/dev/null || fail "push of :keep failed"
old_layer=$(crane manifest "$image:old" | jq -r '.layers[0].digest')
old_manifest=$(crane digest "$image:old")
keep_layer=$(crane manifest "$image:keep" | jq -r '.layers[0].digest')
storage_has_blob "$old_layer" || fail "the old layer is not in storage after the push"

[[ $(v2_status "$user" "$SECRET" DELETE "/v2/$repo/manifests/old") == 202 ]] || fail "tag delete failed"
assert_audit "$user" tag.delete "$repo" old
docker rmi -f "$image:old" "$image:keep" >/dev/null

report=$(registry_cli gc --dry-run --delete-untagged --min-age 0s) || fail "gc --dry-run failed"
echo "$report" | jq -e '.dry_run and .manifests_deleted >= 1 and .blobs_deleted >= 3' >/dev/null ||
  fail "unexpected dry-run report: $report"
storage_has_blob "$old_layer" || fail "the dry run deleted the blob"
pass "gc --dry-run reports the untagged image without deleting it"

report=$(registry_cli gc --delete-untagged --min-age 0s) || fail "gc failed"
echo "$report" | jq -e '(.dry_run | not) and .blobs_deleted >= 3 and .errors == 0' >/dev/null ||
  fail "unexpected gc report: $report"
if storage_has_blob "$old_layer"; then
  fail "the unreferenced layer is still in storage"
fi
storage_has_blob "$keep_layer" || fail "the referenced layer was deleted"
[[ $(v2_status "$user" "$SECRET" GET "/v2/$repo/manifests/$old_manifest") == 404 ]] || fail "the untagged manifest survived"
pass "gc deleted the unreferenced blobs from storage"

docker pull -q "$image:keep" >/dev/null || fail "the remaining image no longer pulls"
pass "the remaining image still pulls"

api GET "/audit?principal=system&action=gc.run&limit=10" |
  jq -e '[.items[] | select(.detail.trigger == "cli" and .detail.report.dry_run == false)] | length >= 1' >/dev/null ||
  fail "the gc run is not audited"
pass "audit: (system, gc.run) with the report"

api POST /gc '{"dry_run": true}' | jq -e '.dry_run' >/dev/null || fail "POST /api/v1/gc dry run failed"
assert_audit "$E2E_ADMIN_LOGIN" gc.run "" "" ok
pass "management API dry run"
