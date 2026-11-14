-- Capabilities of the other node (ADR 0012), as exchanged by the hello: JSON document and when
-- it was received. On `nodes` (nodes this server reads) and `shares` (nodes reading this one).
ALTER TABLE nodes ADD COLUMN peer_caps TEXT;
ALTER TABLE nodes ADD COLUMN peer_caps_at INTEGER;
ALTER TABLE shares ADD COLUMN peer_caps TEXT;
ALTER TABLE shares ADD COLUMN peer_caps_at INTEGER;
