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

/** Energy meter (`energy.meter@1`): state. */
export type EnergyMeterState = {
  /** Energy consumed (kWh) */
  energy_in: number
  /** Energy returned (kWh) */
  energy_out: number | null
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
export const TRAITS = ["climate.zone@1", "energy.meter@1", "energy.power@1", "switch.relay@1"] as const
