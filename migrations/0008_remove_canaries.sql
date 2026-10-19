-- D31 (2026-10-19): honey keys + canary model aliases REMOVED post-M4
-- founder testing — low-yield tripwires; raw-key export removed with them.
-- Dual-DB note: SQLite (>=3.35) and PostgreSQL both support DROP TABLE and
-- this DELETE; no column drops (pool_keys.kind stays, always 'live' now —
-- removing a column would need table rebuilds on both engines for zero
-- value).
DROP TABLE IF EXISTS canary_models;
DELETE FROM pool_key_ips WHERE pool_key_id IN (SELECT id FROM pool_keys WHERE kind = 'canary');
DELETE FROM ip_allowlists WHERE pool_key_id IN (SELECT id FROM pool_keys WHERE kind = 'canary');
DELETE FROM review_flags WHERE pool_key_id IN (SELECT id FROM pool_keys WHERE kind = 'canary');
DELETE FROM pool_keys WHERE kind = 'canary';
