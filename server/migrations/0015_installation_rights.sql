-- Rights are given per installation: 'read', 'control' (with or without the right to change
-- zone names and groups, routines and Airzone schedules) or 'admin' (everything, including the
-- installation's settings). The rights given until now to a user apply to each of their
-- installations.
CREATE TABLE user_installations_new (
    user_id         INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    installation_id INTEGER NOT NULL REFERENCES installations(id) ON DELETE CASCADE,
    permission      TEXT    NOT NULL CHECK (permission IN ('read', 'control', 'admin')),
    edit_zones      INTEGER NOT NULL DEFAULT 1,
    edit_routines   INTEGER NOT NULL DEFAULT 1,
    edit_schedules  INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (user_id, installation_id)
);
INSERT INTO user_installations_new (user_id, installation_id, permission, edit_zones, edit_routines, edit_schedules)
SELECT ui.user_id, ui.installation_id, ui.permission, u.edit_zones, u.edit_routines, u.edit_schedules
FROM user_installations ui JOIN users u ON u.id = ui.user_id;
DROP TABLE user_installations;
ALTER TABLE user_installations_new RENAME TO user_installations;

ALTER TABLE users DROP COLUMN edit_zones;
ALTER TABLE users DROP COLUMN edit_routines;
ALTER TABLE users DROP COLUMN edit_schedules;
