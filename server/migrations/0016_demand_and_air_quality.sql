-- Time each zone spent asking for heat or cold, recorded with the history when the installation
-- asks for it (set on the server that owns the installation; shared copies follow it).
ALTER TABLE installations ADD COLUMN record_demand INTEGER NOT NULL DEFAULT 0;

-- Seconds in demand since the previous history point of the zone (NULL: not recorded).
ALTER TABLE readings ADD COLUMN demand_secs INTEGER;

-- Air quality measured by the zone's thermostat, as read from the WebServer (`aq_quality`;
-- NULL: the zone has no air quality sensor).
ALTER TABLE readings ADD COLUMN air_quality INTEGER;
