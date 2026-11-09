-- What a user (role 'user') may change on the installations they control, on top of driving
-- them. Allowed by default; administrators can always do everything.
ALTER TABLE users ADD COLUMN edit_zones INTEGER NOT NULL DEFAULT 1;     -- zone names and groups
ALTER TABLE users ADD COLUMN edit_routines INTEGER NOT NULL DEFAULT 1;  -- create, change, delete routines
ALTER TABLE users ADD COLUMN edit_schedules INTEGER NOT NULL DEFAULT 1; -- create, change, delete Airzone schedules
