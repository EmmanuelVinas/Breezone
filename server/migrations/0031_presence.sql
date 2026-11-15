-- Who is at home (roadmap F9): each user tells it from the app or the web; the automations read
-- whether someone is home ("the first one arrives", "the last one leaves").
CREATE TABLE presence (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    home INTEGER NOT NULL,
    changed_at INTEGER NOT NULL
);
