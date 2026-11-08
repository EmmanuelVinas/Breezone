// Typed client for the Breezone server API (see docs/API.md).

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
  battery_low: boolean
  errors: string[]
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
  /** false when the other server shares it read-only. */
  controllable: boolean
  connection: Connection
  zones: Zone[]
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
}

/** This Breezone server, as other servers see it (federation, ADR 0008). */
export type NodeIdentity = {
  node_id: string
  name: string
  public_url: string | null
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
  permission: "read" | "control"
}

export type UserDetails = User & { created_at: number; access: AccessEntry[] }

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
      headers: { "Content-Type": "application/json" },
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
    const headers: Record<string, string> = {}
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

export const api = {
  version: () =>
    request<ServerVersion>("GET", "/api/version", undefined, false),
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
  me: () => request<User & { session_id: number }>("GET", "/api/me"),
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
    data: { role?: string; password?: string; access?: AccessEntry[] }
  ) => request<UserDetails>("PUT", `/api/users/${id}`, data),
  deleteUser: (id: number) => request<void>("DELETE", `/api/users/${id}`),

  status: () => request<Installation[]>("GET", "/api/status"),
  installation: (id: number) =>
    request<Installation>("GET", `/api/installations/${id}`),
  refreshInstallation: (id: number) =>
    request<Installation>("POST", `/api/installations/${id}/refresh`),
  installations: () =>
    request<InstallationConfig[]>("GET", "/api/installations"),
  createInstallation: (data: {
    name: string
    host: string
    port: number
    enabled: boolean
  }) => request<Installation>("POST", "/api/installations", data),
  updateInstallation: (
    id: number,
    data: { name: string; host: string; port: number; enabled: boolean }
  ) => request<Installation>("PUT", `/api/installations/${id}`, data),
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
