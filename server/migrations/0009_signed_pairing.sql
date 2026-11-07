-- Secure pairing between nodes: single-use invitation codes, approval by an administrator of the
-- exposing node, and requests signed with the consuming node's Ed25519 key (no bearer secret).
--
-- status: 'invited' (code not used yet) → 'pending' (a node used it, awaiting approval) → 'active'.
-- key_hash keeps the hash of the invitation code while 'invited'.
ALTER TABLE shares ADD COLUMN status TEXT NOT NULL DEFAULT 'active';
ALTER TABLE shares ADD COLUMN pairing_expires_at INTEGER;
ALTER TABLE shares ADD COLUMN remote_public_key TEXT;
ALTER TABLE shares ADD COLUMN requested_at INTEGER;

-- Shares and nodes of the previous scheme (bearer keys) cannot be converted: pair again.
DELETE FROM installations WHERE node_id IS NOT NULL;
DELETE FROM nodes;
DELETE FROM shares;
