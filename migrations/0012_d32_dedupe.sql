-- A-021 (M5 hardening, director ruling 2026-10-22): D32 dedupe becomes a
-- DATABASE invariant. The old count-then-insert dedupe let a concurrent
-- burst from a newly shared key raise N flags + N emails for one sharing
-- event; this partial unique index makes "at most one OPEN d32 flag per
-- key" enforceable at insert time, so the racing loser's INSERT is a
-- no-op on both engines.
--
-- Scope is deliberately ONLY the D32 kind: classifier flags are per-request
-- evidence and may legitimately stack while open; D32 is per-INCIDENT (one
-- sharing event = one queue card until resolved or superseded).
--
-- Partial unique indexes and the matching ON CONFLICT ... DO NOTHING
-- conflict target are supported identically by SQLite (>= 3.24) and
-- PostgreSQL — dual-engine portable, no adapter branching (AGENTS DB rule).
-- A stale OPEN flag (older than the org's window) is superseded by the
-- application layer (resolved + replaced), never left to swallow new
-- incidents — see security::detect.
CREATE UNIQUE INDEX IF NOT EXISTS uq_review_flags_open_d32
    ON review_flags (pool_key_id)
    WHERE kind = 'distinct_ip_concurrency' AND status = 'open';
