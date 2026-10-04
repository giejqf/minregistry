# 1. Record architecture decisions

Date: 2026-10-03 · Status: accepted

## Context

AGENTS.md fixes the product decisions. Implementation choices that are not
obvious from the code, or that interpret AGENTS.md, need a durable record so
later contributors (human or agent) do not undo them by accident.

## Decision

Non-obvious architectural choices are recorded as `docs/adr/NNNN-title.md`
(context, decision, consequences), numbered sequentially and never renumbered.
A superseded ADR stays, with its status pointing at the replacement. Changes to
the storage key layout or the audit schema always need an ADR.

## Consequences

Reviewers check that a PR changing one of these areas updates or adds an ADR.
