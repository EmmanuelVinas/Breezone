//! Wire model of the Airzone local API (`/api/v1/hvac`).

use serde::{Deserialize, Serialize};

/// Airzone operation modes (`mode` on the master zone of a system).
/// Codes confirmed from the Airzone Local API / `aioairzone` reference library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationMode {
    Stop,
    Cooling,
    Heating,
    Fan,
    Dry,
    AuxHeating,
    Auto,
}

impl OperationMode {
    pub fn code(self) -> i64 {
        match self {
            OperationMode::Stop => 1,
            OperationMode::Cooling => 2,
            OperationMode::Heating => 3,
            OperationMode::Fan => 4,
            OperationMode::Dry => 5,
            OperationMode::AuxHeating => 6,
            OperationMode::Auto => 7,
        }
    }

    pub fn from_code(code: i64) -> Option<OperationMode> {
        Some(match code {
            1 => OperationMode::Stop,
            2 => OperationMode::Cooling,
            3 => OperationMode::Heating,
            4 => OperationMode::Fan,
            5 => OperationMode::Dry,
            6 => OperationMode::AuxHeating,
            7 => OperationMode::Auto,
            _ => return None,
        })
    }
}

/// Response of `POST /api/v1/hvac`. Depending on the query and firmware, zones are
/// returned either nested in `systems[].data[]` or directly in `data[]`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HvacResponse {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub systems: Vec<HvacSystem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data: Vec<ZoneData>,
}

impl HvacResponse {
    /// All zones, whatever the response shape.
    pub fn zones(&self) -> Vec<ZoneData> {
        let mut zones: Vec<ZoneData> = self.systems.iter().flat_map(|s| s.data.iter().cloned()).collect();
        zones.extend(self.data.iter().cloned());
        zones
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HvacSystem {
    #[serde(default)]
    pub data: Vec<ZoneData>,
}

/// One zone as returned by the WebServer. Unknown fields are ignored.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ZoneData {
    #[serde(rename = "systemID", default)]
    pub system_id: i64,
    #[serde(rename = "zoneID", default)]
    pub zone_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<i64>,
    #[serde(rename = "roomTemp", default, skip_serializing_if = "Option::is_none")]
    pub room_temp: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub humidity: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setpoint: Option<f64>,
    #[serde(rename = "maxTemp", default, skip_serializing_if = "Option::is_none")]
    pub max_temp: Option<f64>,
    #[serde(rename = "minTemp", default, skip_serializing_if = "Option::is_none")]
    pub min_temp: Option<f64>,
    #[serde(rename = "coolsetpoint", default, skip_serializing_if = "Option::is_none")]
    pub cool_setpoint: Option<f64>,
    #[serde(rename = "heatsetpoint", default, skip_serializing_if = "Option::is_none")]
    pub heat_setpoint: Option<f64>,
    #[serde(rename = "double_sp", default, skip_serializing_if = "Option::is_none")]
    pub double_setpoint: Option<i64>,
    #[serde(rename = "temp_step", default, skip_serializing_if = "Option::is_none")]
    pub temp_step: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modes: Option<Vec<i64>>,
    #[serde(rename = "master_zoneID", default, skip_serializing_if = "Option::is_none")]
    pub master_zone_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speeds: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_values: Option<Vec<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sleep: Option<i64>,
    #[serde(rename = "air_demand", default, skip_serializing_if = "Option::is_none")]
    pub air_demand: Option<i64>,
    #[serde(rename = "floor_demand", default, skip_serializing_if = "Option::is_none")]
    pub floor_demand: Option<i64>,
    /// Air quality of the zone (thermostats with a sensor): 0 none, 1 good, 2 regular, 3 bad.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aq_quality: Option<i64>,
    /// Air purification: 0 off, 1 on, 2 auto.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aq_mode: Option<i64>,
    /// Thresholds of the automatic air purification.
    #[serde(rename = "aq_thrlow", default, skip_serializing_if = "Option::is_none")]
    pub aq_threshold_low: Option<i64>,
    #[serde(rename = "aq_thrhigh", default, skip_serializing_if = "Option::is_none")]
    pub aq_threshold_high: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heat_demand: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cold_demand: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery: Option<i64>,
    #[serde(default)]
    pub errors: Vec<serde_json::Value>,
}

impl ZoneData {
    /// The master zone of a system is the one that carries the `modes` list
    /// (only it can change the operation mode).
    pub fn is_master(&self) -> bool {
        if let Some(master) = self.master_zone_id {
            return master == self.zone_id;
        }
        self.modes.as_ref().is_some_and(|m| !m.is_empty())
    }

    pub fn is_on(&self) -> bool {
        self.on.unwrap_or(0) != 0
    }

    /// Air quality level: "good", "regular" or "bad" (`None`: no sensor or no measure).
    pub fn air_quality_level(&self) -> Option<&'static str> {
        self.aq_quality.and_then(air_quality_level)
    }

    /// The zone is asking for heat or cold (air or radiant floor).
    pub fn in_demand(&self) -> bool {
        [self.air_demand, self.floor_demand, self.heat_demand, self.cold_demand].iter().any(|d| d.unwrap_or(0) != 0)
    }
}

/// Meaning of an `aq_quality` value: 1 good, 2 regular, 3 bad (0: no measure).
pub fn air_quality_level(value: i64) -> Option<&'static str> {
    match value {
        1 => Some("good"),
        2 => Some("regular"),
        3 => Some("bad"),
        _ => None,
    }
}

/// Master zone of a system: the one that publishes the available modes. Some WebServers report
/// every zone of a system as its own master (`master_zoneID` = its id), so `master_zoneID` is
/// only used when no zone of the system lists the modes.
pub fn master_zone(zones: &[ZoneData], system_id: i64) -> Option<i64> {
    let system = || zones.iter().filter(move |z| z.system_id == system_id);
    system().find(|z| z.modes.as_ref().is_some_and(|m| !m.is_empty())).or_else(|| system().find(|z| z.master_zone_id == Some(z.zone_id))).map(|z| z.zone_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_systems_shape() {
        let json = r#"{"systems":[{"data":[{"systemID":1,"zoneID":1,"name":"DKN Plus","on":1,
            "setpoint":79,"roomTemp":75,"modes":[4,3],"mode":3,"speeds":2,"speed":2,"humidity":0,
            "units":1,"errors":[],"air_demand":1,"floor_demand":0,"unknown_field":"x"}]}]}"#;
        let r: HvacResponse = serde_json::from_str(json).unwrap();
        let zones = r.zones();
        assert_eq!(zones.len(), 1);
        assert_eq!(zones[0].name.as_deref(), Some("DKN Plus"));
        assert!(zones[0].is_master());
        assert!(zones[0].is_on());
        assert_eq!(OperationMode::from_code(zones[0].mode.unwrap()), Some(OperationMode::Heating));
    }

    #[test]
    fn parses_flat_data_shape_and_errors() {
        let json = r#"{"data":[{"systemID":1,"zoneID":2,"name":"Salon","on":0,"roomTemp":19.5,
            "errors":[{"Zone":"Low battery"}]}]}"#;
        let r: HvacResponse = serde_json::from_str(json).unwrap();
        let zones = r.zones();
        assert_eq!(zones[0].zone_id, 2);
        assert!(!zones[0].is_master());
        assert_eq!(zones[0].errors.len(), 1);
    }

    #[test]
    fn one_master_per_system() {
        // Real WebServer 4.20: both zones of system 2 claim to be master; only one lists the modes.
        let json = r#"{"systems":[{"data":[
            {"systemID":1,"zoneID":1,"master_zoneID":2},
            {"systemID":1,"zoneID":2,"master_zoneID":2,"modes":[1,4,2,3,5]},
            {"systemID":2,"zoneID":5,"master_zoneID":5},
            {"systemID":2,"zoneID":6,"master_zoneID":6,"modes":[1,4,2,3,5]},
            {"systemID":3,"zoneID":1,"master_zoneID":1}]}]}"#;
        let zones = serde_json::from_str::<HvacResponse>(json).unwrap().zones();
        assert_eq!(master_zone(&zones, 1), Some(2));
        assert_eq!(master_zone(&zones, 2), Some(6));
        assert_eq!(master_zone(&zones, 3), Some(1));
        assert_eq!(master_zone(&zones, 4), None);
    }

    #[test]
    fn mode_codes_roundtrip() {
        for code in 1..=7 {
            assert_eq!(OperationMode::from_code(code).unwrap().code(), code);
        }
        assert_eq!(OperationMode::from_code(0), None);
    }
}
