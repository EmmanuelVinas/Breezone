-- One priority for all the automations (1 first): the solar surplus is shared in this order, and
-- when two automations drive the same device at the same time, the higher one wins. Ranked as they
-- were made.
ALTER TABLE automations ADD COLUMN priority INTEGER NOT NULL DEFAULT 0;
UPDATE automations SET priority = (SELECT COUNT(*) FROM automations a WHERE a.id <= automations.id);
-- The automation that put a device on the solar surplus (its priority orders the loads; deleted,
-- the device no longer follows it); none: the list of before, made into automations at the next
-- start.
ALTER TABLE surplus_loads ADD COLUMN automation_id INTEGER REFERENCES automations(id) ON DELETE CASCADE;
