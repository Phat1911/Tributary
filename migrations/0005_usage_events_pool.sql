-- M1 planned this additive change (see migrations/0002 comment): usage rows
-- gain their pool + provider-key binding so quotas, dashboards (M3), and the
-- ledger (M7) can attribute every metered request.
ALTER TABLE usage_events ADD COLUMN pool_id TEXT;
ALTER TABLE usage_events ADD COLUMN provider_key_id TEXT;

-- Quota checks aggregate per subject within a period window (SPEC §5 step 3):
-- user-scoped and pool-scoped sums hit these indexes; pool_key-scoped sums
-- hit the M1 index on (pool_key_id, ts).
CREATE INDEX IF NOT EXISTS idx_usage_events_pool_ts ON usage_events (pool_id, ts);
CREATE INDEX IF NOT EXISTS idx_usage_events_user_ts ON usage_events (user_id, ts);
