-- Sources of modules (docs/roadmap/modules.md § 4.9): the official library, always there, and those
-- an administrator adds (a developer's own, a community's), each with the Ed25519 key that signs
-- its index and its packages. `url` is NULL for the official one (its address and key are
-- compiled into the server). `seq` is the number of the last index accepted from it.
CREATE TABLE module_repositories (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT    NOT NULL,
    url        TEXT,
    public_key TEXT,
    official   INTEGER NOT NULL DEFAULT 0,
    enabled    INTEGER NOT NULL DEFAULT 1,
    seq        INTEGER,
    created_at INTEGER NOT NULL
);

INSERT INTO module_repositories (name, url, public_key, official, created_at)
VALUES ('Breezone', NULL, NULL, 1, CAST(strftime('%s', 'now') AS INTEGER));

-- The source a module was installed from: its updates come from there only.
ALTER TABLE modules ADD COLUMN repository INTEGER REFERENCES module_repositories(id);
