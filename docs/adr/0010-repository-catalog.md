# 10. Repository catalog

Date: 2026-10-10 · Status: accepted

## Context

`GET /v2/_catalog` lists a registry's repositories. It belongs to the Docker
registry HTTP API, not to the OCI Distribution spec, so MinRegistry did not
serve it. Some registry clients need it. Synology DSM's Container Manager
reads it to show what a configured registry offers, and refuses a registry
without it. The official `registry` image serves it, so users expect it.

A full catalog would expose the names of repositories the caller cannot
read. ADR 0004 keeps those private: an existing repository without a grant
answers 403, and nothing about its content is revealed.

## Decision

- `GET /v2/_catalog` answers `{"repositories": [...]}` in name order, paged
  with `n`/`last` and a `Link: <...>; rel="next"` header exactly like the tag
  list. Without `n` it returns up to 10,000 names in one page.
- It lists only the repositories the principal may pull from: those with a
  grant (every level includes `read`), or all of them for admins. Deleted
  repositories are never listed.
- It requires credentials like every other `/v2/` endpoint and is audited as
  `catalog.list` with the number of names returned (`detail.count`). This
  extends the vocabulary of ADR 0008 and is documented in docs/audit.md.

## Consequences

Clients see their own catalog, not the registry's. Two identities can get
different answers, and an identity without grants gets an empty list. Tools
that mirror a whole registry must use an admin's token.
