# 8. Audit event vocabulary

Date: 2026-10-03 · Status: accepted

## Context

AGENTS.md requires every meaningful action to be audited with who, what,
which repository/reference and from where, and treats the audit schema as a
public contract. The action names were not specified.

## Decision

Actions are dotted `noun.verb` names, listed in `audit::action::ALL` and
documented in [docs/audit.md](../audit.md), together with what `reference`,
`digest` and `detail` contain for each. Notable choices:

- Successful `GET /v2/` is `login` (that is what `docker login` does); invalid
  credentials on any `/v2/` request are `login`/`denied` with the claimed
  username. The unauthenticated first request of the challenge round-trip is
  not recorded.
- Manifest `HEAD` requests are recorded as `manifest.pull` with
  `detail.method = "HEAD"`: clients resolve a tag with `HEAD` and then fetch
  the digest, so this is the only record of which tag was pulled.
- Blob downloads are not recorded unless `MINREGISTRY_AUDIT_BLOB_READS=true`.
- Denials reuse the attempted action's name with `outcome = denied`.
- Background work is attributed to the principal name `system`.

## Consequences

New actions may be added (append to the list and the doc); renaming or
removing one needs a new ADR.
