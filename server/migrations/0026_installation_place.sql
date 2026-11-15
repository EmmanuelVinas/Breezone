-- The country of an installation (ISO 3166-1 alpha-2, "FR"), and whether its city was set by hand:
-- the Airzone account no longer overwrites it then. The weather of the home uses both.
ALTER TABLE installations ADD COLUMN country TEXT;
ALTER TABLE installations ADD COLUMN location_set INTEGER NOT NULL DEFAULT 0;
