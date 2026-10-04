#!/usr/bin/env bash
# The permission matrix with docker: read cannot push (403, audited denied),
# no grant cannot pull (403), a revoked token gets 401.
source "$(dirname "$0")/../lib.sh"

owner=$(unique e2e-owner)
new_identity "$owner"
owner_secret="$SECRET"
repo="e2e/$(unique private)"
image="$REGISTRY/$repo"
docker_login "$owner" "$owner_secret" || fail "owner login failed"
build_image "$image:v1"
docker push -q "$image:v1" >/dev/null || fail "owner push failed"

reader=$(unique e2e-reader)
new_identity "$reader"
reader_id="$ID"
reader_secret="$SECRET"
grant "$repo" "$reader_id" read

docker_login "$reader" "$reader_secret" || fail "reader login failed"
docker rmi -f "$image:v1" >/dev/null
docker pull -q "$image:v1" >/dev/null || fail "a reader cannot pull"
build_image "$image:v2"
expect_fail docker push -q "$image:v2"
[[ $(v2_status "$reader" "$reader_secret" POST "/v2/$repo/blobs/uploads/") == 403 ]] || fail "read-only push is not a 403"
assert_audit "$reader" blob.upload "$repo" "" denied
pass "read grant: pull allowed, push denied with 403"

stranger=$(unique e2e-stranger)
new_identity "$stranger"
stranger_secret="$SECRET"
docker_login "$stranger" "$stranger_secret" || fail "stranger login failed"
docker rmi -f "$image:v1" >/dev/null 2>&1 || true
expect_fail docker pull -q "$image:v1"
[[ $(v2_status "$stranger" "$stranger_secret" GET "/v2/$repo/manifests/v1") == 403 ]] || fail "pull without a grant is not a 403"
[[ $(v2_status "$stranger" "$stranger_secret" POST "/v2/$repo/blobs/uploads/") == 403 ]] || fail "push without a grant is not a 403"
assert_audit "$stranger" manifest.pull "$repo" "" denied
pass "no grant: pull and push denied with 403"

token_id=$(api GET "/principals/$reader_id/tokens" | jq -r '.[0].id')
api DELETE "/principals/$reader_id/tokens/$token_id" >/dev/null
expect_fail docker_login "$reader" "$reader_secret"
[[ $(v2_status "$reader" "$reader_secret" GET "/v2/$repo/manifests/v1") == 401 ]] || fail "a revoked token is not a 401"
assert_audit "$reader" login "" "" denied
assert_audit "$E2E_ADMIN_LOGIN" token.revoke "" "" ok
pass "revoked token: 401"
