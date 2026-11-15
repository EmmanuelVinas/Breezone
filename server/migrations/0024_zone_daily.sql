-- Long history of the Airzone zones: per local day, the mean room temperature, humidity and
-- setpoint (while on), the share of the day the zone was on, the time in demand and the mean air
-- quality. Kept for good; `readings` keeps the detail (every few minutes) for a few weeks only.
CREATE TABLE zone_daily (
    installation_id INTEGER NOT NULL REFERENCES installations(id) ON DELETE CASCADE,
    system_id INTEGER NOT NULL,
    zone_id INTEGER NOT NULL,
    -- Local midnight of the day.
    ts INTEGER NOT NULL,
    room_temp REAL,
    humidity REAL,
    setpoint REAL,
    on_ratio REAL,
    demand_secs INTEGER,
    air_quality REAL,
    PRIMARY KEY (installation_id, system_id, zone_id, ts)
) WITHOUT ROWID;
