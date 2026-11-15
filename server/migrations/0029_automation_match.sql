-- How the conditions of an automation are joined: "all" (and) or "any" (or). Groups inside
-- `conditions` carry their own.
ALTER TABLE automations ADD COLUMN condition_match TEXT NOT NULL DEFAULT 'all';
