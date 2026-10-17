-- M3: audit log — SPEC §6 `audit_log` (actor, action, target, before/after
-- JSON, ip, ts) plus an org_id column so org admins see their org's trail
-- without resolving every target at read time (host actions record the
-- target's org; NULL = host-instance scope, host_admin-only visibility).
-- SANITIZATION RULE (director directive 2026-10-17): before/after JSON must
-- NEVER contain key material — masked display forms only (prefix + last-4)
-- for pool keys, provider keys, and sessions; no key_hash, no ciphertext.
CREATE TABLE IF NOT EXISTS audit_log (
    id          TEXT PRIMARY KEY,
    org_id      TEXT,                -- org whose trail this row belongs to (NULL = host-only)
    actor_id    TEXT NOT NULL,       -- acting user id ('system' reserved for internals)
    actor_email TEXT NOT NULL,       -- snapshot: users can be deleted, the trail cannot
    action      TEXT NOT NULL,       -- '<target>.<verb>' e.g. 'member.created'
    target_kind TEXT NOT NULL,       -- 'org' | 'pool' | 'member' | 'pool_key' | 'provider_key' | 'quota_rule' | 'session'
    target_id   TEXT NOT NULL,
    before_json TEXT,                -- masked snapshot BEFORE the change (NULL for creates)
    after_json  TEXT,                -- masked snapshot AFTER the change (NULL for deletes)
    ip          TEXT,                -- client IP as seen by the control plane (XFF first hop)
    ts          BIGINT NOT NULL      -- unix epoch millis
);

CREATE INDEX IF NOT EXISTS idx_audit_log_org_ts ON audit_log (org_id, ts);
CREATE INDEX IF NOT EXISTS idx_audit_log_actor_ts ON audit_log (actor_id, ts);

-- M3: A-004 login throttle — one row per (email|ip) bucket holding the
-- current fixed window's failure count (quota-engine pattern: this row is
-- the authoritative backstop; an in-process counter is the fast path).
CREATE TABLE IF NOT EXISTS login_throttle (
    bucket       TEXT PRIMARY KEY,   -- '<email>|<ip>'
    window_start BIGINT NOT NULL,    -- UTC-aligned fixed-window start (epoch ms)
    fails        BIGINT NOT NULL     -- failed attempts inside that window
);
