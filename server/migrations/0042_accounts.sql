-- Linked accounts: a connection to an online service (credentials or OAuth tokens, encrypted),
-- used by the modules and features that need it. One per kind of service.
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL UNIQUE,
    -- What is not secret (an e-mail, a client id, a meter number), JSON.
    config TEXT NOT NULL DEFAULT '{}',
    -- The secrets (password, client secret, tokens), JSON encrypted with the data key.
    secret TEXT,
    -- "ok" | "error" | "expired" | "unknown"
    state TEXT NOT NULL DEFAULT 'unknown',
    last_error TEXT,
    last_ok INTEGER,
    -- When the access given by the service ends (OAuth), if it says.
    expires_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- The Airzone Cloud account, until now in the settings: its password moves as it is (already
-- encrypted with the same key), so nothing is asked again.
INSERT INTO accounts (kind, config, secret, state, created_at, updated_at)
SELECT 'airzone', json_object('email', e.value),
       (SELECT value FROM settings WHERE key = 'cloud.password'),
       'unknown', CAST(strftime('%s', 'now') AS INTEGER), CAST(strftime('%s', 'now') AS INTEGER)
FROM settings e WHERE e.key = 'cloud.email';
DELETE FROM settings WHERE key IN ('cloud.email', 'cloud.password');

-- What Enedis measured (Linky), read through an account: energy per local day (Wh)…
CREATE TABLE linky_days (
    prm TEXT NOT NULL,
    day TEXT NOT NULL,
    consumption REAL,
    production REAL,
    PRIMARY KEY (prm, day)
);

-- …and the mean power of each half hour (W), from its start.
CREATE TABLE linky_curve (
    prm TEXT NOT NULL,
    ts INTEGER NOT NULL,
    power REAL NOT NULL,
    PRIMARY KEY (prm, ts)
);
