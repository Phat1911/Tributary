-- M2: pools, provider keys (envelope-encrypted), quota rules — SPEC §6 subset.
-- provider_keys: ciphertext ONLY (never plaintext); key_prefix is the display
-- form (first-5…last-4); the master key lives in env/KMS, never here (rule 2).
CREATE TABLE IF NOT EXISTS pools (
    id              TEXT PRIMARY KEY,
    org_id          TEXT NOT NULL,
    name            TEXT NOT NULL,
    kind            TEXT NOT NULL DEFAULT 'org',    -- 'org' | 'root' (root pools arrive V2)
    model_allowlist TEXT NOT NULL DEFAULT '[]',     -- JSON array; empty = unrestricted
    status          TEXT NOT NULL DEFAULT 'active', -- 'active' | 'disabled'
    currency        TEXT NOT NULL DEFAULT 'usd',
    created_at      BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_pools_org ON pools (org_id);

-- Envelope layout (crates/crypto): per-row random 32-byte DEK encrypts the
-- provider key; the DEK itself is wrapped by the master key. Both blobs are
-- base64(24-byte-nonce || ciphertext+tag) and bound to this row by
-- associated data "provider_keys:<id>" (SPEC §8, D30).
-- base_url keeps upstream identities in data, never code (SPEC §15).
CREATE TABLE IF NOT EXISTS provider_keys (
    id                TEXT PRIMARY KEY,
    pool_id           TEXT NOT NULL,
    provider          TEXT NOT NULL DEFAULT 'openai',
    base_url          TEXT NOT NULL DEFAULT 'https://api.openai.com/v1',
    ciphertext        TEXT NOT NULL,       -- encrypted provider key material
    dek_wrapped       TEXT NOT NULL,       -- DEK wrapped by the master key
    key_prefix        TEXT NOT NULL,       -- display: first-5…last-4 ONLY
    health            TEXT NOT NULL DEFAULT 'unknown', -- 'ok' | 'invalid' | 'unknown' (M2 LIGHT: validate-on-save + manual revalidate)
    last_validated_at BIGINT,
    status            TEXT NOT NULL DEFAULT 'active',  -- 'active' | 'revoked'
    created_at        BIGINT NOT NULL
);

-- Key selection (first-available in M2; health-aware later) reads this index.
CREATE INDEX IF NOT EXISTS idx_provider_keys_pool
    ON provider_keys (pool_id, status, health, created_at);

-- Quota/rate engine (rule 4: enforced at the gateway BEFORE any upstream call).
-- subject_kind: 'pool_key' | 'user' | 'pool'. One rule per (subject, period):
-- limit_requests powers request-rate caps (period='minute'), limit_tokens
-- powers token quotas (period='day'|'month'); limit_cost stays 0 until the
-- price table lands (D23). M2 quotas are token-based per director ruling.
CREATE TABLE IF NOT EXISTS quota_rules (
    id             TEXT PRIMARY KEY,
    subject_kind   TEXT NOT NULL,
    subject_id     TEXT NOT NULL,
    period         TEXT NOT NULL,   -- 'minute' | 'hour' | 'day' | 'month'
    limit_tokens   BIGINT,          -- NULL = this rule sets no token limit
    limit_requests BIGINT,          -- NULL = this rule sets no request limit
    limit_cost     BIGINT NOT NULL DEFAULT 0, -- micro-USD; unused until D23
    created_at     BIGINT NOT NULL,
    UNIQUE (subject_kind, subject_id, period)
);

-- M1 planned this binding: pool keys gain their pool (additive; nullable so
-- M1-era rows keep loading — the gateway rejects poolless keys for chat).
ALTER TABLE pool_keys ADD COLUMN pool_id TEXT;

CREATE INDEX IF NOT EXISTS idx_pool_keys_pool ON pool_keys (pool_id, status);
