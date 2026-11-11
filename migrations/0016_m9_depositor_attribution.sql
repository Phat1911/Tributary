-- M9 (D43): depositor-mode attribution — SCHEMA AS INTERFACE, not logic.
-- This file is part of Session A's doctrine suite: the columns exist so the
-- suite can RUN; every behavior that fills them is Session B's to implement.
-- Additive ONLY (doctrine invariant 10): every new column is nullable or
-- defaulted so every pre-M9 row keeps loading untouched, and no existing
-- column, table, or constraint is rewritten.
--
-- D43 in one line: pro-rata applies to ROUTING WEIGHTS, never to earnings
-- attribution — earnings follow the burn, per BUCKET (provider + model
-- family), and buckets live OFF-CHAIN (these books + M11 Merkle leaves;
-- the on-chain claim registry stays the aggregate per-depositor record).
--
-- Naming decision (M9-A GO refinement 2026-11-02, one word for Session B):
-- the SINGLE contributed-value source is `claims.contributed` — the
-- per-depositor liability account from migration 0015. Per-bucket amounts
-- below are POSITIONS that must always sum back to it. There is
-- deliberately NO weight column here: D43 makes claims.contributed the
-- only routing-weight source (a stored weight would be a second truth).
--
-- Portable SQL: TEXT/BIGINT only, ids minted in Rust, no DB-level FKs —
-- house style since 0001 (see 0015's header for the full rationale).

-- Who contributed this provider key. NULL = an operator/V1 org key (family
-- mode — depositor-mode logic never touches those rows; invariant 9).
ALTER TABLE provider_keys ADD COLUMN depositor_id TEXT;

-- Which model FAMILIES the key declares it can serve (JSON array of
-- strings, e.g. '["gpt-4o"]'). Empty = serves its provider's default
-- family set — the permissive default keeps every pre-M9 key loading and
-- behaving exactly as before (additive, invariant 9/10).
ALTER TABLE provider_keys ADD COLUMN model_families TEXT NOT NULL DEFAULT '[]';

-- Attribution lands AT SETTLE (A-020's request-settle moment — the usage
-- row), NEVER at reserve (invariant 11): the quota_buckets table from 0013
-- is intentionally left without any attribution columns. All three columns
-- stay NULL for V1 org-pool rows forever.
ALTER TABLE usage_events ADD COLUMN depositor_id TEXT;
ALTER TABLE usage_events ADD COLUMN bucket_provider TEXT;
ALTER TABLE usage_events ADD COLUMN bucket_model_family TEXT;

-- Per-depositor × per-bucket claim POSITIONS (invariant 10's "new claim
-- positions"): the M11 Merkle leaves. contributed_micros per bucket must
-- SUM to the depositor's claims.contributed balance on the M7 books;
-- burned_micros is the running metered burn attributed to that depositor's
-- keys in that bucket (the exhaustion ceiling — invariant 6: per-bucket
-- overdraw impossible, burned_micros may never exceed contributed_micros);
-- earned_micros is the settled D43 earnings credited in that bucket.
-- Money is integer micro-USD only (AGENTS.md) — no float column can exist.
CREATE TABLE IF NOT EXISTS depositor_bucket_positions (
    id                TEXT PRIMARY KEY,
    pool_id           TEXT NOT NULL,
    depositor_id      TEXT NOT NULL,
    provider          TEXT NOT NULL,
    model_family      TEXT NOT NULL,
    contributed_micros BIGINT NOT NULL DEFAULT 0,
    burned_micros     BIGINT NOT NULL DEFAULT 0,
    earned_micros     BIGINT NOT NULL DEFAULT 0,
    updated_at        BIGINT NOT NULL,
    UNIQUE (pool_id, depositor_id, provider, model_family)
);

-- The routing scans a bucket's depositors through this index (invariant 5/8).
CREATE INDEX IF NOT EXISTS idx_depositor_bucket_positions_bucket
    ON depositor_bucket_positions (pool_id, provider, model_family);
