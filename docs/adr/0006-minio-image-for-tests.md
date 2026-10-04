# 6. MinIO image used by the tests

Date: 2026-10-03 · Status: accepted

## Context

AGENTS.md specifies MinIO for the S3 storage contract tests and the S3 e2e
run. Since late 2025 MinIO no longer publishes community container images or
binaries: `minio/minio` is gone from Docker Hub, `quay.io/minio/minio` is not
publicly pullable, and `dl.min.io` answers `410 Gone`. Building MinIO from
source in every CI run would add minutes and an AGPL toolchain step.

## Decision

Use Bitnami's last published MinIO build, pinned:
`bitnamilegacy/minio:2025.7.23-debian-12-r5` (amd64 and arm64). It is a real
MinIO server, creates buckets from `MINIO_DEFAULT_BUCKETS`, and ships `mc`
(used by the e2e GC scenario to check storage directly). It is only a test
dependency; nothing of it is distributed with MinRegistry.

## Consequences

The image will not receive updates. If it disappears, build MinIO from a
release tag of its source, or switch the tests to another S3-compatible
server (the storage contract suite is backend-agnostic) and update AGENTS.md.
