-- Modules (docs/roadmap/modules.md § 4.8): those this server has, how they came, the version that
-- runs and the one kept to go back to. The modules compiled into the server are written at start
-- (origin 'native'); the others come from the library, the archive of Breezone or a file.
CREATE TABLE modules (
    id               TEXT    PRIMARY KEY,
    version          TEXT    NOT NULL,
    previous_version TEXT,
    -- 'native' | 'builtin' | 'library' | 'file'
    origin           TEXT    NOT NULL,
    enabled          INTEGER NOT NULL DEFAULT 1,
    -- 'installed' | 'pending' | 'incompatible' | 'error'
    state            TEXT    NOT NULL DEFAULT 'installed',
    last_error       TEXT,
    auto_update      INTEGER NOT NULL DEFAULT 1,
    -- Versions given up after a failure (JSON list): not proposed again.
    rejected         TEXT    NOT NULL DEFAULT '[]',
    installed_at     INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);
