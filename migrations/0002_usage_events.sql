-- M1: usage metering — subset of SPEC §6 `usage_events`.
-- pool_id / provider_key_id arrive with M2+ (additive). cost_micros stays 0
-- until the pricing table lands (D23); it is ALWAYS integer micro-USD (rule:
-- money is integer micro-units only — no floats, ever).
CREATE TABLE IF NOT EXISTS usage_events (
    id                TEXT PRIMARY KEY,
    ts                BIGINT NOT NULL, -- unix epoch millis
    pool_key_id       TEXT NOT NULL,
    user_id           TEXT,
    model             TEXT NOT NULL,
    prompt_tokens     BIGINT NOT NULL DEFAULT 0,
    completion_tokens BIGINT NOT NULL DEFAULT 0,
    cost_micros       BIGINT NOT NULL DEFAULT 0, -- micro-USD; 0 in M1
    latency_ms        BIGINT NOT NULL DEFAULT 0,
    status            TEXT NOT NULL DEFAULT '200' -- upstream HTTP status as text
);

CREATE INDEX IF NOT EXISTS idx_usage_events_pool_key_ts
    ON usage_events (pool_key_id, ts);
