-- Every page is a dashboard of blocks (the overview, the energy, the air conditioning…): each layout
-- is that of a page of an installation, for everyone (user_id NULL) or a user's own.
ALTER TABLE dashboards ADD COLUMN page TEXT NOT NULL DEFAULT 'overview';
DROP INDEX dashboards_one_per_scope;
CREATE UNIQUE INDEX dashboards_one_per_scope ON dashboards(installation_id, page, IFNULL(user_id, 0));
