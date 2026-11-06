-- Schedules of the Airzone app, mirrored from Airzone Cloud (Airzone stays the master and runs
-- them; FreeZone shows and edits them). `data` = the schedule as returned by Airzone,
-- `zones` = its zones resolved to [{system_id, zone_id}].
CREATE TABLE airzone_schedules (
    cloud_id  TEXT    NOT NULL,
    sched_id  TEXT    NOT NULL,
    data      TEXT    NOT NULL,
    zones     TEXT    NOT NULL,
    synced_at INTEGER NOT NULL,
    PRIMARY KEY (cloud_id, sched_id)
);

-- Routines imported once from Airzone (previous version): Airzone runs these schedules itself
-- now, keeping them would run them twice.
DELETE FROM routines WHERE origin LIKE 'airzone:%';
