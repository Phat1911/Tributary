-- M2: control-plane identities — SPEC §6 subset (organizations, users, sessions).
-- Portable SQL: runs identically on SQLite and PostgreSQL (TEXT/BIGINT only).
CREATE TABLE IF NOT EXISTS organizations (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    slug       TEXT NOT NULL UNIQUE,
    settings   TEXT NOT NULL DEFAULT '{}', -- JSON blob; unused in M2, kept per SPEC §6
    created_at BIGINT NOT NULL             -- unix epoch millis
);

-- Passwords are argon2 PHC strings (D8 built-in lightweight auth).
-- role: 'host_admin' (instance owner) | 'org_admin' | 'member' (no control-plane
-- access until M3 self-service).
CREATE TABLE IF NOT EXISTS users (
    id            TEXT PRIMARY KEY,
    email         TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL DEFAULT 'member',
    org_id        TEXT,                    -- NULL for host_admin
    created_at    BIGINT NOT NULL
);

-- Session tokens: raw token lives only in the admin's client; only the
-- SHA-256 hex digest is stored (same hygiene as pool_keys.key_hash).
CREATE TABLE IF NOT EXISTS sessions (
    id         TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    created_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL             -- unix epoch millis
);
