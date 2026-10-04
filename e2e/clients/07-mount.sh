#!/usr/bin/env bash
# Cross-repository blob mount: pushing the same image to a second repository
# mounts its layer instead of uploading it again (docker, then crane).
source "$(dirname "$0")/../lib.sh"

user=$(unique e2e-mount)
new_identity "$user"
docker_login "$user" "$SECRET" || fail "docker login failed"
crane auth login "$REGISTRY" -u "$user" -p "$SECRET" >/dev/null 2>&1 || fail "crane auth login failed"

first="e2e/$(unique mount-a)"
second="e2e/$(unique mount-b)"
build_image "$REGISTRY/$first:v1"
docker push -q "$REGISTRY/$first:v1" >/dev/null || fail "first push failed"
layer=$(crane manifest "$REGISTRY/$first:v1" | jq -r '.layers[0].digest')
[[ $(audit_count "$user" blob.upload "$first" "$layer") == 1 ]] || fail "the first push did not upload the layer"

docker tag "$REGISTRY/$first:v1" "$REGISTRY/$second:v1"
docker push -q "$REGISTRY/$second:v1" >/dev/null || fail "second push failed"
[[ $(audit_count "$user" blob.mount "$second" "$layer") -ge 1 ]] || fail "docker did not mount $layer into $second"
[[ $(audit_count "$user" blob.upload "$second" "$layer") == 0 ]] || fail "the layer was uploaded again"
pass "docker: the second push mounted the layer from $first (no re-upload)"

third="e2e/$(unique mount-c)"
crane copy "$REGISTRY/$first:v1" "$REGISTRY/$third:v1" >/dev/null 2>&1 || fail "crane copy within the registry failed"
[[ $(audit_count "$user" blob.mount "$third" "$layer") -ge 1 ]] || fail "crane did not mount $layer into $third"
[[ $(audit_count "$user" blob.upload "$third" "$layer") == 0 ]] || fail "crane uploaded the layer again"
pass "crane: a copy within the registry mounted the layer"
