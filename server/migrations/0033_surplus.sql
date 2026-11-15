-- Solar surplus steering (roadmap E6): the loads Breezone runs on the power that would go back to
-- the grid, in order of priority (`position`). A charger has its current set between `min_amps` and
-- `max_amps`, and may be guaranteed `guarantee_kwh` before `guarantee_by` ("07:00"); a relay
-- (water heater, pool pump…) of `power` watts is switched on and off. `tolerance` is what may be
-- drawn from the grid before cutting back (W).
CREATE TABLE surplus_loads (
    device_id INTEGER PRIMARY KEY REFERENCES devices(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1,
    min_amps INTEGER,
    max_amps INTEGER,
    power INTEGER,
    guarantee_kwh REAL,
    guarantee_by TEXT,
    min_on_minutes INTEGER NOT NULL DEFAULT 0,
    tolerance INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
