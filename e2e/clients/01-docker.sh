#!/usr/bin/env bash
# docker: login success/failure, push to a new repository (auto-create, the
# pusher becomes owner), pull by tag and by digest, multi-arch push + pull.
source "$(dirname "$0")/../lib.sh"

user=$(unique e2e-docker)
new_identity "$user"

expect_fail docker_login "$user" "mr_not-the-right-token"
assert_audit "$user" login "" "" denied
docker_login "$user" "$SECRET" || fail "docker login with a valid token failed"
assert_audit "$user" login "" "" ok
pass "docker login rejects a bad token and accepts a good one"

repo="e2e/$(unique app)"
image="$REGISTRY/$repo"
build_image "$image:v1"
docker push -q "$image:v1" >/dev/null || fail "docker push to a new repository failed"
assert_audit "$user" repository.create "$repo"
assert_audit "$user" manifest.push "$repo" v1
api GET "/repositories/$(repo_id "$repo")/permissions" |
  jq -e --arg n "$user" '.[] | select(.principal.name == $n and .level == "owner")' >/dev/null ||
  fail "the pusher is not the owner of $repo"
pass "push auto-created $repo with the pusher as owner"

digest=$(docker inspect --format '{{index .RepoDigests 0}}' "$image:v1")
digest="${digest#*@}"
docker rmi -f "$image:v1" >/dev/null
docker pull -q "$image:v1" >/dev/null || fail "docker pull by tag failed"
assert_audit "$user" manifest.pull "$repo" v1
docker rmi -f "$image:v1" >/dev/null
docker pull -q "$image@$digest" >/dev/null || fail "docker pull by digest failed"
assert_audit "$user" manifest.pull "$repo" "$digest"
pass "docker pull by tag and by digest"

multi_repo="e2e/$(unique multi)"
multi="$REGISTRY/$multi_repo"
for arch in amd64 arm64; do
  build_image "$multi:$arch" "linux/$arch"
  docker push -q "$multi:$arch" >/dev/null || fail "docker push $multi:$arch failed"
done
docker buildx imagetools create -t "$multi:latest" "$multi:amd64" "$multi:arm64" >/dev/null 2>&1 ||
  fail "docker buildx imagetools create failed"
docker buildx imagetools inspect --raw "$multi:latest" |
  jq -e '(.manifests | length) == 2 and ([.manifests[].platform.architecture] | sort) == ["amd64", "arm64"]' >/dev/null ||
  fail "the pushed index does not list both platforms"
docker rmi -f "$multi:latest" "$multi:amd64" "$multi:arm64" >/dev/null 2>&1 || true
docker pull -q "$multi:latest" >/dev/null || fail "docker pull of the multi-arch image failed"
assert_audit "$user" manifest.push "$multi_repo" latest
assert_audit "$user" manifest.pull "$multi_repo" latest
pass "multi-arch push (buildx imagetools create) and pull"
