-- Installations another node shares that this node chose not to show (by their id over there).
-- Everything a node shares is shown unless excluded here.
CREATE TABLE node_excluded_installations (
    node_id   INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    remote_id INTEGER NOT NULL,
    PRIMARY KEY (node_id, remote_id)
);
