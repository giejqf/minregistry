# Common tasks. `just --list` shows them all.

set shell := ["bash", "-euo", "pipefail", "-c"]

dev_db := "sqlite://" + justfile_directory() + "/target/sqlx-dev.db"

default:
    @just --list

# Run the server locally (configure MINREGISTRY_* first; see docs/config.md).
serve:
    cargo run -p minregistry -- serve

# Recreate the development database the sqlx macros check queries against.
db-reset:
    mkdir -p target
    rm -f target/sqlx-dev.db target/sqlx-dev.db-shm target/sqlx-dev.db-wal
    DATABASE_URL={{dev_db}} cargo sqlx database create
    DATABASE_URL={{dev_db}} cargo sqlx migrate run --source server/migrations

# Build against the live dev database (use after editing queries or migrations).
build-live: db-reset
    DATABASE_URL={{dev_db}} cargo build --all-targets

# Regenerate the committed .sqlx/ offline query data.
sqlx-prepare: db-reset
    DATABASE_URL={{dev_db}} cargo sqlx prepare --workspace -- --all-targets

# Regenerate web/openapi.json and the frontend SDK from the backend.
openapi:
    cargo run -q -p minregistry -- openapi > web/openapi.json
    pnpm --dir web gen:sdk

# Backend checks (definition of done, part 1).
check-rust:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test
    cargo deny check licenses

# Frontend checks (definition of done, part 2).
check-web:
    pnpm --dir web install --frozen-lockfile
    pnpm --dir web lint
    pnpm --dir web typecheck
    pnpm --dir web test
    pnpm --dir web build

# Generated files are up to date (definition of done, part 3).
check-generated: openapi
    git diff --exit-code -- web/openapi.json web/src/sdk

# End-to-end: real clients + OCI conformance (storage = fs | s3).
e2e storage="fs":
    MINREGISTRY_STORAGE={{storage}} ./e2e/run.sh

check: check-rust check-web check-generated
