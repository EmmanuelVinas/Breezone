// Generated from server/contracts/traits by `BREEZONE_WRITE_GENERATED=1 cargo test --test generated` (in server/): do not edit.

/** Remote or button (`button@1`): state. */
export type ButtonState = {
  /** Buttons */
  buttons: number
  /** Last button pressed */
  last_button: number | null
  /** Last press */
  last_press: "single" | "double" | "long" | null
  /** Time of the last press (s) */
  last_at: number | null
}

/** Air conditioning (`climate.zone@1`): state. */
export type ClimateZoneState = {
  /** On */
  on: boolean
  /** Temperature (°C) */
  room_temp: number | null
  /** Humidity (%) */
  humidity: number | null
  /** Setpoint (°C) */
  setpoint: number | null
  /** Cooling setpoint (°C) */
  cool_setpoint: number | null
  /** Heating setpoint (°C) */
  heat_setpoint: number | null
  /** Mode */
  mode: "stop" | "cooling" | "heating" | "fan" | "dry" | "aux_heating" | "auto" | null
  /** Fan speed */
  speed: number | null
  /** Sleep (min) */
  sleep: number | null
  /** In demand */
  demand: boolean
  /** Air quality */
  air_quality: "good" | "regular" | "bad" | null
}

/** Air conditioning (`climate.zone@1`): action `set`. */
export type ClimateZoneSet = {
  on?: boolean
  setpoint?: number
  cool_setpoint?: number
  heat_setpoint?: number
  mode?: "stop" | "cooling" | "heating" | "fan" | "dry" | "aux_heating" | "auto"
  speed?: number
  sleep?: number
}

/** Shutter, blind or window (`cover.position@1`): state. */
export type CoverPositionState = {
  /** Opening (%) */
  position: number
  /** Opening asked (%) */
  target: number | null
  /** Moving */
  moving: "opening" | "closing" | "stopped" | null
  /** Can be stopped on the way */
  stoppable: boolean | null
}

/** Shutter, blind or window (`cover.position@1`): action `set`. */
export type CoverPositionSet = {
  position: number
}

/** Shutter, blind or window (`cover.position@1`): action `stop`, without parameters. */
export type CoverPositionStop = Record<string, never>

/** Energy meter (`energy.meter@1`): state. */
export type EnergyMeterState = {
  /** Energy consumed (kWh) */
  energy_in: number
  /** Energy returned (kWh) */
  energy_out: number | null
}

/** EV charger (`ev.charger@1`): state. */
export type EvChargerState = {
  /** Status */
  status: "available" | "plugged" | "charging" | "fault"
  /** Vehicle plugged in */
  vehicle: boolean | null
  /** Paused */
  paused: boolean
  /** Locked */
  locked: boolean
  /** Session energy (kWh) */
  session_energy: number
  /** Session duration (s) */
  session_secs: number
  /** Charging current (A) */
  limit: number | null
  /** Minimum current (A) */
  limit_min: number | null
  /** Maximum current (A) */
  limit_max: number | null
  /** Phases */
  phases: number | null
  /** Current set by the charger */
  auto_limit: boolean | null
  /** Stops at (kWh) */
  energy_goal: number | null
}

/** EV charger (`ev.charger@1`): action `set_limit`. */
export type EvChargerSetLimit = {
  limit: number
}

/** EV charger (`ev.charger@1`): action `start`. */
export type EvChargerStart = {
  limit?: number
  energy?: number
}

/** EV charger (`ev.charger@1`): action `stop`, without parameters. */
export type EvChargerStop = Record<string, never>

/** EV charger (`ev.charger@1`): action `lock`. */
export type EvChargerLock = {
  locked: boolean
}

/** EV charger (`ev.charger@1`): action `set_auto_limit`. */
export type EvChargerSetAutoLimit = {
  on: boolean
}

/** Light (`light@1`): state. */
export type LightState = {
  /** On */
  on: boolean
  /** Brightness (%) */
  brightness: number | null
  /** White temperature (K) */
  color_temp: number | null
  /** Warmest white (K) */
  color_temp_min: number | null
  /** Coolest white (K) */
  color_temp_max: number | null
  /** Color */
  color: string | null
  /** Mode */
  mode: "white" | "color" | null
  /** Reachable */
  reachable: boolean | null
}

/** Light (`light@1`): action `set`. */
export type LightSet = {
  on?: boolean
  brightness?: number
  color_temp?: number
  color?: string
}

/** Solar inverter (`solar.inverter@1`): state. */
export type SolarInverterState = {
  /** Production today (kWh) */
  energy_today: number
  /** Total production (kWh) */
  energy_total: number
  /** Temperature (°C) */
  temperature: number | null
  /** Grid frequency (Hz) */
  frequency: number | null
  /** Panels */
  panels: number
}

/** Solar panel (`solar.panel@1`): state. */
export type SolarPanelState = {
  /** Power (W) */
  power: number
  /** Voltage (V) */
  voltage: number | null
  /** Current (A) */
  current: number | null
  /** Production today (kWh) */
  energy_today: number
  /** Total production (kWh) */
  energy_total: number
  /** Inverter */
  inverter: string
  /** Input */
  port: number
}

/** Power (`energy.power@1`): state. */
export type EnergyPowerState = {
  /** Power (W) */
  power: number
  /** Phase A (W) */
  power_a: number | null
  /** Phase B (W) */
  power_b: number | null
  /** Phase C (W) */
  power_c: number | null
  /** Voltage (V) */
  voltage: number | null
  /** Current (A) */
  current: number | null
}

/** Relay (`switch.relay@1`): state. */
export type SwitchRelayState = {
  /** On */
  on: boolean
}

/** Relay (`switch.relay@1`): action `set`. */
export type SwitchRelaySet = {
  on: boolean
}

/** Ventilation valve (`ventilation.room@1`): state. */
export type VentilationRoomState = {
  /** Air flow (m³/h) */
  flow: number
  /** Nominal flow (m³/h) */
  nominal: number | null
  /** Ventilation level (%) */
  level: number | null
  /** Profile */
  profile: "eco" | "health" | "intense" | null
  /** Boosting */
  boost: boolean
  /** Boost ends in (s) */
  boost_remaining: number | null
  /** Humidity (%) */
  humidity: number | null
  /** Temperature (°C) */
  temperature: number | null
  /** CO₂ (ppm) */
  co2: number | null
  /** VOC (ppm) */
  voc: number | null
}

/** Ventilation valve (`ventilation.room@1`): action `boost`. */
export type VentilationRoomBoost = {
  minutes: number
}

/** Ventilation valve (`ventilation.room@1`): action `stop_boost`, without parameters. */
export type VentilationRoomStopBoost = Record<string, never>

/** Ventilation valve (`ventilation.room@1`): action `set_profile`. */
export type VentilationRoomSetProfile = {
  profile: "eco" | "health" | "intense"
}

/** Ventilation unit (`ventilation.unit@1`): state. */
export type VentilationUnitState = {
  /** Air flow (m³/h) */
  flow: number
  /** Power (W) */
  power: number | null
  /** Air quality index */
  air_quality_index: number | null
  /** Main cause */
  main_pollutant: "humidity" | "co2" | "voc" | "other" | null
  /** Room most in demand */
  worst_room: string | null
  /** Boosting */
  boost: boolean
  /** Room measures */
  sensors: boolean
}

/** Ventilation unit (`ventilation.unit@1`): action `boost`. */
export type VentilationUnitBoost = {
  minutes: number
}

/** Ventilation unit (`ventilation.unit@1`): action `stop_boost`, without parameters. */
export type VentilationUnitStopBoost = Record<string, never>

/** Traits described by this version (`trait@version`). */
export const TRAITS = ["button@1", "climate.zone@1", "cover.position@1", "energy.meter@1", "ev.charger@1", "light@1", "solar.inverter@1", "solar.panel@1", "energy.power@1", "switch.relay@1", "ventilation.room@1", "ventilation.unit@1"] as const
