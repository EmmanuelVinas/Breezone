-- A zone of the air conditioning run on the solar surplus: it heats to `heat_setpoint` while the
-- outdoor temperature is below `heat_below` (and its system heats), cools to `cool_setpoint` while
-- it is above `cool_above` (and its system cools), then is put back as it was. `power` is what it
-- is reckoned to draw.
ALTER TABLE surplus_loads ADD COLUMN heat_setpoint REAL;
ALTER TABLE surplus_loads ADD COLUMN cool_setpoint REAL;
ALTER TABLE surplus_loads ADD COLUMN heat_below REAL;
ALTER TABLE surplus_loads ADD COLUMN cool_above REAL;
