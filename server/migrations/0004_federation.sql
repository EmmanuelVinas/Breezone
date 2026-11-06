-- Federation between FreeZone nodes (ADR 0008).
--
-- A node *exposes* some of its local installations through shares (one secret key each,
-- stored hashed) and *consumes* the installations other nodes expose to it (nodes table).

-- Shares exposed by this node.
CREATE TABLE shares (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    label          TEXT    NOT NULL,
    key_hash       TEXT    NOT NULL UNIQUE,
    permission     TEXT    NOT NULL CHECK (permission IN ('read', 'control')),
    expires_at     INTEGER,
    created_at     INTEGER NOT NULL,
    created_by     TEXT,
    -- Identity of the node using the share (known after its first call).
    remote_node_id TEXT,
    remote_name    TEXT,
    last_seen      INTEGER,
    last_address   TEXT
);

CREATE TABLE share_installations (
    share_id        INTEGER NOT NULL REFERENCES shares(id) ON DELETE CASCADE,
    installation_id INTEGER NOT NULL REFERENCES installations(id) ON DELETE CASCADE,
    PRIMARY KEY (share_id, installation_id)
);

-- Nodes this node consumes. The share key is encrypted like the Airzone credentials.
CREATE TABLE nodes (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    remote_node_id TEXT    NOT NULL UNIQUE,
    name           TEXT    NOT NULL,
    url            TEXT    NOT NULL,
    key_enc        TEXT    NOT NULL,
    permission     TEXT    NOT NULL DEFAULT 'read',
    created_at     INTEGER NOT NULL,
    last_seen      INTEGER,
    last_error     TEXT
);

-- Installation reached through another node (NULL = local WebServer), and its id there.
ALTER TABLE installations ADD COLUMN node_id INTEGER REFERENCES nodes(id) ON DELETE CASCADE;
ALTER TABLE installations ADD COLUMN remote_id INTEGER;
