-- Sources of devices (ADR 0013): one configured piece of hardware (a Shelly at 192.168.1.40…),
-- read by a driver. Their devices are linked to them and go with them.
CREATE TABLE sources (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    driver     TEXT    NOT NULL,
    name       TEXT    NOT NULL,
    config     TEXT    NOT NULL DEFAULT '{}',
    enabled    INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL
);

ALTER TABLE devices ADD COLUMN source_id INTEGER REFERENCES sources(id) ON DELETE CASCADE;
