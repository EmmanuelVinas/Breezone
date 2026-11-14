-- What a source must keep secret (a HomeKit pairing: Breezone's long-term key), encrypted with the
-- server key; never sent by the API.
ALTER TABLE sources ADD COLUMN secret TEXT;
