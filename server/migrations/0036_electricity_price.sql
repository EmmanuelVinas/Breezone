-- The price of electricity (roadmap E32). The contract of the home: one per server, as the meter its
-- modules read. Its tariffs, each from a local day on (`valid_from`, "YYYY-MM-DD"), as JSON
-- (subscription, profiles of day and their slots, calendar): a change of prices adds one, the past
-- keeps its own.
CREATE TABLE electricity_contract (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    country    TEXT,
    supplier   TEXT,
    offer      TEXT NOT NULL DEFAULT 'custom',
    power_kva  REAL,
    currency   TEXT NOT NULL DEFAULT 'EUR',
    updated_at INTEGER NOT NULL
);

CREATE TABLE electricity_tariffs (
    valid_from TEXT PRIMARY KEY,
    tariff     TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
