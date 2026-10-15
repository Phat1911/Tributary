-- M1: pool API keys — subset of SPEC §6 `pool_keys`.
-- pool_id / scopes / limits / last_used_at arrive with M2+ (additive migrations only, SPEC §15).
-- Portable SQL: runs identically on SQLite and PostgreSQL (TEXT/BIGINT only).
CREATE TABLE IF NOT EXISTS pool_keys (
    id         TEXT PRIMARY KEY,
    key_hash   TEXT NOT NULL UNIQUE, -- hex(SHA-256(raw key)); raw key never stored
    key_prefix TEXT NOT NULL,        -- display form: prefix + first-5 … last-4
    kind       TEXT NOT NULL DEFAULT 'live', -- 'live' | 'canary' (canary logic lands M4)
    status     TEXT NOT NULL DEFAULT 'active', -- 'active' | 'revoked'
    expires_at BIGINT,               -- unix epoch millis; NULL = never expires
    user_id    TEXT,                 -- nullable until org membership exists (M2)
    created_at BIGINT NOT NULL
);
