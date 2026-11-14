-- History of the devices (ADR 0013): every number of their state, every few minutes (the mean of
-- the readings since the previous point; the last value for a counter such as `energy_in`). Read
-- by the device curves and the energy page.
CREATE TABLE device_samples (
    device_id INTEGER NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    ts INTEGER NOT NULL,
    field TEXT NOT NULL,
    value REAL NOT NULL,
    PRIMARY KEY (device_id, field, ts)
) WITHOUT ROWID;
CREATE INDEX idx_device_samples_ts ON device_samples(ts);
