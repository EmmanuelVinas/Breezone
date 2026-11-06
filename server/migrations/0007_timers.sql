-- Timers ("minuteries"): an action planned on a zone, a group, an installation or everything,
-- and the delayed actions of routines. Deleted once run.
CREATE TABLE timers (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    label      TEXT    NOT NULL,
    run_at     INTEGER NOT NULL,
    target     TEXT    NOT NULL,
    command    TEXT    NOT NULL,
    created_by TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX timers_run_at ON timers (run_at);

-- Routines of the Airzone app, as read from Airzone Cloud (displayed; import to come).
CREATE TABLE airzone_routines (
    cloud_id  TEXT    PRIMARY KEY,
    data      TEXT    NOT NULL,
    synced_at INTEGER NOT NULL
);
