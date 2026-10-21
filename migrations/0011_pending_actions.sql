-- D33 (2026-10-21): admin four-eyes. A sensitive admin request does not
-- execute directly (unless the org has a single admin, or the action is
-- containment with a linked alert) — it lands here as a PENDING action
-- and only a SECOND, DISTINCT admin's approval executes it.
-- BIGINT epoch-ms timestamps on both engines (the dual-DB INTEGER rule);
-- payload_json is SANITIZED action parameters (sealed blobs for secrets,
-- never plaintext keys — rule 2).
CREATE TABLE IF NOT EXISTS pending_actions (
    id            TEXT PRIMARY KEY,
    org_id        TEXT   NOT NULL,
    kind          TEXT   NOT NULL,
    target_id     TEXT   NOT NULL,
    payload_json  TEXT   NOT NULL,
    requested_by  TEXT   NOT NULL,
    status        TEXT   NOT NULL DEFAULT 'pending',
    decided_by    TEXT,
    decided_at    BIGINT,
    created_at    BIGINT NOT NULL
);
