-- Airzone Cloud synchronisation: installation location/MAC, settings, cloud installations.

ALTER TABLE installations ADD COLUMN location TEXT;
ALTER TABLE installations ADD COLUMN mac TEXT;

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Installations of the Airzone Cloud account. `installation_id` is the local FreeZone
-- installation it was matched to (NULL = WebServer not reachable from this server).
CREATE TABLE cloud_installations (
    cloud_id        TEXT    PRIMARY KEY,
    name            TEXT    NOT NULL,
    location        TEXT,
    ws_ids          TEXT    NOT NULL,
    zones           INTEGER NOT NULL DEFAULT 0,
    installation_id INTEGER REFERENCES installations(id) ON DELETE SET NULL,
    synced_at       INTEGER NOT NULL
);
