-- A charging profile is now a power only: up to `kw` (NULL: the charger's maximum), with a colour
-- for the screens. When a charger charges is up to the automations: the profiles of before (kinds
-- of day, slots, calendar) are dropped, decided on 2026-11-24.
-- A charger follows `mode`: 'profile' (`profile_id`), 'off' (no charge) or 'solar' (the solar
-- surplus only); NULL: Breezone leaves it alone. `automation_id`: the automation that set it last
-- (NULL: by hand). `solar`: how it charges on the surplus (JSON: `kw`, `fixed`, `start_kw`).
UPDATE charger_plans SET profile_id = NULL;
DELETE FROM charge_profiles;
ALTER TABLE charge_profiles DROP COLUMN plan;
ALTER TABLE charge_profiles ADD COLUMN kw REAL;
ALTER TABLE charge_profiles ADD COLUMN color TEXT;
ALTER TABLE charger_plans ADD COLUMN mode TEXT;
ALTER TABLE charger_plans ADD COLUMN automation_id INTEGER;
ALTER TABLE charger_plans ADD COLUMN solar TEXT;
