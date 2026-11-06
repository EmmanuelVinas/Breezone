-- Where a routine comes from: NULL = created in FreeZone, "airzone:<installation>:<schedule>[:end]"
-- = imported from an Airzone schedule (a second import does not duplicate it).
ALTER TABLE routines ADD COLUMN origin TEXT;
CREATE UNIQUE INDEX routines_origin ON routines (origin) WHERE origin IS NOT NULL;
