-- Automations: the actions run when the conditions are false ("sinon"), what to do when the rule
-- fires again while it waits ("single": ignore, "restart": start again), and the last runs of each
-- rule, those that did not happen included (why, in every language).
ALTER TABLE automations ADD COLUMN else_actions TEXT NOT NULL DEFAULT '[]';
ALTER TABLE automations ADD COLUMN run_mode TEXT NOT NULL DEFAULT 'single';
CREATE TABLE automation_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    automation_id INTEGER NOT NULL REFERENCES automations(id) ON DELETE CASCADE,
    started_at INTEGER NOT NULL,
    -- NULL while it runs.
    finished_at INTEGER,
    -- "then" | "else" | "skipped" (the conditions were false) | "busy" (it was already running)
    branch TEXT NOT NULL,
    reason TEXT NOT NULL,
    -- "done" | "stopped" (a wait ran out) | "cancelled" (started again) | "interrupted" (server stopped)
    ended TEXT,
    steps TEXT NOT NULL DEFAULT '[]'
);
CREATE INDEX idx_automation_runs ON automation_runs(automation_id, started_at);
