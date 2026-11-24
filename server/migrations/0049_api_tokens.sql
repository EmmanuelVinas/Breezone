-- Personal access tokens: a user signs a tool in with one (an assistant over MCP, a script)
-- instead of a password. Only the hash is kept; `hint` is the start of the token, to tell them
-- apart. They go with the user, and a password change or reset removes them like the sessions.
CREATE TABLE api_tokens (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT    NOT NULL,
    token_hash   TEXT    NOT NULL UNIQUE,
    hint         TEXT    NOT NULL,
    created_at   INTEGER NOT NULL,
    last_used_at INTEGER
);
CREATE INDEX idx_api_tokens_user ON api_tokens(user_id);
