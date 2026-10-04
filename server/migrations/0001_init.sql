-- MinRegistry initial schema. Append-only: never edit a merged migration.
-- Timestamps are RFC 3339 UTC strings with millisecond precision
-- (e.g. 2026-10-03T19:07:00.123Z), which sort lexically.

CREATE TABLE principals (
    id           INTEGER PRIMARY KEY,
    kind         TEXT    NOT NULL CHECK (kind IN ('github', 'identity')),
    name         TEXT    NOT NULL UNIQUE,
    github_id    INTEGER UNIQUE,
    display_name TEXT    NOT NULL,
    enabled      INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at   TEXT    NOT NULL,
    CHECK ((kind = 'github') = (github_id IS NOT NULL))
);

CREATE TABLE tokens (
    id           INTEGER PRIMARY KEY,
    principal_id INTEGER NOT NULL REFERENCES principals (id),
    name         TEXT    NOT NULL,
    hash         TEXT    NOT NULL UNIQUE,
    prefix       TEXT    NOT NULL,
    created_by   INTEGER REFERENCES principals (id),
    created_at   TEXT    NOT NULL,
    expires_at   TEXT,
    revoked_at   TEXT,
    last_used_at TEXT
);
CREATE INDEX tokens_principal ON tokens (principal_id);

CREATE TABLE repositories (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    created_by INTEGER REFERENCES principals (id),
    created_at TEXT NOT NULL,
    deleted_at TEXT
);

CREATE TABLE permissions (
    principal_id  INTEGER NOT NULL REFERENCES principals (id),
    repository_id INTEGER NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    level         TEXT    NOT NULL CHECK (level IN ('read', 'write', 'owner')),
    granted_by    INTEGER REFERENCES principals (id),
    granted_at    TEXT    NOT NULL,
    PRIMARY KEY (principal_id, repository_id)
);
CREATE INDEX permissions_repository ON permissions (repository_id);

-- Content-addressed, global. Manifests are blobs too (stored by digest).
CREATE TABLE blobs (
    digest     TEXT NOT NULL PRIMARY KEY,
    size       INTEGER NOT NULL,
    created_at TEXT    NOT NULL
);

-- Which repositories may serve which blobs. created_at is the link time and
-- protects freshly uploaded blobs from garbage collection (docs/adr/0003).
CREATE TABLE repository_blobs (
    repository_id INTEGER NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    digest        TEXT    NOT NULL REFERENCES blobs (digest),
    created_at    TEXT    NOT NULL,
    PRIMARY KEY (repository_id, digest)
);
CREATE INDEX repository_blobs_digest ON repository_blobs (digest);

CREATE TABLE manifests (
    repository_id  INTEGER NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    digest         TEXT    NOT NULL,
    media_type     TEXT    NOT NULL,
    size           INTEGER NOT NULL,
    subject_digest TEXT,
    artifact_type  TEXT,
    annotations    TEXT, -- JSON object, served by the referrers API
    platforms      TEXT, -- JSON array of {os, architecture, variant}
    created_at     TEXT    NOT NULL,
    pushed_by      INTEGER REFERENCES principals (id),
    PRIMARY KEY (repository_id, digest)
);
CREATE INDEX manifests_subject ON manifests (repository_id, subject_digest);
CREATE INDEX manifests_digest ON manifests (digest);

CREATE TABLE manifest_refs (
    repository_id   INTEGER NOT NULL,
    manifest_digest TEXT    NOT NULL,
    child_digest    TEXT    NOT NULL,
    kind            TEXT    NOT NULL CHECK (kind IN ('blob', 'manifest')),
    PRIMARY KEY (repository_id, manifest_digest, child_digest, kind),
    FOREIGN KEY (repository_id, manifest_digest)
        REFERENCES manifests (repository_id, digest) ON DELETE CASCADE
);
CREATE INDEX manifest_refs_child ON manifest_refs (child_digest);

CREATE TABLE tags (
    repository_id   INTEGER NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    name            TEXT    NOT NULL,
    manifest_digest TEXT    NOT NULL,
    updated_at      TEXT    NOT NULL,
    updated_by      INTEGER REFERENCES principals (id),
    PRIMARY KEY (repository_id, name),
    FOREIGN KEY (repository_id, manifest_digest)
        REFERENCES manifests (repository_id, digest) ON DELETE CASCADE
);
CREATE INDEX tags_manifest ON tags (repository_id, manifest_digest);

CREATE TABLE uploads (
    uuid             TEXT NOT NULL PRIMARY KEY,
    repository_id    INTEGER NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    principal_id     INTEGER NOT NULL REFERENCES principals (id),
    "offset"         INTEGER NOT NULL DEFAULT 0,
    started_at       TEXT    NOT NULL,
    last_activity_at TEXT    NOT NULL
);

-- Append-only. The only DELETE path is the retention job.
CREATE TABLE audit_events (
    id             INTEGER PRIMARY KEY,
    ts             TEXT NOT NULL,
    principal_id   INTEGER,
    principal_name TEXT,
    action         TEXT NOT NULL,
    repository     TEXT,
    reference      TEXT,
    digest         TEXT,
    client_ip      TEXT,
    user_agent     TEXT,
    outcome        TEXT NOT NULL CHECK (outcome IN ('ok', 'denied', 'error')),
    detail         TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX audit_events_ts ON audit_events (ts);
CREATE INDEX audit_events_principal ON audit_events (principal_name, id);
CREATE INDEX audit_events_repository ON audit_events (repository, id);
CREATE INDEX audit_events_action ON audit_events (action, id);

CREATE TRIGGER audit_events_append_only
BEFORE UPDATE ON audit_events
BEGIN
    SELECT RAISE(ABORT, 'audit_events is append-only');
END;

-- Admin web sessions (tower-sessions store). data is a JSON object.
CREATE TABLE sessions (
    id          TEXT NOT NULL PRIMARY KEY,
    data        TEXT    NOT NULL,
    expiry_date INTEGER NOT NULL -- unix seconds
);
CREATE INDEX sessions_expiry ON sessions (expiry_date);

-- Blobs chosen for deletion by GC whose storage objects are not deleted yet.
-- Makes GC resumable; uploads of the same digest remove their row (docs/adr/0003).
CREATE TABLE gc_sweep (
    digest    TEXT NOT NULL PRIMARY KEY,
    marked_at TEXT NOT NULL
);
