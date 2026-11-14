-- What an energy measure stands for (ADR 0013): the whole home ("grid"), a solar production, a
-- battery, or the consumption of a device (a zone, the charger…) or of a room. Read by the energy
-- page (flows, use by use).
ALTER TABLE devices ADD COLUMN role TEXT;
ALTER TABLE devices ADD COLUMN measures_device_id INTEGER REFERENCES devices(id) ON DELETE SET NULL;
ALTER TABLE devices ADD COLUMN measures_room_id INTEGER REFERENCES rooms(id) ON DELETE SET NULL;
