-- The off-peak hours of the contract (JSON: [{"from": "22:00", "to": "06:00"}, …]), set by the grid
-- operator for each home: they cut the prices of the CRE's "Heures creuses" option into slots.
ALTER TABLE electricity_contract ADD COLUMN offpeak TEXT;
