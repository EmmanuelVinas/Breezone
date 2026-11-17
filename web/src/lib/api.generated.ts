// Generated from server/contracts/views.json by `BREEZONE_WRITE_GENERATED=1 cargo test --test views` (in server/): do not edit.

export type AirQualityLevel = "good" | "regular" | "bad"

export type DayColor = "blue" | "white" | "red"

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

/** The profile a day of an announced colour follows (Tempo). */
export type ColorProfiles = {
  blue?: string
  white?: string
  red?: string
}

/** The link with an installation's WebServer (or its node). */
export type Connection = {
  state: "online" | "offline" | "unknown"
  last_seen: number | null
  last_error: string | null
  latency_ms: number | null
}

/** The electricity contract of the home (one per server, as its meter). */
export type Contract = {
  /** ISO 3166-1 alpha-2 ("FR"). */
  country: string | null
  supplier: string | null
  /** "custom": prices set by hand. */
  offer: string
  /** Subscribed power (kVA). */
  power_kva: number | null
  /** ISO 4217 ("EUR"). */
  currency: string
}

/** The prices of a kind of day ("normal", "red day"…), slot by slot. */
export type DayProfile = {
  id: string
  name: string
  /** Chosen by hand ("#2563eb"). */
  color: string | null
  slots: PriceSlot[]
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

/** The price at a moment, and where it comes from. */
export type PriceAt = {
  /** Per kWh. */
  price: number
  slot: string
  profile: string
  profile_name: string
  /** The colour of the day, when the calendar follows one and it is known. */
  color: DayColor | null
  /** The day of the tariff ("YYYY-MM-DD"): the day before at night when a day starts at 06:00. */
  day: string
}

/** A stretch of time at one price. */
export type PricePeriod = {
  start: number
  end: number
  price: number
  slot: string
  profile: string
  profile_name: string
  color: DayColor | null
}

/** A slot of a profile of day: from its start until the next one. */
export type PriceSlot = {
  /** "HH:MM", local time; the first one is "00:00". */
  start: string
  /** "HP", "HC"… */
  name: string
  /** Per kWh, taxes included. */
  price: number
}

/** The price of electricity now, and over the next hours. */
export type PriceView = {
  currency: string
  /** null: no tariff for now. */
  now: PriceAt | null
  /** From now on. */
  periods: PricePeriod[]
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

/** The prices from a day on. */
export type Tariff = {
  /** Per month, taxes included. */
  subscription: number
  /** Per kWh returned to the grid; null: not sold. */
  export_price: number | null
  profiles: DayProfile[]
  calendar: TariffCalendar
}

/** Which profile a day follows. */
export type TariffCalendar = {
  /** "HH:MM": when a day of the tariff starts (Tempo: "06:00"). */
  day_start: string
  /** The profile of each day of the week, Monday first. */
  weekdays: string[]
  /** Instead of the day of the week, when the colour of the day is announced. */
  colors: ColorProfiles
}

/** A tariff and the local day it applies from. */
export type TariffVersion = {
  /** "YYYY-MM-DD". */
  valid_from: string
  tariff: Tariff
}

/** The contract of the home and its tariffs. */
export type TariffView = {
  /** null: not set yet. */
  contract: Contract | null
  /** Oldest first. */
  versions: TariffVersion[]
  /** This user may change them. */
  can_edit: boolean
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
