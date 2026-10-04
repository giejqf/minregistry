# 2. GitHub principals and the admin allowlist

Date: 2026-10-03 · Status: accepted

## Context

Admins sign in with GitHub and are listed in `MINREGISTRY_ADMIN_GITHUB_LOGINS`
(AGENTS.md §2). Admins are also principals so they can mint tokens for
`docker login`. Identities (token-only) and GitHub principals share the
`principals.name` namespace, which is also the Basic-auth username.

## Decision

- A GitHub principal is created on its first successful sign-in, keyed by the
  immutable GitHub user id; its `name` is the lower-cased login (renames are
  followed on the next sign-in). Admin status is **not stored**: a GitHub
  principal is an admin exactly while its login is in the allowlist.
- A GitHub principal whose login is removed from the allowlist can no longer
  authenticate at all — neither the web UI nor its registry tokens. Removing
  someone from the list must remove their access, and GitHub principals have
  no other purpose than being admins.
- Usernames are matched case-insensitively (lower-cased) for Basic auth.
- Identity names: `^[a-z0-9]([a-z0-9._-]{0,62}[a-z0-9])?$`; names on the admin
  allowlist are reserved. If a GitHub login collides with an existing identity,
  that sign-in is refused (403) rather than merging the two.
- Tokens for a GitHub principal can only be created by that admin; any admin
  may revoke any token. GitHub principals cannot be disabled or renamed through
  the API (the allowlist is the control).
- Tokens are `mr_` + 43 base64url characters (32 random bytes); the
  SHA-256 hex digest and the first 8 characters are stored. Verification
  compares against every active token of the principal in constant time.
  `last_used_at` is written at most once per minute per token, asynchronously.

## Consequences

Config changes take effect on the next request without a restart or a sign-out.
A former admin's principal row (and audit history) remains, shown as inactive.
