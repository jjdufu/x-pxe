-- x-pxe 初始结构。时间统一为 UTC RFC 3339（`YYYY-MM-DDTHH:MM:SSZ`，定长，可按文本比较）。

CREATE TABLE meta (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT;

CREATE TABLE users (
    id                   INTEGER PRIMARY KEY,
    username             TEXT NOT NULL UNIQUE,
    password_hash        TEXT NOT NULL,
    role                 TEXT NOT NULL CHECK (role IN ('admin', 'operator', 'viewer')),
    must_change_password INTEGER NOT NULL DEFAULT 0 CHECK (must_change_password IN (0, 1)),
    disabled             INTEGER NOT NULL DEFAULT 0 CHECK (disabled IN (0, 1)),
    created_at           TEXT NOT NULL
) STRICT;

CREATE TABLE sessions (
    id_hash      TEXT PRIMARY KEY NOT NULL,
    user_id      INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    csrf_hash    TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    expires_at   TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    ip           TEXT NOT NULL,
    user_agent   TEXT NOT NULL
) STRICT;
CREATE INDEX sessions_user ON sessions (user_id);

CREATE TABLE api_tokens (
    id           INTEGER PRIMARY KEY,
    token_hash   TEXT NOT NULL UNIQUE,
    user_id      INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    scopes       TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    expires_at   TEXT NOT NULL,
    last_used_at TEXT
) STRICT;

CREATE TABLE login_failures (
    scope     TEXT NOT NULL CHECK (scope IN ('user', 'ip')),
    subject   TEXT NOT NULL,
    failures  INTEGER NOT NULL,
    last_at   TEXT NOT NULL,
    PRIMARY KEY (scope, subject)
) STRICT;

CREATE TABLE config_objects (
    id           INTEGER PRIMARY KEY,
    kind         TEXT NOT NULL,
    name         TEXT NOT NULL,
    head_version INTEGER NOT NULL,
    UNIQUE (kind, name)
) STRICT;

CREATE TABLE config_versions (
    object_id      INTEGER NOT NULL REFERENCES config_objects (id),
    version        INTEGER NOT NULL,
    parent_version INTEGER,
    author_id      INTEGER REFERENCES users (id) ON DELETE SET NULL,
    created_at     TEXT NOT NULL,
    comment        TEXT NOT NULL,
    schema         INTEGER NOT NULL,
    content        TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    PRIMARY KEY (object_id, version)
) STRICT;

CREATE TABLE nodes (
    id                INTEGER PRIMARY KEY,
    spec_object_id    INTEGER NOT NULL UNIQUE REFERENCES config_objects (id),
    mac               TEXT NOT NULL UNIQUE,
    hostname          TEXT NOT NULL UNIQUE,
    state             TEXT NOT NULL,
    state_since       TEXT NOT NULL,
    deadline_at       TEXT,
    active_session_id TEXT,
    last_event_id     TEXT
) STRICT;

CREATE TABLE install_sessions (
    id                TEXT PRIMARY KEY NOT NULL,
    node_id           INTEGER NOT NULL REFERENCES nodes (id),
    channel           TEXT NOT NULL,
    boot_token_hash   TEXT NOT NULL UNIQUE,
    agent_token_hash  TEXT NOT NULL UNIQUE,
    created_at        TEXT NOT NULL,
    expires_at        TEXT NOT NULL,
    closed_at         TEXT,
    preseed_served_at TEXT,
    plan_served_at    TEXT,
    pinned            TEXT NOT NULL,
    outcome           TEXT
) STRICT;
CREATE INDEX install_sessions_node ON install_sessions (node_id);

CREATE TABLE node_events (
    id         INTEGER PRIMARY KEY,
    node_id    INTEGER NOT NULL REFERENCES nodes (id),
    session_id TEXT,
    event_id   TEXT NOT NULL,
    at         TEXT NOT NULL,
    source     TEXT NOT NULL,
    kind       TEXT NOT NULL,
    from_state TEXT NOT NULL,
    to_state   TEXT NOT NULL,
    payload    TEXT NOT NULL,
    UNIQUE (node_id, event_id)
) STRICT;

CREATE TABLE bmc_secrets (
    bmc_object_id INTEGER PRIMARY KEY NOT NULL,
    key_id        TEXT NOT NULL,
    nonce         BLOB NOT NULL,
    ciphertext    BLOB NOT NULL
) STRICT;

CREATE TABLE assets (
    id         INTEGER PRIMARY KEY,
    kind       TEXT NOT NULL,
    label      TEXT NOT NULL,
    source     TEXT NOT NULL CHECK (source IN ('official', 'url', 'upload')),
    source_url TEXT,
    sha256     TEXT NOT NULL,
    size       INTEGER NOT NULL,
    verify     TEXT NOT NULL,
    kernel_abi TEXT,
    stage      TEXT NOT NULL CHECK (stage IN ('staged', 'verified', 'canary_passed', 'retired')),
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;
CREATE INDEX assets_sha256 ON assets (sha256);

CREATE TABLE bootnet_observations (
    id         INTEGER PRIMARY KEY,
    at         TEXT NOT NULL,
    mac        TEXT NOT NULL,
    arch       TEXT NOT NULL,
    user_class TEXT NOT NULL,
    kind       TEXT NOT NULL,
    detail     TEXT NOT NULL
) STRICT;
CREATE INDEX bootnet_observations_at ON bootnet_observations (at);
CREATE INDEX bootnet_observations_mac ON bootnet_observations (mac);

CREATE TABLE audit_log (
    id        INTEGER PRIMARY KEY,
    at        TEXT NOT NULL,
    actor     TEXT NOT NULL,
    ip        TEXT NOT NULL,
    action    TEXT NOT NULL,
    target    TEXT NOT NULL,
    outcome   TEXT NOT NULL,
    detail    TEXT NOT NULL,
    prev_hash TEXT NOT NULL,
    hash      TEXT NOT NULL
) STRICT;
