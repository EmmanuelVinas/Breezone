-- What an automation does when its lasting trigger stops holding ("tant que… ; ensuite…"),
-- JSON steps as `actions`; empty: nothing.
ALTER TABLE automations ADD COLUMN over_actions TEXT NOT NULL DEFAULT '[]';
