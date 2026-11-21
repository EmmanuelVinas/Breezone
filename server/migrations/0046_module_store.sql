-- The store of each source run by a module (docs/roadmap/modules.md § 4.5): small values the module
-- keeps across restarts (a session, a counter). They go with the source.
CREATE TABLE module_store (
    source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    key       TEXT    NOT NULL,
    value     BLOB    NOT NULL,
    PRIMARY KEY (source_id, key)
);
