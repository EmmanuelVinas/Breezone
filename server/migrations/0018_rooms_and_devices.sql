-- The home model (ADR 0013): rooms chosen by the user, independent of the Airzone groups (floors,
-- systems), and devices described by traits.
CREATE TABLE rooms (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT    NOT NULL,
    position   INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

-- A device has a stable id for a given source and reference (for Airzone:
-- "<installation>/<system>/<zone>"): scenes and automations will point to it. Airzone zones are
-- added automatically; `name` overrides the name given by the source.
CREATE TABLE devices (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    source          TEXT    NOT NULL,
    source_ref      TEXT    NOT NULL,
    installation_id INTEGER REFERENCES installations(id) ON DELETE CASCADE,
    kind            TEXT    NOT NULL,
    name            TEXT,
    room_id         INTEGER REFERENCES rooms(id) ON DELETE SET NULL,
    position        INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL,
    UNIQUE (source, source_ref)
);
CREATE INDEX devices_room ON devices(room_id);
