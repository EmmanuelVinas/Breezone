-- A linked account or service can be switched off without forgetting it.
ALTER TABLE accounts ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1;

-- The weather (Open-Meteo, MeteoAlarm) becomes a linked service: no credentials, only a switch.
INSERT INTO accounts (kind, state, created_at, updated_at)
VALUES ('weather', 'unknown', CAST(strftime('%s', 'now') AS INTEGER), CAST(strftime('%s', 'now') AS INTEGER));
