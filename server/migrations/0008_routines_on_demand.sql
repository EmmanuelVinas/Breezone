-- Routines are started on demand only; fixed-time schedules are those of the Airzone app
-- (run by Airzone, edited from FreeZone). Former scheduled routines become scenes.
UPDATE routines SET time = NULL, days = 0;
