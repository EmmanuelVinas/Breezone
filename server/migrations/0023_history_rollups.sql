-- Long history of the devices: the mean of every number per hour (kept about a year) and per local
-- day (kept for good), the last value for a counter such as `energy_in`. Computed from
-- device_samples, which keeps the detail (every few minutes) for a few weeks only.
CREATE TABLE device_hourly (
    device_id INTEGER NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    field TEXT NOT NULL,
    ts INTEGER NOT NULL,
    value REAL NOT NULL,
    PRIMARY KEY (device_id, field, ts)
) WITHOUT ROWID;
CREATE INDEX idx_device_hourly_ts ON device_hourly(ts);

CREATE TABLE device_daily (
    device_id INTEGER NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    field TEXT NOT NULL,
    -- Local midnight of the day.
    ts INTEGER NOT NULL,
    value REAL NOT NULL,
    PRIMARY KEY (device_id, field, ts)
) WITHOUT ROWID;

-- The energy of the home per local day (JSON of DayEnergy), kept once the detail it comes from is
-- gone: the energy page goes back years.
CREATE TABLE energy_days (
    day TEXT PRIMARY KEY,
    data TEXT NOT NULL
) WITHOUT ROWID;
