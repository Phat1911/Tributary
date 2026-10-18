-- M4: leak-prevention free tier (SPEC §9) — IP allowlists, per-key IP
-- history (impossible travel groundwork), canary model aliases, the admin
-- review queue, the email-alert outbox, and the BYOK purpose classifier
-- (D28). pool_keys.status additionally takes 'suspended' (auto-block,
-- pending review — one-click unblock) and 'rotated' (reissue); those are
-- value-space extensions only (the column has no CHECK constraint by
-- design), documented here because both backends store plain TEXT.
--
-- Portable SQL: runs identically on SQLite and PostgreSQL (TEXT/BIGINT
-- only). Classifier scores/thresholds are INTEGER basis points (bp:
-- 0.5000 alignment = 5000) — exact cross-backend comparison, no float
-- storage anywhere (the AGENTS float rule targets money; we extend it to
-- scores for driver-exactness, not paranoia).

-- SPEC §6 `ip_allowlists`: CIDRs that pin a pool key to known networks.
-- Empty set (no rows) = unrestricted — the dormant-by-default pattern.
CREATE TABLE IF NOT EXISTS ip_allowlists (
    id          TEXT PRIMARY KEY,
    pool_key_id TEXT NOT NULL,
    cidr        TEXT NOT NULL,        -- a.b.c.d/n | a.b.c.d (=/32) | IPv6 forms
    note        TEXT,                 -- optional human hint ("office VPN")
    created_at  BIGINT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_ip_allowlists_key_cidr
    ON ip_allowlists (pool_key_id, cidr);
CREATE INDEX IF NOT EXISTS idx_ip_allowlists_key ON ip_allowlists (pool_key_id);

-- Per-key source-IP history (impossible-travel groundwork, SPEC §9). The
-- `country` column is filled ONLY when a GeoLite2 db is configured
-- (dormant-feature pattern: no GeoIP dependency in the default build).
CREATE TABLE IF NOT EXISTS pool_key_ips (
    pool_key_id TEXT NOT NULL,
    ip          TEXT NOT NULL,
    country     TEXT,                 -- ISO code when geo resolution is active
    first_seen  BIGINT NOT NULL,
    last_seen   BIGINT NOT NULL,
    PRIMARY KEY (pool_key_id, ip)
);

-- Canary model aliases (SPEC §9 "per-member canary model aliases"): look
-- like real models in GET /v1/models, but any request targeting one is a
-- detection. user_id NULL = visible pool-wide; a value scopes the alias to
-- that member's model listing (detection still trips for ANY requester —
-- a stolen model list is the threat, not the requester).
CREATE TABLE IF NOT EXISTS canary_models (
    id         TEXT PRIMARY KEY,
    pool_id    TEXT NOT NULL,
    alias      TEXT NOT NULL,
    user_id    TEXT,
    created_at BIGINT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_canary_models_pool_alias
    ON canary_models (pool_id, alias);
CREATE INDEX IF NOT EXISTS idx_canary_models_pool ON canary_models (pool_id);

-- Admin review queue: every free-tier detection lands here for a human
-- decision (D17: detection free, response automation paid — M4 ships
-- alert + manual action only). detail_json is SANITIZED at write time:
-- no prompts, no key material — masked prefixes, model names, IPs.
CREATE TABLE IF NOT EXISTS review_flags (
    id          TEXT PRIMARY KEY,
    org_id      TEXT NOT NULL,
    pool_id     TEXT,
    pool_key_id TEXT,
    user_id     TEXT,
    kind        TEXT NOT NULL,        -- 'honey_key_used' | 'canary_model' | 'classifier_flag' | 'impossible_travel'
    severity    TEXT NOT NULL,        -- 'low' | 'medium' | 'high'
    score_bp    BIGINT,               -- classifier alignment (bp); NULL for non-scored kinds
    detail_json TEXT NOT NULL,        -- sanitized context for the reviewer
    status      TEXT NOT NULL DEFAULT 'open',   -- 'open' | 'resolved'
    created_at  BIGINT NOT NULL,
    resolved_at BIGINT,
    resolved_by TEXT
);

CREATE INDEX IF NOT EXISTS idx_review_flags_org_status
    ON review_flags (org_id, status, created_at);

-- Email-alert outbox (async send, log-fallback when SMTP is unconfigured
-- — SPEC §9 free tier). Rows persist regardless so tests/ops can prove an
-- alert fired even on the fallback path.
CREATE TABLE IF NOT EXISTS email_alerts (
    id         TEXT PRIMARY KEY,
    org_id     TEXT NOT NULL,
    to_email   TEXT NOT NULL,
    subject    TEXT NOT NULL,
    body       TEXT NOT NULL,         -- plain text, no templating libs
    status     TEXT NOT NULL DEFAULT 'queued',  -- 'queued' | 'sent' | 'skipped'
    created_at BIGINT NOT NULL,
    sent_at    BIGINT
);

-- D28 BYOK classifier config: ONE row per org; NO row (or a deleted row)
-- ⇒ the feature is dormant. The API key is envelope-encrypted exactly like
-- provider keys (rule 2); thresholds are basis points.
CREATE TABLE IF NOT EXISTS classifier_config (
    org_id             TEXT PRIMARY KEY,
    base_url           TEXT NOT NULL,
    model              TEXT NOT NULL, -- model id lives in config, never code (SPEC §15)
    ciphertext         TEXT NOT NULL, -- sealed classifier API key
    dek_wrapped        TEXT NOT NULL,
    key_prefix         TEXT NOT NULL, -- display: first-5…last-4 ONLY
    purpose_statement  TEXT NOT NULL, -- what this org's pool is FOR
    mode               TEXT NOT NULL DEFAULT 'alert',  -- 'alert' | 'auto_block'
    alert_threshold_bp BIGINT NOT NULL DEFAULT 5000,   -- flag below 0.5 alignment (D28 rec.)
    block_threshold_bp BIGINT NOT NULL DEFAULT 1500,   -- suspend below 0.15 (D28 rec.)
    auto_block_live    BIGINT NOT NULL DEFAULT 0,      -- 0 = shadow, 1 = live
    shadow_started_at  BIGINT,                          -- when shadow mode began
    sampling_per_hour  BIGINT NOT NULL DEFAULT 30,      -- async sampling budget (N/hour)
    updated_at         BIGINT NOT NULL
);

-- One row per classification (async, sampled). Stores the SCORE and
-- verdict metadata — never the prompt (D28). would_block powers the
-- shadow-window statistics ("would have blocked N requests from M members").
CREATE TABLE IF NOT EXISTS classifier_scores (
    id          TEXT PRIMARY KEY,
    org_id      TEXT NOT NULL,
    pool_key_id TEXT,
    user_id     TEXT,
    model       TEXT NOT NULL,
    score_bp    BIGINT NOT NULL,
    would_block BIGINT NOT NULL DEFAULT 0,  -- score < block_threshold at score time
    ts          BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_classifier_scores_org_ts
    ON classifier_scores (org_id, ts);
