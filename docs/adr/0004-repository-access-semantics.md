# 4. Repository access semantics

Date: 2026-10-03 · Status: accepted

## Context

AGENTS.md §2 and §5.4 define `read`/`write`/`owner`, admin bypass, and
auto-create on push. Two points needed interpretation, and one rule conflicts
with real clients.

## Decision

1. **Deletes are writes.** `Delete` (manifests, tags and blobs through `/v2/`)
   requires `write`. `owner`'s extras — managing the repository's permissions
   and deleting the repository — are management-API operations, which only
   admins can call today (identities have no web login).
2. **`Mount`** is checked on the *source* repository and requires `read`
   (the target is checked as a `Push`). A refused mount falls back to a normal
   upload session (202), as the spec requires, and is audited as `denied`.
3. **A missing repository reads as empty.** AGENTS.md §5.4 asks for 403 when
   an authenticated principal touches a repository that does not exist, so
   that existence is not leaked. In practice crane, oras and skopeo check
   blob/manifest existence with `HEAD` *before* their first push to a new
   repository and abort on anything but 200/404 (docker happens to tolerate
   it); the distribution spec requires 404 for content a repository does not
   have. Following AGENTS.md §8 ("the client is right until proven otherwise
   with a spec citation"), every action on a missing repository is allowed for
   authenticated principals: reads find nothing (404 `NAME_UNKNOWN`) and pushes
   create it with the pusher as `owner`.

   This does not weaken confidentiality in practice: with auto-create, any
   principal can already tell whether a name exists by trying to push to it
   (202 for a free name, 403 for a taken one). Existing repositories without a
   grant still answer 403 to every request, and nothing about their content is
   revealed.

## Consequences

- The e2e permission scenario checks 403 for an existing repository without a
  grant (pull and push), and the integration tests check 404 for a missing one.
- If the owner prefers to hide existence completely, the alternative is to
  answer 404 for every request on an existing repository the principal cannot
  read (GitHub-style) — which contradicts the "403 on pull without a grant"
  requirement — or to drop auto-create.
