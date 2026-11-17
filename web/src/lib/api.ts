// Typed client for the Breezone server API (see docs/API.md).
import type { WeatherView } from "@/lib/weather"

/** The Android app on Google Play. */
export const ANDROID_APP_URL =
  "https://play.google.com/store/apps/details?id=com.breezone.app"
import { lang } from "@/lib/i18n"
import type {
  Contract,
  DeviceView,
  Installation,
  MeasureRole,
  Mode,
  NodeState,
  PriceView,
  Room,
  Tariff,
  TariffView,
  Zone,
} from "@/lib/api.generated"

/** The main views, generated from the contract of the server (`server/contracts/views.json`). */
export type {
  AirQuality,
  AirQualityLevel,
  ColorProfiles,
  Connection,
  Contract,
  DayColor,
  OffPeak,
  DayProfile,
  PriceAt,
  PricePeriod,
  PriceSlot,
  PriceView,
  Tariff,
  TariffCalendar,
  TariffVersion,
  TariffView,
  TempoView,
  DeviceView,
  Installation,
  InstallationUserRights,
  MeasureRole,
  Measures,
  Mode,
  NodeRef,
  NodeState,
  Room,
  Zone,
} from "@/lib/api.generated"

export type User = { id: number; username: string; role: "admin" | "user" }

export type TokenPair = {
  access_token: string
  refresh_token: string
  token_type: string
  expires_in: number
  refresh_expires_at: number
  user: User
}

/** An action of a routine on a device of a module, as `POST /api/devices/{id}/actions` takes it. */
export type DeviceAction = {
  device_id: number
  trait: string
  action: string
  params: Record<string, unknown>
  delay_minutes?: number | null
}

/** One action of a routine: a zone command, or an action of a device. */
export type RoutineAction = Action | DeviceAction

export const isDeviceAction = (a: RoutineAction): a is DeviceAction =>
  "device_id" in a

export type InstallationInput = {
  name: string
  host: string
  port: number
  enabled: boolean
  /** Left out: unchanged. */
  record_demand?: boolean
  /** City ("Labège, Haute-Garonne"); empty: back to the Airzone account's. Left out: unchanged. */
  location?: string
  /** ISO 3166-1 alpha-2 ("FR"). Left out: unchanged. */
  country?: string
}

export type InstallationConfig = {
  id: number
  name: string
  host: string
  port: number
  enabled: boolean
  created_at: number
  location: string | null
  /** ISO 3166-1 alpha-2, set by hand. */
  country: string | null
  /** The city was set by hand: the Airzone account no longer changes it. */
  location_set: boolean
  mac: string | null
  cloud_id: string | null
  node_id: number | null
  remote_id: number | null
  record_demand: boolean
}

/** This Breezone server, as other servers see it (federation, ADR 0008). */
export type NodeIdentity = {
  node_id: string
  name: string
  public_url: string | null
}

/** Version and capabilities of another server (ADR 0012), once they said hello. */
export type PeerView = {
  /** Breezone version on that server (unknown before 0.10.1). */
  version: string | null
  channel: string
  /** The server predates the exchange of capabilities (0.10.0 or older). */
  legacy: boolean
  checked_at: number | null
  compatibility: {
    state: "compatible" | "limited" | "outdated"
    /** What this server can do that the other cannot (`trait@v`, `feature@v`). */
    missing: string[]
    /** What the other server can do that this one cannot read yet. */
    newer: string[]
  }
  modules: { id: string; version: string; provides: string[] }[]
}

export type Share = {
  id: number
  label: string
  permission: "read" | "control"
  installations: { id: number; name: string }[]
  expires_at: number | null
  expired: boolean
  created_at: number
  created_by: string | null
  remote_name: string | null
  last_seen: number | null
  last_address: string | null
  connected: boolean
  /** "invited": code not used yet · "pending": a server asks for access · "active" */
  status: "invited" | "pending" | "active"
  pairing_expires_at: number | null
  pairing_expired: boolean
  requested_at: number | null
  /** The invitation asks the other server to share back. */
  reciprocal: boolean
  /** The other server accepted to share back (its installations are added on approval). */
  remote_shares_back: boolean
  /** Share given back to a server that invited this one. */
  reverse: boolean
  /** The server reading the share, once it said hello. */
  peer: PeerView | null
}

export type ShareInput = {
  label: string
  installation_ids: number[]
  permission: "read" | "control"
  expires_in_days?: number | null
  url?: string | null
  /** Ask the other server to share its installations too (creation only). */
  reciprocal?: boolean
}

/** Answer to a reciprocal invitation: what this server shares back. */
export type ReciprocalInput = {
  installation_ids: number[]
  permission: "read" | "control"
  url?: string | null
}

export type NodeStatus = {
  state: NodeState
  last_seen: number | null
  last_error: string | null
  latency_ms: number | null
  permission: "read" | "control"
}

export type BreezoneNode = {
  id: number
  name: string
  url: string
  created_at: number
  status: NodeStatus
  installations: { id: number; name: string }[]
  /** A certificate failing the usual HTTPS checks was trusted for this server. */
  certificate_pinned: boolean
  /** Its version and what it can do, once exchanged. */
  peer: PeerView | null
  /** Installations this server shares with us, and whether they are shown here. */
  offered: {
    remote_id: number
    name: string
    location: string | null
    included: boolean
  }[]
}

/** HTTPS certificate presented by another server, to review before trusting it. */
export type CertificateInfo = {
  /** SHA-256, lowercase hex */
  fingerprint: string
  subject: string | null
  names: string[]
  issuer: string | null
  not_before: number | null
  not_after: number | null
  self_signed: boolean
}

export type ZoneCommand = {
  on?: boolean
  setpoint?: number
  cool_setpoint?: number
  heat_setpoint?: number
  mode?: Mode
  speed?: number
  sleep?: number
}

export type Action = ZoneCommand & {
  installation_id: number
  system_id: number
  zone_id: number
  /** Run that many minutes after the routine starts. */
  delay_minutes?: number | null
}

export type TimerTarget =
  | {
      kind: "zone"
      installation_id: number
      system_id: number
      zone_id: number
    }
  | { kind: "group"; installation_id: number; group: string }
  | { kind: "installation"; installation_id: number }
  | { kind: "all" }
  /** The delayed action of a routine on a device. */
  | {
      kind: "device"
      device_id: number
      trait: string
      action: string
      params: Record<string, unknown>
    }

/** "Minuterie": one action planned later on a zone, group, installation or everything. */
export type Timer = {
  id: number
  label: string
  run_at: number
  target: TimerTarget
  command: ZoneCommand
  created_by: string | null
  created_at: number
}

export type AirzoneRoutine = {
  cloud_id: string
  installation: string
  id: string | null
  name: string
  enabled: boolean | null
}

export type Routine = {
  id: number
  name: string
  enabled: boolean
  actions: RoutineAction[]
  last_run_at: number | null
  created_at: number
  updated_at: number
}

/** A schedule of the Airzone app (mirror of Airzone Cloud; Airzone runs it). */
export type AirzoneSchedule = {
  cloud_id: string
  installation: string
  installation_id: number | null
  id: string
  name: string
  enabled: boolean
  kind: string
  days: number[]
  time: string | null
  zones: { system_id: number; zone_id: number }[]
  action: ZoneCommand
  end: { time: string; action: ZoneCommand } | null
  editable: boolean
  note: string | null
}

export type SchedulesState = {
  last_sync: number | null
  last_error: string | null
  account: boolean
  schedules: AirzoneSchedule[]
}

export type AirzoneScheduleInput = {
  cloud_id?: string
  name: string
  enabled: boolean
  days: number[]
  time: string
  zones: { system_id: number; zone_id: number }[]
  action: ZoneCommand
}

export type RoutineInput = {
  name: string
  enabled: boolean
  actions: RoutineAction[]
}

export type Reading = {
  ts: number
  room_temp: number | null
  humidity: number | null
  setpoint: number | null
  is_on: boolean | null
  mode: number | null
  /** Seconds in demand since the previous point (null: not recorded). */
  demand_secs: number | null
  /** 1 good, 2 regular, 3 bad, 0 no measure; null: no sensor. */
  air_quality: number | null
}

/** Room temperature of a zone over the last hours, a few points: `[ts, temperature]`. */
export type ZoneTrend = {
  system_id: number
  zone_id: number
  points: [number, number][]
}

/** Comfort of a zone over one local day (see docs/API.md). */
export type DayStats = {
  day: string
  min_temp: number | null
  max_temp: number | null
  avg_temp: number | null
  avg_humidity: number | null
  recorded_secs: number
  on_secs: number
  comfort_secs: number
  comfort: number | null
  demand_secs: number | null
  /** null when the zone has no air quality sensor. */
  air_quality_secs: { good: number; regular: number; bad: number } | null
}

export type FeedEvent = {
  id: number
  ts: number
  kind:
    | "connection"
    | "command"
    | "routine"
    | "automation"
    | "notification"
    | "presence"
    | "system"
    | "auth"
  level: "info" | "warn" | "error"
  installation_id: number | null
  actor: string | null
  message: string
  data: string | null
}

/** A request that failed on the server's side, kept in memory for the administrators. */
export type Incident = {
  ts: number
  method: string
  path: string
  status: number
  message: string
  cause: string | null
}

export type Session = {
  id: number
  device_name: string
  created_at: number
  last_used_at: number
  expires_at: number
  current: boolean
}

/** One installation given to a user (administrators see everything). */
export type AccessEntry = {
  installation_id: number
  /** "admin": every right on the installation, including its settings. */
  permission: "read" | "control" | "admin"
  /** With "control": what may be changed besides driving. */
  rights: UserRights
}

/** What a user may change on the installations they drive (all true for an administrator). */
export type UserRights = {
  /** Zone names and groups. */
  zones: boolean
  /** Create, change and delete routines (running them only needs control). */
  routines: boolean
  /** Create, change and delete Airzone schedules (switching on/off only needs control). */
  schedules: boolean
}

export const ALL_RIGHTS: UserRights = {
  zones: true,
  routines: true,
  schedules: true,
}

export type UserDetails = User & {
  created_at: number
  access: AccessEntry[]
}

export type Me = User & {
  session_id: number
  access: AccessEntry[] | null
  rights: UserRights
  /** Chosen in the profile; null follows the browser. */
  language: "fr" | "en" | "es" | null
}

export type CloudInstallation = {
  cloud_id: string
  name: string
  location: string | null
  zones: number
  installation_id: number | null
  reachable: boolean
  /** MAC address(es) of its WebServer, as known by Airzone. */
  ws_ids: string[]
  /** Fixed local address of its WebServer (null = found by scanning the server's network). */
  access: AccessPoint | null
}

export type AccessPoint = { host: string; port: number }

export type CloudStatus = {
  auto_sync: boolean
  email: string | null
  last_sync: number | null
  last_error: string | null
  installations: CloudInstallation[]
  default_subnet: string | null
}

export type SyncReport = {
  created: string[]
  updated: string[]
  unreachable: string[]
  notes: string[]
}

export type AttachResult = {
  attached: boolean
  installation_id: number | null
  message: string
  report: SyncReport
}

export type DiscoverySettings = {
  subnet: string | null
  effective: string | null
  in_container: boolean
}

export type Found = {
  host: string
  port: number
  mac: string | null
  ws_type: string | null
  firmware: string | null
  zones: number | null
}

export type ActionResult = { ok: boolean; error: string | null }

// ---------------------------------------------------------------------------------------
// Token storage. The refresh token is long-lived and sliding: the browser stays signed in
// as long as it is used at least once a year (or until the device is revoked).
// ---------------------------------------------------------------------------------------

const REFRESH_KEY = "breezone.refresh_token"
const USER_KEY = "breezone.user"

let accessToken: string | null = null
let accessExpiresAt = 0
let refreshing: Promise<boolean> | null = null
const listeners = new Set<() => void>()

export function onAuthChange(listener: () => void) {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

function notify() {
  listeners.forEach((l) => l())
}

export function currentUser(): User | null {
  const raw = localStorage.getItem(USER_KEY)
  return raw ? (JSON.parse(raw) as User) : null
}

export function isSignedIn() {
  return localStorage.getItem(REFRESH_KEY) !== null
}

export function storeTokens(pair: TokenPair) {
  accessToken = pair.access_token
  accessExpiresAt = Date.now() + (pair.expires_in - 30) * 1000
  localStorage.setItem(REFRESH_KEY, pair.refresh_token)
  localStorage.setItem(USER_KEY, JSON.stringify(pair.user))
  notify()
}

export function clearTokens() {
  accessToken = null
  accessExpiresAt = 0
  localStorage.removeItem(REFRESH_KEY)
  localStorage.removeItem(USER_KEY)
  notify()
}

/** Gets a fresh access token, refreshing it (once, even with concurrent callers). */
export async function getAccessToken(): Promise<string | null> {
  if (accessToken && Date.now() < accessExpiresAt) return accessToken
  if (!refreshing) {
    refreshing = doRefresh().finally(() => {
      refreshing = null
    })
  }
  return (await refreshing) ? accessToken : null
}

/**
 * Refreshes the tokens. Tabs share the refresh token (localStorage) and the server rotates it
 * on every use: the refresh runs under a lock shared by all the tabs, so they never rotate it
 * concurrently, and a tab that gets a 401 retries with the token another tab stored meanwhile
 * instead of signing out.
 */
async function doRefresh(): Promise<boolean> {
  const run = () => refreshOnce(2)
  if (typeof navigator !== "undefined" && navigator.locks?.request) {
    return navigator.locks.request("breezone-refresh", run)
  }
  return run()
}

async function refreshOnce(attempts: number): Promise<boolean> {
  const refresh = localStorage.getItem(REFRESH_KEY)
  if (!refresh) return false
  try {
    const res = await fetch("/api/auth/refresh", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "Accept-Language": lang(),
      },
      body: JSON.stringify({ refresh_token: refresh }),
    })
    if (res.status === 401) {
      // Another tab may have rotated the token in the meantime.
      const current = localStorage.getItem(REFRESH_KEY)
      if (current && current !== refresh && attempts > 1)
        return refreshOnce(attempts - 1)
      clearTokens()
      return false
    }
    if (!res.ok) return false
    storeTokens((await res.json()) as TokenPair)
    return true
  } catch {
    // Network error: keep the refresh token, try again later.
    return false
  }
}

export class ApiError extends Error {
  status: number
  /** Whole JSON body of the error (extra fields such as `certificate`). */
  data: Record<string, unknown>
  constructor(
    status: number,
    message: string,
    data: Record<string, unknown> = {}
  ) {
    super(message)
    this.status = status
    this.data = data
  }
}

/** A scene of a module's own app (Hue). */
export interface SceneView {
  source_id: number
  id: string
  name: string
  /** The room of the module's app it belongs to. */
  room: string | null
  device_ids: number[]
}

/** What an import of the configuration changed, and what is left to do by hand. */
export interface ConfigImported {
  installations: number
  modules: number
  devices: number
  routines: number
  /** Users added without a password: an administrator gives them one. */
  without_password: string[]
  /** Modules to pair again (HomeKit, Hue). */
  to_pair: string[]
  /** The Airzone account needs its password again. */
  airzone_password: boolean
}

export async function request<T>(
  method: string,
  path: string,
  body?: unknown,
  auth = true
): Promise<T> {
  const send = async (token: string | null) => {
    // Server messages (errors, activity feed) in the language of the page.
    const headers: Record<string, string> = { "Accept-Language": lang() }
    if (body !== undefined) headers["Content-Type"] = "application/json"
    if (token) headers["Authorization"] = `Bearer ${token}`
    return fetch(path, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    })
  }
  let res = await send(auth ? await getAccessToken() : null)
  if (auth && res.status === 401) {
    accessToken = null
    const token = await getAccessToken()
    if (token) res = await send(token)
  }
  if (!res.ok) {
    let message = res.statusText
    let data: Record<string, unknown> = {}
    try {
      data = (await res.json()) as Record<string, unknown>
      if (typeof data.error === "string") message = data.error
    } catch {
      /* not JSON */
    }
    if (res.status === 401 && auth) clearTokens()
    throw new ApiError(res.status, message, data)
  }
  if (res.status === 204) return undefined as T
  const text = await res.text()
  return (text ? JSON.parse(text) : undefined) as T
}

function deviceName() {
  const ua = navigator.userAgent
  const browser = /Firefox/.test(ua)
    ? "Firefox"
    : /Edg\//.test(ua)
      ? "Edge"
      : /Chrome/.test(ua)
        ? "Chrome"
        : /Safari/.test(ua)
          ? "Safari"
          : "Navigateur"
  const os = /Mac OS/.test(ua)
    ? "macOS"
    : /Windows/.test(ua)
      ? "Windows"
      : /Android/.test(ua)
        ? "Android"
        : /iPhone|iPad/.test(ua)
          ? "iOS"
          : /Linux/.test(ua)
            ? "Linux"
            : ""
  return `${browser}${os ? ` · ${os}` : ""} (web)`
}

const enc = encodeURIComponent

/** `GET /api/version`: the server software and the API it speaks (see docs/API.md). */
export type ServerVersion = {
  name: string
  version: string
  api_version: number
  min_client_api: number
}

/** Updates of Breezone itself (issue #40), for administrators. */
export type UpdateView = {
  current: string
  latest: string | null
  /** Release notes (Markdown). */
  notes: string | null
  published_at: string | null
  url: string | null
  available: boolean
  can_install: boolean
  reason: "disabled" | "platform" | "development" | "assets" | "failed" | null
  state:
    "idle" | "checking" | "downloading" | "installing" | "restarting" | "failed"
  /** Download progress, 0 to 1. */
  progress: number | null
  error: string | null
  checked_at: number | null
  /** The last update did not start and was undone. */
  rolled_back: { version: string; to: string } | null
  /** This server also receives the pre-releases (beta channel). */
  beta: boolean
  /** `latest` is a pre-release. */
  prerelease: boolean
}

/** Only the fields given change; `null` clears. */
export type DeviceInput = {
  name?: string | null
  room_id?: number | null
  position?: number
  role?: MeasureRole | null
  measures_device_id?: number | null
  measures_room_id?: number | null
}

export type DriverInfo = {
  id: string
  label: string
  provides: string[]
  models: string[]
  config: { name: string; type: string; optional: boolean; label: string }[]
}

export type SourceView = {
  id: number
  driver: string
  name: string
  config: Record<string, unknown>
  enabled: boolean
  created_at: number
  status: {
    state: "online" | "offline" | "error" | "disabled" | "unknown"
    last_seen: number | null
    last_error: string | null
    model: string | null
    firmware: string | null
  }
  devices: number
}

export type SourceInput = {
  driver: string
  name: string
  config: Record<string, unknown>
  enabled?: boolean
}

/** A supported product found on the network. */
export type FoundProduct = {
  brand: "airzone" | "shelly" | "v2c" | "hoymiles" | "homekit" | "hue"
  driver: string
  model: string
  host: string
  port: number
  mac: string | null
  device_id: string | null
  firmware: string | null
  note: string | null
  supported: boolean
  /** HomeKit feature flags (pairing). */
  flags: number
  added: boolean
}

/** What a consumption measure measures, as shown in the breakdown. */
export type Consumer = {
  name: string
  device_id: number | null
  room_id: number | null
}

/** Power of the home (W) per role; `null` for a role without a measure. */
export type EnergyFlows = {
  /** Drawn from the grid (negative: returned to it). */
  grid: number | null
  solar: number | null
  /** Charging (negative: discharging). */
  battery: number | null
  home: number | null
}

export type EnergyLive = EnergyFlows & {
  roles: Record<MeasureRole, boolean>
  /** Largest first. */
  consumers: (Consumer & { power: number })[]
  /** What the home uses beyond its consumption measures (with a grid meter). */
  other: number | null
}

/** Energy of the home over a local day (kWh); `null` for what it has no measure of. */
export type DayEnergy = {
  day: string
  imported: number | null
  exported: number | null
  production: number | null
  charged: number | null
  discharged: number | null
  consumption: number | null
  self_consumed: number | null
  consumers: (Consumer & { energy: number })[]
  other: number | null
  devices: { device_id: number; energy_in: number; energy_out: number }[]
  /** What the day cost, with a tariff of the electricity (null: none covers it). */
  cost?: DayCost | null
}

/** The cost of a day, in the currency of the contract, taxes included. */
export type DayCost = {
  /** The energy drawn from the grid, each kWh at its price. */
  energy: number
  /** The share of the subscription of that day. */
  subscription: number
  /** What the production used on the spot would have cost from the grid. */
  saved: number
  /** What the energy returned to the grid was paid. */
  earned: number
}

export type EnergyDay = {
  date: string
  bucket: number
  points: (EnergyFlows & { ts: number })[]
  totals: DayEnergy
}

export type DeviceHistory = {
  bucket: number
  series: Record<string, [number, number][]>
  /** The oldest point of the device (seconds): the periods offered stop there. */
  oldest?: number | null
}

/** A label of the catalog of traits, in every language. */
export type TraitLabel = { fr: string; en: string; es: string }

/** A value of the state of a trait, or a parameter of one of its actions. */
export type TraitField = {
  name: string
  type: "bool" | "integer" | "number" | "string" | "enum"
  unit?: string
  values?: string[]
  /** The words for each value of an enum. */
  value_labels?: Record<string, TraitLabel>
  min?: number
  max?: number
  optional?: boolean
  /** False for a setting or a fixed value of the hardware (not a measure). */
  history?: boolean
  label: TraitLabel
}

/** A trait of the catalog (`GET /api/traits`): what a kind of device holds and accepts. */
export type TraitInfo = {
  trait: string
  version: number
  label: TraitLabel
  state: TraitField[]
  actions: { name: string; label: TraitLabel; params: TraitField[] }[]
}

/** A number an automation can read now. */
export type AutomationMeasure =
  | { kind: "energy"; flow: "export" | "import" | "solar" | "home" }
  | { kind: "device"; device_id: number; field: string }
  | { kind: "outdoor" }
  | {
      kind: "weather"
      field:
        | "precipitation"
        | "wind"
        | "rain_next_hour"
        | "tomorrow_min"
        | "tomorrow_max"
        /** Relative humidity outside (%). */
        | "humidity"
        /** 0 green, 1 yellow, 2 orange, 3 red (Météo-France, through MeteoAlarm). */
        | "vigilance"
    }
  /** The price of electricity now, per kWh (a tariff of Settings › Electricity). */
  | { kind: "price" }
  /** The colour of a Tempo day: 1 blue, 2 white, 3 red. */
  | { kind: "tempo"; day: "today" | "tomorrow" }

export type AutomationOp = "above" | "below"

export type SunEvent = "sunrise" | "sunset" | "dawn" | "dusk"

/** Days: 1 (Monday) to 7 (Sunday); empty: every day. */
export type AutomationTrigger =
  | {
      kind: "measure"
      measure: AutomationMeasure
      op: AutomationOp
      value: number
      for_minutes: number
    }
  | {
      kind: "state"
      device_id: number
      field: string
      value: boolean | string
      for_minutes: number
    }
  | { kind: "time"; time: string; days: number[] }
  | { kind: "sun"; event: SunEvent; offset_minutes: number; days: number[] }
  /** The sun reaches a façade facing `facing` (degrees from the north). */
  | { kind: "facade"; facing: number }
  /** A button of a remote; any button or press when absent. */
  | {
      kind: "button"
      device_id: number
      button?: number | null
      press?: "single" | "double" | "long" | null
    }
  /** The first one arrives home (`home`), or the last one leaves. */
  | { kind: "presence"; home: boolean }

/** All true ("et") or at least one ("ou"). */
export type AutomationMatch = "all" | "any"

export type AutomationCondition =
  | {
      kind: "measure"
      measure: AutomationMeasure
      op: AutomationOp
      value: number
    }
  | { kind: "state"; device_id: number; field: string; value: boolean | string }
  | { kind: "months"; from: number; to: number }
  | { kind: "hours"; from: string; to: string }
  | { kind: "days"; days: number[] }
  | { kind: "daylight"; day: boolean }
  | { kind: "facade"; facing: number }
  | { kind: "presence"; home: boolean }
  /** One level only: no group in a group. */
  | {
      kind: "group"
      match: AutomationMatch
      conditions: AutomationCondition[]
    }

export type AutomationStep =
  | {
      kind: "device"
      device_id: number
      trait: string
      action: string
      params: Record<string, unknown>
    }
  | { kind: "scene"; routine_id: number }
  | { kind: "wait"; seconds: number }
  | {
      kind: "wait_until"
      condition: AutomationCondition
      for_seconds: number
      timeout_seconds: number
      on_timeout: "continue" | "stop"
    }
  /** Puts back the devices driven before, as they were. */
  | { kind: "restore" }
  /** The activity feed, the open apps and, when set, an ntfy topic. */
  | { kind: "notify"; message: string }

export type AutomationInput = {
  name: string
  enabled: boolean
  /** Any one starts the rule. */
  triggers: AutomationTrigger[]
  match: AutomationMatch
  conditions: AutomationCondition[]
  actions: AutomationStep[]
  /** Run when a trigger starts the rule and the conditions are false. */
  else: AutomationStep[]
  /** Started again while it waits: ignored, or started over. */
  mode: "single" | "restart"
  /** Run when the lasting trigger that started the rule no longer holds. */
  over: AutomationStep[]
}

/** A rule "quand… si… alors… sinon…", with its sentence and what it sees now. */
/** A device with a history, and the fields it has one of (what a chart can show). */
export type Charted = { device_id: number; fields: string[] }

/** The share of a row a block of the overview takes on a wide screen. */
/** The pages that are dashboards of blocks. */
export type DashboardPage =
  | "overview"
  | "energy"
  | "climate"
  | "lights"
  | "ventilation"
  | "charging"
  | "shutters"

export type DashboardSize = "quarter" | "half" | "full"

/** A block of the overview: what it shows is set by its type and `config`. */
export type DashboardBlock = {
  id: string
  type: string
  size: DashboardSize
  config: Record<string, unknown>
}

export type DashboardLayout = { version: 1; blocks: DashboardBlock[] }

/** The overview of an installation: the user's own, the installation's, or none yet. */
export type DashboardView = {
  layout: DashboardLayout | null
  source: "mine" | "installation" | "generated"
  has_default: boolean
  can_set_default: boolean
}

/** A rule written by a newer version: it does not run on this server. */
export type UnreadableAutomation = {
  id: number
  name: string
  enabled: boolean
  updated_at: number
}

export type Automation = AutomationInput & {
  id: number
  summary: {
    when: string
    triggers: string[]
    conditions: string[]
    /** "et" or "ou". */
    joined: string
    actions: string[]
    else: string[]
    over?: string[]
  }
  status: {
    value: number | null
    unit: string | null
    holds: boolean | null
    conditions: (boolean | null)[]
    next_at: number | null
    running: boolean
  }
  /** Enabled rules that may set the same device otherwise at the same time. */
  conflicts: { id: number; name: string; what: string }[]
  last_run_at: number | null
  last_reason: string | null
  created_at: number
  updated_at: number
}

/** What the automations share. */
export type AutomationSettings = {
  /** While set, no rule starts by itself: until this time, or until switched back on (0). */
  paused_until: number | null
  /** Administrators only. */
  ntfy_url?: string
}

/** Who is at home, as each user told it. */
export type Presence = {
  me: boolean | null
  someone_home: boolean | null
  people: {
    user_id: number
    username: string
    home: boolean
    changed_at: number
  }[]
}

/** A run of an automation, or a time it did not run (and why). */
export type AutomationRun = {
  started_at: number
  finished_at: number | null
  branch: "then" | "else" | "skipped" | "busy"
  reason: string
  /** null while it runs. */
  ended: "done" | "stopped" | "cancelled" | "interrupted" | null
  steps: { what: string; ok: boolean; error: string | null }[]
}

/** What a load does now on the solar surplus. */
export type SurplusStatus = {
  state:
    "solar" | "guarantee" | "waiting" | "no_car" | "unavailable" | "no_meter"
  amps: number | null
  power: number
  text: string
}

/** How a load is run on the solar surplus. */
export type SurplusLoadInput = {
  enabled: boolean
  min_amps?: number | null
  max_amps?: number | null
  /** A relay: its power when on (W). */
  power?: number | null
  guarantee_kwh?: number | null
  /** "HH:MM". */
  guarantee_by?: string | null
  min_on_minutes?: number
  /** What may be drawn from the grid before cutting back (W). */
  tolerance?: number
}

export type SurplusLoad = Required<SurplusLoadInput> & {
  device_id: number
  name: string
  kind: "charger" | "relay"
  position: number
  limit_min: number | null
  limit_max: number | null
  status: SurplusStatus | null
}

export type SurplusView = {
  /** The home has a grid meter (null: not looked at yet). */
  meter: boolean | null
  /** What goes back to the grid now (W; negative: drawn from it). */
  export: number | null
  /** In order of priority. */
  loads: SurplusLoad[]
  candidates: { device_id: number; name: string; kind: "charger" | "relay" }[]
}

export const api = {
  surplus: () => request<SurplusView>("GET", "/api/surplus"),
  saveSurplusLoad: (deviceId: number, input: SurplusLoadInput) =>
    request<SurplusView>("PUT", `/api/surplus/loads/${deviceId}`, input),
  removeSurplusLoad: (deviceId: number) =>
    request<void>("DELETE", `/api/surplus/loads/${deviceId}`),
  orderSurplus: (deviceIds: number[]) =>
    request<SurplusView>("PUT", "/api/surplus/order", {
      device_ids: deviceIds,
    }),
  traits: () => request<TraitInfo[]>("GET", "/api/traits"),
  charted: () => request<Charted[]>("GET", "/api/devices/charted"),
  /** The layout of a page of an installation ("overview", "energy"…). */
  dashboard: (installationId: number, page: DashboardPage = "overview") =>
    request<DashboardView>(
      "GET",
      `/api/installations/${installationId}/dashboard?page=${page}`
    ),
  /** The user's own layout of the page. */
  saveDashboard: (
    installationId: number,
    layout: DashboardLayout,
    page: DashboardPage = "overview"
  ) =>
    request<DashboardView>(
      "PUT",
      `/api/installations/${installationId}/dashboard?page=${page}`,
      layout
    ),
  /** Back to the installation's layout of the page. */
  resetDashboard: (installationId: number, page: DashboardPage = "overview") =>
    request<DashboardView>(
      "DELETE",
      `/api/installations/${installationId}/dashboard?page=${page}`
    ),
  /** The layout everyone sees on the page (administrators of the installation). */
  saveDefaultDashboard: (
    installationId: number,
    layout: DashboardLayout,
    page: DashboardPage = "overview"
  ) =>
    request<DashboardView>(
      "PUT",
      `/api/installations/${installationId}/dashboard/default?page=${page}`,
      layout
    ),
  automations: () => request<Automation[]>("GET", "/api/automations"),
  /** Rules this server version cannot read (administrators). */
  unreadableAutomations: () =>
    request<UnreadableAutomation[]>("GET", "/api/automations/unreadable"),
  createAutomation: (a: AutomationInput) =>
    request<Automation>("POST", "/api/automations", a),
  updateAutomation: (id: number, a: AutomationInput) =>
    request<Automation>("PUT", `/api/automations/${id}`, a),
  enableAutomation: (id: number, enabled: boolean) =>
    request<Automation>("PUT", `/api/automations/${id}/enabled`, { enabled }),
  deleteAutomation: (id: number) =>
    request<void>("DELETE", `/api/automations/${id}`),
  runAutomation: (id: number) =>
    request<{ results: ActionResult[]; running: boolean }>(
      "POST",
      `/api/automations/${id}/run`
    ),
  automationSettings: () =>
    request<AutomationSettings>("GET", "/api/automations/settings"),
  saveAutomationSettings: (
    input: Partial<{ paused_until: number | null; ntfy_url: string | null }>
  ) => request<AutomationSettings>("PUT", "/api/automations/settings", input),
  notifyTest: () => request<void>("POST", "/api/automations/notify-test"),
  presence: () => request<Presence>("GET", "/api/presence"),
  setPresence: (home: boolean) =>
    request<Presence>("PUT", "/api/presence", { home }),
  automationRuns: (id: number) =>
    request<AutomationRun[]>("GET", `/api/automations/${id}/runs`),
  devices: () => request<DeviceView[]>("GET", "/api/devices"),
  /** The history of a device over a period (timestamps), `bucket` seconds per point. */
  deviceHistoryBetween: (
    id: number,
    from: number,
    until: number,
    bucket: number
  ) =>
    request<DeviceHistory>(
      "GET",
      `/api/devices/${id}/history?from=${from}&until=${until}&bucket=${bucket}`
    ),
  /** The history of a device over the last `hours`, `bucket` seconds per point. */
  deviceHistoryOver: (id: number, hours: number, bucket: number) =>
    request<DeviceHistory>(
      "GET",
      `/api/devices/${id}/history?hours=${hours}&bucket=${bucket}`
    ),
  deviceHistory: (id: number, hours = 24, points = 96) =>
    request<DeviceHistory>(
      "GET",
      `/api/devices/${id}/history?hours=${hours}&points=${points}`
    ),
  act: (
    id: number,
    trait: string,
    action: string,
    params: Record<string, unknown>
  ) =>
    request<DeviceView>("POST", `/api/devices/${id}/actions`, {
      trait,
      action,
      params,
    }),
  energyLive: () => request<EnergyLive>("GET", "/api/energy/live"),
  /** The energy of a local day; its cost at the prices of `installationId` (the home by default). */
  energyDay: (date?: string, installationId?: number) => {
    const q = new URLSearchParams()
    if (date) q.set("date", date)
    if (installationId) q.set("installation_id", String(installationId))
    const query = q.toString()
    return request<EnergyDay>(
      "GET",
      `/api/energy/day${query ? `?${query}` : ""}`
    )
  },
  /** The electricity contract of an installation and its dated prices. */
  tariff: (installationId: number) =>
    request<TariffView>(
      "GET",
      `/api/installations/${installationId}/electricity`
    ),
  saveContract: (installationId: number, contract: Contract) =>
    request<TariffView>(
      "PUT",
      `/api/installations/${installationId}/electricity/contract`,
      contract
    ),
  /** The prices from a local day on ("YYYY-MM-DD"). */
  saveTariff: (installationId: number, validFrom: string, tariff: Tariff) =>
    request<TariffView>(
      "PUT",
      `/api/installations/${installationId}/electricity/prices/${validFrom}`,
      tariff
    ),
  deleteTariff: (installationId: number, validFrom: string) =>
    request<TariffView>(
      "DELETE",
      `/api/installations/${installationId}/electricity/prices/${validFrom}`
    ),
  /** The price now at an installation, and the stretches of the next `hours`. */
  price: (installationId: number, hours = 24) =>
    request<PriceView>(
      "GET",
      `/api/installations/${installationId}/electricity/price?hours=${hours}`
    ),
  /** The energy day by day; the costs at the prices of `installationId` (the home by default). */
  energyDays: (days: number, installationId?: number) =>
    request<DayEnergy[]>(
      "GET",
      `/api/energy/days?days=${days}${installationId ? `&installation_id=${installationId}` : ""}`
    ),
  /** The weather of the home and the sun of today. */
  /** The weather of an installation (by default the home). */
  weather: (installationId?: number) =>
    request<WeatherView>(
      "GET",
      `/api/weather${installationId ? `?installation_id=${installationId}` : ""}`
    ),
  /** The outdoor temperature over the last hours: `[ts, °C]`. */
  outdoorHistory: (hours: number, installationId?: number) =>
    request<[number, number][]>(
      "GET",
      `/api/weather/history?hours=${hours}${installationId ? `&installation_id=${installationId}` : ""}`
    ),
  /** The first day the home has an energy measure of. */
  energyRange: () =>
    request<{ first_day: string | null }>("GET", "/api/energy/range"),
  updateDevice: (id: number, input: DeviceInput) =>
    request<DeviceView>("PUT", `/api/devices/${id}`, input),
  rooms: () => request<Room[]>("GET", "/api/rooms"),
  createRoom: (input: { name: string; position: number }) =>
    request<Room>("POST", "/api/rooms", input),
  /** `icon` left out: unchanged; empty: guessed from the name again. */
  updateRoom: (
    id: number,
    input: { name: string; position: number; icon?: string }
  ) => request<Room>("PUT", `/api/rooms/${id}`, input),
  deleteRoom: (id: number) => request<void>("DELETE", `/api/rooms/${id}`),
  drivers: () => request<DriverInfo[]>("GET", "/api/drivers"),
  sources: () => request<SourceView[]>("GET", "/api/sources"),
  createSource: (input: SourceInput) =>
    request<SourceView>("POST", "/api/sources", input),
  updateSource: (id: number, input: SourceInput) =>
    request<SourceView>("PUT", `/api/sources/${id}`, input),
  deleteSource: (id: number) => request<void>("DELETE", `/api/sources/${id}`),
  /** Adds a Hue bridge: its round button pressed in the last 30 seconds. */
  linkHue: (input: { host: string; name: string }) =>
    request<SourceView>("POST", "/api/hue/link", input),
  /** Pairs Breezone with a HomeKit accessory (its 8-digit code), then adds it as a module. */
  pairHomekit: (input: {
    host: string
    name: string
    code: string
    flags: number
  }) => request<SourceView>("POST", "/api/homekit/pair", input),
  /** The configuration of this server as a file (no secret, no history). */
  exportConfig: () => request<unknown>("GET", "/api/system/config"),
  /** Replaces the configuration with an exported one. */
  importConfig: (file: unknown) =>
    request<ConfigImported>("POST", "/api/system/config", file),
  /** Takes the rooms of the module's own app (Hue): missing ones created, unplaced devices placed. */
  takeRooms: (sourceId: number) =>
    request<{ created: string[]; placed: number }>(
      "POST",
      `/api/sources/${sourceId}/rooms`
    ),
  /** What a module's hardware says about itself, raw (support). */
  sourceDiagnostic: (sourceId: number) =>
    request<unknown>("GET", `/api/sources/${sourceId}/diagnostic`),
  /** Gives a module its maker's key (Renson): the hardware keeps it, Breezone does not. */
  unlockSource: (sourceId: number, key: string) =>
    request<{ accepted: boolean }>("POST", `/api/sources/${sourceId}/key`, {
      key,
    }),
  /** The scenes of the modules that have some (Hue). */
  scenes: () => request<SceneView[]>("GET", "/api/scenes"),
  activateScene: (sourceId: number, id: string) =>
    request<void>("POST", `/api/scenes/${sourceId}/${encodeURIComponent(id)}`),
  /** Starts pairing: an accessory with a screen shows its code now. */
  startHomekitPairing: (input: { host: string; flags: number }) =>
    request<void>("POST", "/api/homekit/pair/start", input),
  cancelHomekitPairing: (host: string) =>
    request<void>("POST", "/api/homekit/pair/cancel", { host }),
  /** Looks for the source's hardware on the network, and follows it if its address changed. */
  locateSource: (id: number) =>
    request<
      SourceView & {
        /** The source knows its hardware's identity (else it cannot be looked for). */
        identified: boolean
        /** What the network announces of it (null: not announced). */
        seen: { host: string; paired: boolean | null } | null
        moved: boolean
      }
    >("POST", `/api/sources/${id}/locate`),
  refreshSource: (id: number) =>
    request<SourceView>("POST", `/api/sources/${id}/refresh`),
  scanProducts: (subnet?: string) =>
    request<{ subnet: string; duration_ms: number; products: FoundProduct[] }>(
      "POST",
      "/api/discovery/products",
      subnet ? { subnet } : {}
    ),
  version: () =>
    request<ServerVersion>("GET", "/api/version", undefined, false),
  update: () => request<UpdateView>("GET", "/api/system/update"),
  checkUpdate: () => request<UpdateView>("POST", "/api/system/update/check"),
  installUpdate: () => request<UpdateView>("POST", "/api/system/update"),
  setUpdateChannel: (beta: boolean) =>
    request<UpdateView>("PUT", "/api/system/update/channel", { beta }),
  setupStatus: () =>
    request<{ needs_setup: boolean }>("GET", "/api/setup", undefined, false),
  setup: async (username: string, password: string) => {
    storeTokens(
      await request<TokenPair>(
        "POST",
        "/api/setup",
        { username, password, device_name: deviceName() },
        false
      )
    )
  },
  login: async (username: string, password: string) => {
    storeTokens(
      await request<TokenPair>(
        "POST",
        "/api/auth/login",
        { username, password, device_name: deviceName() },
        false
      )
    )
  },
  logout: async () => {
    const refresh = localStorage.getItem(REFRESH_KEY)
    if (refresh)
      await request(
        "POST",
        "/api/auth/logout",
        { refresh_token: refresh },
        false
      ).catch(() => undefined)
    clearTokens()
  },
  me: () => request<Me>("GET", "/api/me"),
  setLanguage: (language: "fr" | "en" | "es" | null) =>
    request<void>("PUT", "/api/me/language", { language }),
  changePassword: (current_password: string, new_password: string) =>
    request<void>("PUT", "/api/me/password", {
      current_password,
      new_password,
    }),
  sessions: () => request<Session[]>("GET", "/api/sessions"),
  revokeSession: (id: number) => request<void>("DELETE", `/api/sessions/${id}`),

  users: () => request<UserDetails[]>("GET", "/api/users"),
  createUser: (
    username: string,
    password: string,
    role: string,
    access: AccessEntry[] = []
  ) =>
    request<UserDetails>("POST", "/api/users", {
      username,
      password,
      role,
      access,
    }),
  updateUser: (
    id: number,
    data: {
      role?: string
      password?: string
      access?: AccessEntry[]
    }
  ) => request<UserDetails>("PUT", `/api/users/${id}`, data),
  deleteUser: (id: number) => request<void>("DELETE", `/api/users/${id}`),

  status: () => request<Installation[]>("GET", "/api/status"),
  installation: (id: number) =>
    request<Installation>("GET", `/api/installations/${id}`),
  refreshInstallation: (id: number) =>
    request<Installation>("POST", `/api/installations/${id}/refresh`),
  installations: () =>
    request<InstallationConfig[]>("GET", "/api/installations"),
  createInstallation: (data: InstallationInput) =>
    request<Installation>("POST", "/api/installations", data),
  updateInstallation: (id: number, data: InstallationInput) =>
    request<Installation>("PUT", `/api/installations/${id}`, data),
  trends: (id: number, hours = 24) =>
    request<ZoneTrend[]>(
      "GET",
      `/api/installations/${id}/trends?hours=${hours}`
    ),
  deleteInstallation: (id: number) =>
    request<void>("DELETE", `/api/installations/${id}`),
  /** Says which installation of the Airzone account it is (null = none). */
  linkCloud: (id: number, cloud_id: string | null) =>
    request<Installation>("PUT", `/api/installations/${id}/cloud`, {
      cloud_id,
    }),

  identity: () => request<NodeIdentity>("GET", "/api/node"),
  saveIdentity: (name: string, public_url: string | null) =>
    request<NodeIdentity>("PUT", "/api/node", { name, public_url }),
  shares: () => request<Share[]>("GET", "/api/shares"),
  createShare: (data: ShareInput) =>
    request<{ share: Share; token: string }>("POST", "/api/shares", data),
  updateShare: (id: number, data: ShareInput) =>
    request<Share>("PUT", `/api/shares/${id}`, data),
  deleteShare: (id: number) => request<void>("DELETE", `/api/shares/${id}`),
  approveShare: (id: number) =>
    request<Share>("POST", `/api/shares/${id}/approve`),
  nodes: () => request<BreezoneNode[]>("GET", "/api/nodes"),
  addNode: (
    token: string,
    url?: string | null,
    reciprocal?: ReciprocalInput | null,
    certificate?: string | null
  ) =>
    request<BreezoneNode>("POST", "/api/nodes", {
      token,
      url,
      reciprocal,
      certificate,
    }),
  nodeCertificate: (id: number) =>
    request<{ pinned: string | null; certificate: CertificateInfo }>(
      "GET",
      `/api/nodes/${id}/certificate`
    ),
  trustNodeCertificate: (id: number, fingerprint: string) =>
    request<BreezoneNode>("PUT", `/api/nodes/${id}/certificate`, {
      fingerprint,
    }),
  setNodeInstallations: (id: number, included: number[]) =>
    request<BreezoneNode>("PUT", `/api/nodes/${id}/installations`, {
      included,
    }),
  untrustNodeCertificate: (id: number) =>
    request<BreezoneNode>("DELETE", `/api/nodes/${id}/certificate`),
  updateNode: (id: number, url: string) =>
    request<BreezoneNode>("PUT", `/api/nodes/${id}`, { url }),
  refreshNode: (id: number) =>
    request<BreezoneNode>("POST", `/api/nodes/${id}/refresh`),
  deleteNode: (id: number) => request<void>("DELETE", `/api/nodes/${id}`),
  testInstallation: (host: string, port: number) =>
    request<{ ok: boolean; error?: string; zones?: { name: string | null }[] }>(
      "POST",
      "/api/installations/test",
      { host, port }
    ),
  scan: (subnet?: string) =>
    request<{ subnet: string | null; found: Found[] }>(
      "POST",
      "/api/discovery/scan",
      subnet ? { subnet } : {}
    ),

  control: (
    z: { installation_id: number; system_id: number; zone_id: number },
    cmd: ZoneCommand
  ) =>
    request<Zone>(
      "PATCH",
      `/api/installations/${z.installation_id}/zones/${z.system_id}/${z.zone_id}`,
      cmd
    ),
  zoneMeta: (
    z: { installation_id: number; system_id: number; zone_id: number },
    meta: {
      display_name?: string | null
      group?: string | null
      position?: number
    }
  ) =>
    request<Zone | null>(
      "PUT",
      `/api/installations/${z.installation_id}/zones/${z.system_id}/${z.zone_id}/meta`,
      meta
    ),
  history: (
    z: { installation_id: number; system_id: number; zone_id: number },
    hours: number
  ) =>
    request<Reading[]>(
      "GET",
      `/api/installations/${z.installation_id}/zones/${z.system_id}/${z.zone_id}/history?hours=${hours}`
    ),
  /** How far back the history of a zone goes. */
  historyRange: (z: {
    installation_id: number
    system_id: number
    zone_id: number
  }) =>
    request<{ oldest: number | null }>(
      "GET",
      `/api/installations/${z.installation_id}/zones/${z.system_id}/${z.zone_id}/history/range`
    ),
  daily: (
    z: { installation_id: number; system_id: number; zone_id: number },
    days: number
  ) =>
    request<DayStats[]>(
      "GET",
      `/api/installations/${z.installation_id}/zones/${z.system_id}/${z.zone_id}/daily?days=${days}`
    ),
  batch: (actions: Action[]) =>
    request<{ results: ActionResult[] }>("POST", "/api/control/batch", {
      actions,
    }),

  incidents: () => request<Incident[]>("GET", "/api/system/incidents"),
  events: (limit = 100, before?: number) =>
    request<FeedEvent[]>(
      "GET",
      `/api/events?limit=${limit}${before ? `&before=${enc(String(before))}` : ""}`
    ),

  routines: () => request<Routine[]>("GET", "/api/routines"),
  createRoutine: (r: RoutineInput) =>
    request<Routine>("POST", "/api/routines", r),
  updateRoutine: (id: number, r: RoutineInput) =>
    request<Routine>("PUT", `/api/routines/${id}`, r),
  deleteRoutine: (id: number) => request<void>("DELETE", `/api/routines/${id}`),
  timers: () => request<Timer[]>("GET", "/api/timers"),
  createTimer: (t: {
    delay_minutes: number
    target: TimerTarget
    command: ZoneCommand
    label?: string
  }) => request<Timer>("POST", "/api/timers", t),
  cancelTimer: (id: number) => request<void>("DELETE", `/api/timers/${id}`),
  airzoneRoutines: () =>
    request<AirzoneRoutine[]>("GET", "/api/airzone/routines"),
  airzoneSchedules: () =>
    request<SchedulesState>("GET", "/api/airzone/schedules"),
  syncAirzoneSchedules: () =>
    request<SchedulesState>("POST", "/api/airzone/schedules/sync"),
  createAirzoneSchedule: (input: AirzoneScheduleInput) =>
    request<SchedulesState>("POST", "/api/airzone/schedules", input),
  updateAirzoneSchedule: (
    cloudId: string,
    id: string,
    input: AirzoneScheduleInput
  ) =>
    request<SchedulesState>(
      "PUT",
      `/api/airzone/schedules/${cloudId}/${id}`,
      input
    ),
  setAirzoneScheduleEnabled: (cloudId: string, id: string, enabled: boolean) =>
    request<SchedulesState>(
      "PUT",
      `/api/airzone/schedules/${cloudId}/${id}/enabled`,
      { enabled }
    ),
  deleteAirzoneSchedule: (cloudId: string, id: string) =>
    request<SchedulesState>(
      "DELETE",
      `/api/airzone/schedules/${cloudId}/${id}`
    ),
  runRoutine: (id: number) =>
    request<{ results: ActionResult[] }>("POST", `/api/routines/${id}/run`),

  cloud: () => request<CloudStatus>("GET", "/api/cloud"),
  cloudConnect: (email: string, password: string, remember: boolean) =>
    request<{ report: SyncReport; status: CloudStatus }>("PUT", "/api/cloud", {
      email,
      password,
      remember,
    }),
  cloudSync: () =>
    request<{ report: SyncReport; status: CloudStatus }>(
      "POST",
      "/api/cloud/sync"
    ),
  cloudForget: () => request<void>("DELETE", "/api/cloud"),
  setAccess: (
    cloudId: string,
    access: { host: string | null; port?: number }
  ) =>
    request<{ result: AttachResult; status: CloudStatus }>(
      "PUT",
      `/api/cloud/installations/${encodeURIComponent(cloudId)}/access`,
      access
    ),
  discoverySettings: () =>
    request<DiscoverySettings>("GET", "/api/discovery/settings"),
  saveDiscoverySettings: (subnet: string | null) =>
    request<DiscoverySettings>("PUT", "/api/discovery/settings", { subnet }),
}
