-- The electricity contract and its prices belong to an installation (a place: the home, an
-- office…), no longer to the server. What was set goes to the first installation of this server.
CREATE TABLE electricity_contracts (
    installation_id INTEGER PRIMARY KEY REFERENCES installations(id) ON DELETE CASCADE,
    country    TEXT,
    supplier   TEXT,
    offer      TEXT NOT NULL DEFAULT 'custom',
    power_kva  REAL,
    currency   TEXT NOT NULL DEFAULT 'EUR',
    offpeak    TEXT,
    updated_at INTEGER NOT NULL
);

INSERT INTO electricity_contracts (installation_id, country, supplier, offer, power_kva, currency, offpeak, updated_at)
SELECT (SELECT MIN(id) FROM installations WHERE node_id IS NULL), country, supplier, offer, power_kva, currency, offpeak, updated_at
FROM electricity_contract
WHERE (SELECT MIN(id) FROM installations WHERE node_id IS NULL) IS NOT NULL;

DROP TABLE electricity_contract;

CREATE TABLE electricity_prices (
    installation_id INTEGER NOT NULL REFERENCES installations(id) ON DELETE CASCADE,
    valid_from TEXT NOT NULL,
    tariff     TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (installation_id, valid_from)
);

INSERT INTO electricity_prices (installation_id, valid_from, tariff, updated_at)
SELECT (SELECT MIN(id) FROM installations WHERE node_id IS NULL), valid_from, tariff, updated_at
FROM electricity_tariffs
WHERE (SELECT MIN(id) FROM installations WHERE node_id IS NULL) IS NOT NULL;

DROP TABLE electricity_tariffs;
