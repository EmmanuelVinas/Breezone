-- Installations a user (role 'user') can see, and whether they can drive them.
-- Administrators see and drive every installation. New installations are not given to users.
CREATE TABLE user_installations (
    user_id         INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    installation_id INTEGER NOT NULL REFERENCES installations(id) ON DELETE CASCADE,
    permission      TEXT    NOT NULL CHECK (permission IN ('read', 'control')),
    PRIMARY KEY (user_id, installation_id)
);

-- Users saw and drove every installation before: keep it that way for the existing ones.
INSERT INTO user_installations (user_id, installation_id, permission)
SELECT u.id, i.id, 'control' FROM users u CROSS JOIN installations i WHERE u.role = 'user';
