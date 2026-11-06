-- FreeZone initial schema. Timestamps are unix seconds (UTC).

CREATE TABLE users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    username      TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT    NOT NULL,
    role          TEXT    NOT NULL CHECK (role IN ('admin', 'user')),
    created_at    INTEGER NOT NULL
);

-- One row per logged-in device. Holds the (hashed) long-lived refresh token.
CREATE TABLE sessions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id         INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash      TEXT    NOT NULL UNIQUE,
    prev_token_hash TEXT,
    rotated_at      INTEGER,
    device_name     TEXT    NOT NULL,
    created_at      INTEGER NOT NULL,
    last_used_at    INTEGER NOT NULL,
    expires_at      INTEGER NOT NULL
);
CREATE INDEX idx_sessions_prev ON sessions(prev_token_hash);

-- An Airzone WebServer reachable on the LAN (local API, port 3000 by default).
CREATE TABLE installations (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT    NOT NULL,
    host       TEXT    NOT NULL,
    port       INTEGER NOT NULL DEFAULT 3000,
    enabled    INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL
);

-- FreeZone-side metadata for a zone (grouping by floor, custom name, ordering).
CREATE TABLE zone_meta (
    installation_id INTEGER NOT NULL REFERENCES installations(id) ON DELETE CASCADE,
    system_id       INTEGER NOT NULL,
    zone_id         INTEGER NOT NULL,
    display_name    TEXT,
    group_name      TEXT,
    position        INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (installation_id, system_id, zone_id)
);

-- Temperature / humidity / setpoint history.
CREATE TABLE readings (
    installation_id INTEGER NOT NULL REFERENCES installations(id) ON DELETE CASCADE,
    system_id       INTEGER NOT NULL,
    zone_id         INTEGER NOT NULL,
    ts              INTEGER NOT NULL,
    room_temp       REAL,
    humidity        INTEGER,
    setpoint        REAL,
    is_on           INTEGER,
    mode            INTEGER
);
CREATE INDEX idx_readings_zone_ts ON readings(installation_id, system_id, zone_id, ts);
CREATE INDEX idx_readings_ts ON readings(ts);

-- Routines: a list of actions, run manually (scene) and/or on a weekly schedule.
CREATE TABLE routines (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT    NOT NULL,
    enabled     INTEGER NOT NULL DEFAULT 1,
    days        INTEGER NOT NULL DEFAULT 0,   -- bitmask: Monday = 1, Tuesday = 2, ... Sunday = 64
    time        TEXT,                         -- "HH:MM" local time; NULL = manual only
    actions     TEXT    NOT NULL,             -- JSON array of actions
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    last_run_at INTEGER
);

-- Activity feed shown in the web UI ("flux").
CREATE TABLE events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    ts              INTEGER NOT NULL,
    kind            TEXT    NOT NULL,  -- connection | command | routine | system | auth
    level           TEXT    NOT NULL,  -- info | warn | error
    installation_id INTEGER,
    actor           TEXT,
    message         TEXT    NOT NULL,
    data            TEXT
);
CREATE INDEX idx_events_ts ON events(ts);
