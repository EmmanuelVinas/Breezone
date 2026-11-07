-- Reciprocal sharing: the node that invites can ask the invited node to share back.
--
-- reciprocal:          this share's invitation asks the other node to share its installations too.
-- remote_url:          address of the other node, sent with its access request (to reach it back).
-- remote_shares_back:  the other node accepted and shares installations with this node.
ALTER TABLE shares ADD COLUMN reciprocal INTEGER NOT NULL DEFAULT 0;
ALTER TABLE shares ADD COLUMN remote_url TEXT;
ALTER TABLE shares ADD COLUMN remote_shares_back INTEGER NOT NULL DEFAULT 0;

-- Certificate of a node trusted by an administrator although it fails the usual HTTPS checks
-- (SHA-256 of the certificate, lowercase hex): only this exact certificate is then accepted.
ALTER TABLE nodes ADD COLUMN tls_fingerprint TEXT;
