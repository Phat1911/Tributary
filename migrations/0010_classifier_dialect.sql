-- D28 + director research 2026-10-21: classifier dialects. The org picks
-- which wire contract its endpoint speaks: 'jev' = TypeSafe AI's real
-- hosted systemone contract; 'custom' = the generic 0..1 scorer shape this
-- crate originally shipped. Existing rows predate the column and mean the
-- generic shape — hence the default.
ALTER TABLE classifier_config ADD COLUMN dialect TEXT NOT NULL DEFAULT 'custom';
