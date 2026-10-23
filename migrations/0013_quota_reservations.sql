-- A-020 (M5 hardening, director ruling 2026-10-22): quota reservations.
-- The M2 admission check read usage aggregates BEFORE the upstream call,
-- but usage only lands at response END — a concurrent burst fit N times
-- through a cap that admits one (TOCTOU). Fix shape (per the ruling):
-- reserve→settle→release over counter rows.
--
-- One row per (subject, period, window) holds the RESERVED (in-flight)
-- token/request amounts. Admission does ONE conditional UPDATE:
--   UPDATE quota_buckets SET reserved = reserved + estimate
--   WHERE subject… AND reserved + estimate + usage <= limit
-- A single UPDATE is the atomicity unit on BOTH engines: PostgreSQL
-- row-locks the bucket and re-evaluates the WHERE against the committed
-- winner (EvalPlanQual on READ COMMITTED); SQLite serializes writers.
-- Settle (response end) decrements the reservation and appends the real
-- usage event; release (error/cancel) decrements without usage; a TTL
-- sweep drops windows that have rolled over, reclaiming reservations
-- orphaned by a crashed process (they can only over-reserve, never
-- under-reserve, and die with their window).
--
-- Cost-unit reservation (micro-USD) joins when the D23 price table lands;
-- today the enforced units are tokens and requests (the enforced limit
-- columns), and the estimate is token-based by the same token→cost math
-- the meter will use.
CREATE TABLE IF NOT EXISTS quota_buckets (
    subject_kind     TEXT   NOT NULL,   -- 'pool_key' | 'user' | 'pool'
    subject_id       TEXT   NOT NULL,
    period           TEXT   NOT NULL,   -- 'minute' | 'hour' | 'day' | 'month'
    window_start     BIGINT NOT NULL,   -- epoch ms, aligned period start
    reserved_tokens  BIGINT NOT NULL DEFAULT 0,
    reserved_requests BIGINT NOT NULL DEFAULT 0,
    created_at       BIGINT NOT NULL,
    PRIMARY KEY (subject_kind, subject_id, period, window_start)
);

-- The sweep's lookup path (drop rolled-over windows). INCLUDE would be
-- nice on PG; plain INDEX keeps the SQL identical on both engines.
CREATE INDEX IF NOT EXISTS idx_quota_buckets_sweep
    ON quota_buckets (window_start);
