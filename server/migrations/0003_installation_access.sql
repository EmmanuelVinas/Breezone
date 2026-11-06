-- Access point of an installation. A FreeZone server scans only its own network; an
-- installation is reached either through that network (WebServer found by the scan, or its
-- fixed local address set here) or, later, through another FreeZone node (see ADR 0008).

-- Snapshot of the cloud organisation (groups, zone names), to attach an installation later
-- without logging in to Airzone again.
ALTER TABLE cloud_installations ADD COLUMN detail TEXT;

-- Link between a FreeZone installation and its Airzone Cloud installation.
ALTER TABLE installations ADD COLUMN cloud_id TEXT;

-- Fixed local address of the WebServer of a cloud installation (kept across synchronisations).
CREATE TABLE installation_access (
    cloud_id   TEXT    PRIMARY KEY,
    host       TEXT    NOT NULL,
    port       INTEGER NOT NULL DEFAULT 3000,
    updated_at INTEGER NOT NULL
);
