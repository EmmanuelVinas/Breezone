// Generated from server/contracts/views.json by `BREEZONE_WRITE_GENERATED=1 cargo test --test views` (in server/): do not edit.

export type AirQualityLevel = "good" | "regular" | "bad"

export type MeasureRole = "grid" | "solar" | "battery" | "consumption"

export type Mode = "stop" | "cooling" | "heating" | "fan" | "dry" | "aux_heating" | "auto"

export type NodeState = "online" | "offline" | "pending" | "certificate" | "revoked" | "expired" | "error" | "unknown"

/** Air quality, for thermostats with a sensor. */
export type AirQuality = {
  /** null while the sensor gives no measure. */
  level: AirQualityLevel | null
  /** Value read from the WebServer. */
  value: number
  /** Air purification. */
  mode: "off" | "on" | "auto" | null
  threshold_low: number | null
  threshold_high: number | null
}

/** The link with an installation's WebServer (or its node). */
export type Connection = {
  state: "online" | "offline" | "unknown"
  last_seen: number | null
  last_error: string | null
  latency_ms: number | null
}

/** A device of the home: an Airzone zone or a device of a module (ADR 0013). */
export type DeviceView = {
  id: number
  /** Main trait ("climate.zone"). */
  kind: string
  /** `trait@version` the device has. */
  traits: string[]
  /** Driver that provides it ("airzone", "shelly"). */
  source: string
  source_id: number | null
  installation_id: number | null
  /** Where to find it in its source (Airzone: `{system_id, zone_id}`). */
  reference: unknown
  name: string
  room_id: number | null
  role: MeasureRole | null
  measures: Measures
  position: number
  /** The source currently provides it (a zone removed from its installation is not). */
  available: boolean
  /** Values of its traits (fields described in the catalog). */
  state: Record<string, unknown> | null
  /** This user may drive it. */
  can_control: boolean
}

/** An Airzone installation, with its zones. */
export type Installation = {
  id: number
  name: string
  location: string | null
  /** Country (ISO 3166-1 alpha-2), set by hand. */
  country?: string
  host: string
  port: number
  enabled: boolean
  /** "local": WebServer on this server's network; "node": through another Breezone server. */
  access: "local" | "node"
  node?: NodeRef
  /** false when read-only (for this user, or shared read-only by its node). */
  controllable: boolean
  /** What the signed-in user may change on it. */
  rights: InstallationUserRights
  /** The time each zone spends in demand is recorded (set on the server that owns it). */
  record_demand: boolean
  connection: Connection
  zones: Zone[]
}

/** What a user may change on an installation, besides driving it. */
export type InstallationUserRights = {
  /** Zone names and groups. */
  zones: boolean
  routines: boolean
  /** Airzone schedules. */
  schedules: boolean
  /** The installation's own settings (address, Airzone link, deletion). */
  admin: boolean
}

/** What a "consumption" measure measures: a device, or a room. */
export type Measures = {
  device_id: number | null
  room_id: number | null
}

/** The node an installation is reached through. */
export type NodeRef = {
  id: number
  name: string
  state: NodeState
}

/** A room of the home. */
export type Room = {
  id: number
  name: string
  position: number
  created_at: number
  /** Chosen by hand (a Lucide name); null: guessed from the name. */
  icon: string | null
}

/** A zone of an installation. */
export type Zone = {
  installation_id: number
  system_id: number
  zone_id: number
  /** Name to display (Breezone custom name, else the Airzone name). */
  name: string
  airzone_name: string | null
  group: string | null
  position: number
  on: boolean
  room_temp: number | null
  humidity: number | null
  setpoint: number | null
  min_temp: number | null
  max_temp: number | null
  step: number
  units: "celsius" | "fahrenheit"
  mode: Mode | null
  /** Modes available on this zone's system. */
  modes: Mode[]
  is_master: boolean
  speed: number | null
  speeds: number[]
  sleep: number | null
  double_setpoint: boolean
  cool_setpoint: number | null
  heat_setpoint: number | null
  /** The zone is currently asking for heat or cold. */
  demand: boolean
  /** Thermostats with an air quality sensor only. */
  air_quality: AirQuality | null
  battery_low: boolean
  errors: string[]
}
