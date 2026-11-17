-- The colour announced for each day (EDF Tempo: blue, white, red), for the tariffs whose calendar
-- follows it (roadmap E32). `day` is a local "YYYY-MM-DD".
CREATE TABLE day_colors (
    day        TEXT PRIMARY KEY,
    color      TEXT NOT NULL,
    fetched_at INTEGER NOT NULL
);
