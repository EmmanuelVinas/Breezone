-- Charging profiles of the chargers (roadmap E39): the plan of a week in kinds of day cut into
-- slots ("off", "max" up to some kW, "solar"), with a calendar that may follow the Tempo colours
-- (`plan`, JSON). A charger follows one profile (`profile_id`), unless it is told to charge now:
-- at `now_amps` (`now_kw` asked) until it is unplugged, until `now_until`, or until `now_kwh` more
-- than the `now_base_energy` of its session when it was asked.
CREATE TABLE charge_profiles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    plan TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE charger_plans (
    device_id INTEGER PRIMARY KEY REFERENCES devices(id) ON DELETE CASCADE,
    profile_id INTEGER REFERENCES charge_profiles(id) ON DELETE SET NULL,
    now_amps INTEGER,
    now_kw REAL,
    now_until INTEGER,
    now_kwh REAL,
    now_base_energy REAL,
    updated_at INTEGER NOT NULL
);
