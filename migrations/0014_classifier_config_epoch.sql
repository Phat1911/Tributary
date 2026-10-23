-- A-028 (M5 hardening): config-epoch binding for classifier approvals.
--
-- Problem: a pending org_policy approval freezes a SEALED classifier row
-- at request time. If the config MUTATES between request and approval
-- (another approval executed, a reconfigure landed), executing the stale
-- approval silently overwrites the newer world — the approver approved a
-- configuration that no longer describes the present.
--
-- Fix: classifier_config carries a monotonically bumped `config_epoch`
-- (every mutation +1). Pending approvals record the epoch they OBSERVED;
-- execution refuses (409 + auto-reject) on mismatch. "Stale approval ⇒
-- 409, arms nothing" — the approval requester re-requests against the
-- world as it is.
--
-- classifier_min_shadow_secs: OPTIONAL org setting (default 0 = OFF) for
-- the minimum shadow age before go_live may execute. Stored on
-- org_settings next to the D32 window; BIGINT seconds, 0 disables.
ALTER TABLE classifier_config ADD COLUMN config_epoch BIGINT NOT NULL DEFAULT 0;

ALTER TABLE org_settings ADD COLUMN classifier_min_shadow_secs BIGINT NOT NULL DEFAULT 0;

-- The watermark makes epochs monotonic PER ORG even across delete+
-- recreate cycles: without it, "configure@epoch1 → remove → configure
-- (epoch restarts at 1)" would let a stale approval's expected epoch
-- collide with a fresh row's epoch, silently arming the old config.
-- Every classifier mutation (configure/go_live/remove) bumps the
-- watermark and stamps it into the row; deleted-then-recreated configs
-- therefore always carry epochs no stale approval can match.
ALTER TABLE org_settings ADD COLUMN classifier_epoch_watermark BIGINT NOT NULL DEFAULT 0;
