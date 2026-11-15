-- The weather follows each installation: its city is turned into coordinates once, when it is set
-- (by hand, from the Airzone account or from another node), and kept on the installation with its
-- area (the French "département", for the weather warnings) and country. `geo_key` is the city and
-- country they were found for: a new city is looked up again.
ALTER TABLE installations ADD COLUMN latitude REAL;
ALTER TABLE installations ADD COLUMN longitude REAL;
ALTER TABLE installations ADD COLUMN area TEXT;
ALTER TABLE installations ADD COLUMN geo_country TEXT;
ALTER TABLE installations ADD COLUMN geo_key TEXT;

-- The city set for "the home" in the settings goes to the installation it stood for.
UPDATE installations SET location = (SELECT value FROM settings WHERE key = 'weather.place'), location_set = 1
WHERE id = (SELECT id FROM installations WHERE node_id IS NULL ORDER BY id LIMIT 1)
  AND EXISTS (SELECT 1 FROM settings WHERE key = 'weather.place' AND trim(value) <> '');
DELETE FROM settings WHERE key IN ('weather.place', 'weather.coords');

-- The outdoor temperature of each installation. What was kept was that of the first installation
-- with a city: it stays with it.
CREATE TABLE outdoor_samples_by_installation (
    installation_id INTEGER NOT NULL REFERENCES installations(id) ON DELETE CASCADE,
    ts INTEGER NOT NULL,
    temperature REAL NOT NULL,
    PRIMARY KEY (installation_id, ts)
) WITHOUT ROWID;
INSERT INTO outdoor_samples_by_installation (installation_id, ts, temperature)
SELECT (SELECT id FROM installations WHERE node_id IS NULL AND location IS NOT NULL AND location <> '' ORDER BY id LIMIT 1), ts, temperature
FROM outdoor_samples
WHERE EXISTS (SELECT 1 FROM installations WHERE node_id IS NULL AND location IS NOT NULL AND location <> '');
DROP TABLE outdoor_samples;
ALTER TABLE outdoor_samples_by_installation RENAME TO outdoor_samples;
