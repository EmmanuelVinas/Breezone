// Generated from server/contracts/traits by `BREEZONE_WRITE_GENERATED=1 cargo test --test generated` (in server/): do not edit.

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

/** Traits described by this version (`trait@version`). */
export const TRAITS = ["climate.zone@1", "cover.position@1", "energy.meter@1", "ev.charger@1", "light@1", "solar.inverter@1", "solar.panel@1", "energy.power@1", "switch.relay@1"] as const
