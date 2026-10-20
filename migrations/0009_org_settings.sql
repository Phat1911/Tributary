-- D32 (2026-10-20): org-tunable detector knobs. First knob: the distinct-IP
-- concurrency window (seconds, default 300 = 5 min). One row per org once
-- tuned; absent row = compiled default. BIGINT on both engines (the
-- dual-DB INTEGER rule: never bind a Rust bool or float here).
CREATE TABLE IF NOT EXISTS org_settings (
    org_id                   TEXT   PRIMARY KEY,
    concurrency_window_secs  BIGINT NOT NULL DEFAULT 300,
    updated_at               BIGINT NOT NULL
);
