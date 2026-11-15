-- The outdoor temperature of the home, from the weather (every 30 minutes): shown with the curves of
-- the zones. A few dozen rows a day, kept for good.
CREATE TABLE outdoor_samples (
    ts INTEGER PRIMARY KEY,
    temperature REAL NOT NULL
);
