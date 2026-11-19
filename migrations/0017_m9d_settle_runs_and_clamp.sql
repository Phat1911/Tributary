-- M9-D FIX SESSION (2026-11-19, doctrine-suite step 14): three additive
-- pieces closing Session C findings A-047/A-048/A-049.
--
-- (1) `settle_runs` — the settle-run journal (A-048): one row per executed
--     settlement, keyed uniquely by (pool, window). A retried POST /settle
--     over the SAME window returns the FIRST run's stored result (client-
--     retry safe, dual-DB); the ledger postings of a run carry the run id
--     in `ledger_entries.ref` (A-049's coupling ruling: one run = one
--     batch of postings, retry = no-op on BOTH sides).
-- (2) `depositor_bucket_positions.settled_burn_micros` — the settled-burn
--     marker (A-047/A-048): cumulative burn already attributed into a
--     settlement. A later settle attributes exactly `burned − settled`
--     (never re-batching), and the pre-fix tree's already-metered burns
--     are marked settled HERE (forward-only: legacy positions never
--     re-attribute their history).
-- (3) The chart pairing for the NEW posting template `capacity.consume`
--     (A-049, founder-approved 2026-11-19): Dr claims.contributed /
--     Cr revenue.usage — burned principal converts into the settlement
--     pot that settle.fee + settle.distribute then split (D39). Only the
--     two counterparty lists grow; the six M7 templates and their
--     pairings are untouched, and the SEED↔DB mirror test pins the new
--     lists equal by construction.
--
-- Additive ONLY (doctrine invariant 10): everything new is a new table or a
-- nullable/defaulted column; no existing column, table, or constraint is
-- rewritten. Portable SQL: TEXT/BIGINT, ids minted in Rust, no DB-specific
-- types (house style since 0001).

-- (1) The settle-run journal. response_json is the EXACT response
-- payload of the run (distributable/fee/undistributed/payouts), replayed
-- verbatim on an identical-window retry; status is the run's lifecycle
-- ('pending' while posting, 'ok' once complete — only 'ok' runs replay —
-- or 'failed', a loud tombstone that blocks the exact window and names
-- the audit trail). Money is integer micro-USD only (AGENTS.md).
CREATE TABLE IF NOT EXISTS settle_runs (
    id                    TEXT PRIMARY KEY,
    pool_id               TEXT NOT NULL,
    from_ms               BIGINT NOT NULL,
    to_ms                 BIGINT NOT NULL,
    fee_bps               BIGINT NOT NULL,
    distributable_micros  BIGINT NOT NULL,
    fee_micros            BIGINT NOT NULL,
    undistributed_micros  BIGINT NOT NULL,
    response_json         TEXT NOT NULL,
    status                TEXT NOT NULL,
    created_at            BIGINT NOT NULL,
    UNIQUE (pool_id, from_ms, to_ms)
);

-- (2) The settled-burn marker on the per-bucket positions.
ALTER TABLE depositor_bucket_positions ADD COLUMN settled_burn_micros BIGINT NOT NULL DEFAULT 0;

-- Forward-only legacy seed: every burn the pre-fix tree already metered is
-- marked settled NOW, so the first post-fix settle attributes only burns
-- that land after this migration. (Pre-fix settles credited positions
-- directly without posting the books; that legacy divergence is recorded
-- in AUDIT.md A-049 and stays a documented operator cleanup item — this
-- migration deliberately does NOT try to re-settle history.)
UPDATE depositor_bucket_positions SET settled_burn_micros = burned_micros;

-- (3) Chart counterparties for capacity.consume (claims ↔ revenue).
UPDATE chart_of_accounts
   SET allowed_counterparties = '["capacity.contributed","revenue.usage"]'
 WHERE code = 'claims.contributed';
UPDATE chart_of_accounts
   SET allowed_counterparties = '["prepaid.renter","equity.operator","payable.depositor","claims.contributed"]'
 WHERE code = 'revenue.usage';
