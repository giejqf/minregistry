#!/usr/bin/env bash
# A chunked upload of a large layer (128 MiB) that is interrupted mid-chunk
# and resumed from the offset the registry reports; then a ranged download,
# as an interrupted pull resumes.
source "$(dirname "$0")/../lib.sh"

user=$(unique e2e-chunked)
new_identity "$user"
auth="$user:$SECRET"
repo="e2e/$(unique chunked)"
work="${E2E_WORK:?}/chunked"
mkdir -p "$work"
blob="$work/layer.bin"
size=$((128 * 1024 * 1024))
head -c "$size" /dev/urandom >"$blob"
digest="sha256:$(sha256sum "$blob" | cut -d' ' -f1)"
first=$((80 * 1024 * 1024))

location=$(curl -sS -u "$auth" -X POST -D - -o /dev/null "$REGISTRY_URL/v2/$repo/blobs/uploads/" |
  tr -d '\r' | awk 'tolower($1) == "location:" {print $2}')
[[ "$location" == /v2/* ]] || fail "no upload session ($location)"
url="$REGISTRY_URL$location"

# First chunk (0..80 MiB), cut off after ~2 s at ~16 MB/s.
head -c "$first" "$blob" >"$work/chunk1.bin"
if curl -sS -u "$auth" -X PATCH -H 'Expect:' -H 'Content-Type: application/octet-stream' \
  -H "Content-Range: 0-$((first - 1))" --limit-rate 16M --max-time 2 \
  --data-binary @"$work/chunk1.bin" -o /dev/null "$url" 2>/dev/null; then
  fail "the first chunk was not interrupted"
fi

range=$(curl -sS -u "$auth" -D - -o /dev/null "$url" | tr -d '\r' | awk 'tolower($1) == "range:" {print $2}')
offset=$((${range#0-} + 1))
((offset > 0 && offset < first)) || fail "unexpected progress after the interruption: Range $range"
pass "interrupted after $offset bytes; the registry kept them (Range: $range)"

# Resume: the rest of the layer as the next chunk.
tail -c +"$((offset + 1))" "$blob" >"$work/rest.bin"
status=$(curl -sS -u "$auth" -X PATCH -H 'Expect:' -H 'Content-Type: application/octet-stream' \
  -H "Content-Range: $offset-$((size - 1))" --data-binary @"$work/rest.bin" -o /dev/null -w '%{http_code}' "$url")
[[ "$status" == 202 ]] || fail "resumed chunk: HTTP $status"
sep='?'
[[ "$url" == *\?* ]] && sep='&'
status=$(curl -sS -u "$auth" -X PUT -o /dev/null -w '%{http_code}' "$url${sep}digest=$digest")
[[ "$status" == 201 ]] || fail "completing the upload: HTTP $status"
api GET "/audit?action=blob.upload&repository=$repo&limit=10" |
  jq -e --arg d "$digest" --arg u "$user" '[.items[] | select(.digest == $d and .outcome == "ok" and .principal_name == $u)] | length == 1' >/dev/null ||
  fail "no blob.upload audit event for $digest"
pass "resumed and completed the 128 MiB upload ($digest)"

half=$((size / 2))
curl -sS -u "$auth" -r "0-$((half - 1))" -o "$work/part1.bin" "$REGISTRY_URL/v2/$repo/blobs/$digest"
curl -sS -u "$auth" -r "$half-" -o "$work/part2.bin" "$REGISTRY_URL/v2/$repo/blobs/$digest"
got="sha256:$(cat "$work/part1.bin" "$work/part2.bin" | sha256sum | cut -d' ' -f1)"
[[ "$got" == "$digest" ]] || fail "ranged download does not match ($got)"
pass "ranged download reassembles to the same digest"
rm -rf "${work:?}"
