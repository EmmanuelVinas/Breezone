-- Automations (roadmap E15): rules "quand… si… alors…" run by this server. `trigger_def`, `conditions`
-- and `actions` are JSON (see `automations.rs`); `last_reason` is what was true at the last run, in
-- every language.
CREATE TABLE automations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    trigger_def TEXT NOT NULL,
    conditions TEXT NOT NULL DEFAULT '[]',
    actions TEXT NOT NULL,
    last_run_at INTEGER,
    last_reason TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
