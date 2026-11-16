-- The overview of an installation, as its blocks: the one its administrators set for everyone
-- (user_id NULL), and those users made for themselves.
CREATE TABLE dashboards (
    id              INTEGER PRIMARY KEY,
    installation_id INTEGER NOT NULL REFERENCES installations(id) ON DELETE CASCADE,
    user_id         INTEGER REFERENCES users(id) ON DELETE CASCADE,
    layout          TEXT NOT NULL,
    updated_at      INTEGER NOT NULL
);

CREATE UNIQUE INDEX dashboards_one_per_scope ON dashboards(installation_id, IFNULL(user_id, 0));
