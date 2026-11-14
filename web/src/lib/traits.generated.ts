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

/** Traits described by this version (`trait@version`). */
export const TRAITS = ["climate.zone@1"] as const
