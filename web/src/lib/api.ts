// Typed client for the Breezone server API (see docs/API.md).

/** The Android app on Google Play. */
export const ANDROID_APP_URL =
  "https://play.google.com/store/apps/details?id=com.breezone.app"
import { lang } from "@/lib/i18n"

export type Mode =
  "stop" | "cooling" | "heating" | "fan" | "dry" | "aux_heating" | "auto"

export type User = { id: number; username: string; role: "admin" | "user" }

export type TokenPair = {
  access_token: string
  refresh_token: string
  token_type: string
  expires_in: number
  refresh_expires_at: number
  user: User
}

export type Connection = {
  state: "online" | "offline" | "unknown"
  last_seen: number | null
  last_error: string | null
  latency_ms: number | null
}

export type Zone = {
  installation_id: number
  system_id: number
  zone_id: number
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
  modes: Mode[]
  is_master: boolean
  speed: number | null
  speeds: number[]
  sleep: number | null
  double_setpoint: boolean
  cool_setpoint: number | null
  heat_setpoint: number | null
  demand: boolean
  /** Thermostats with an air quality sensor only. */
  air_quality: AirQuality | null
  battery_low: boolean
  errors: string[]
}

export type AirQualityLevel = "good" | "regular" | "bad"

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

export type NodeState =
  | "online"
  | "offline"
  | "revoked"
  | "expired"
  | "error"
  | "pending"
  | "certificate"
  | "unknown"

export type NodeRef = { id: number; name: string; state: NodeState }

export type Installation = {
  id: number
  name: string
  location: string | null
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

export type InstallationInput = {
  name: string
  host: string
  port: number
  enabled: boolean
  /** Left out: unchanged. */
  record_demand?: boolean
}

export type InstallationConfig = {
  id: number
  name: string
  host: string
  port: number
  enabled: boolean
  created_at: number
  location: string | null
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
  actions: Action[]
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
  actions: Action[]
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
  kind: "connection" | "command" | "routine" | "system" | "auth"
  level: "info" | "warn" | "error"
  installation_id: number | null
  actor: string | null
  message: string
  data: string | null
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

export type InstallationUserRights = UserRights & {
  /** The installation's settings (address, Airzone link, deletion). */
  admin: boolean
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

/** Role of an energy measure (energy page). */
export type MeasureRole = "grid" | "solar" | "battery" | "consumption"

/** A device (ADR 0013): an Airzone zone, a Shelly channel or relay… described by its traits. */
export type DeviceView = {
  id: number
  kind: string
  traits: string[]
  source: string
  source_id: number | null
  installation_id: number | null
  reference: unknown
  name: string
  room_id: number | null
  role: MeasureRole | null
  measures: { device_id: number | null; room_id: number | null }
  position: number
  available: boolean
  state: Record<string, unknown> | null
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

export type Room = {
  id: number
  name: string
  position: number
  created_at: number
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
  brand: "airzone" | "shelly"
  driver: string
  model: string
  host: string
  port: number
  mac: string | null
  device_id: string | null
  firmware: string | null
  note: string | null
  supported: boolean
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
}

export const api = {
  devices: () => request<DeviceView[]>("GET", "/api/devices"),
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
  energyDay: (date?: string) =>
    request<EnergyDay>("GET", `/api/energy/day${date ? `?date=${date}` : ""}`),
  energyDays: (days: number) =>
    request<DayEnergy[]>("GET", `/api/energy/days?days=${days}`),
  updateDevice: (id: number, input: DeviceInput) =>
    request<DeviceView>("PUT", `/api/devices/${id}`, input),
  rooms: () => request<Room[]>("GET", "/api/rooms"),
  drivers: () => request<DriverInfo[]>("GET", "/api/drivers"),
  sources: () => request<SourceView[]>("GET", "/api/sources"),
  createSource: (input: SourceInput) =>
    request<SourceView>("POST", "/api/sources", input),
  updateSource: (id: number, input: SourceInput) =>
    request<SourceView>("PUT", `/api/sources/${id}`, input),
  deleteSource: (id: number) => request<void>("DELETE", `/api/sources/${id}`),
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
