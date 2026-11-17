-- What a user (not an administrator) may see and drive among the devices of the modules (Shelly,
-- Hue, shutters, ventilation…): all of them ('all'), those of a room ('room', its id), of a type
-- ('type': light, shutter, switch, energy…) or one device ('device', its id); 'read' or 'control'.
-- The zones of the Airzone installations keep their grants per installation.
CREATE TABLE user_device_access (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scope      TEXT NOT NULL,
    ref        TEXT NOT NULL DEFAULT '',
    permission TEXT NOT NULL,
    PRIMARY KEY (user_id, scope, ref)
);

-- Until now every user saw the devices of the modules, without driving them: kept so.
INSERT INTO user_device_access (user_id, scope, ref, permission) SELECT id, 'all', '', 'read' FROM users WHERE role != 'admin';
