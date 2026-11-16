import { createContext, useContext } from "react"
import { Link } from "react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { ChevronRight, Play, Sun } from "lucide-react"
import { toast } from "sonner"
import type {
  DashboardBlock,
  DeviceView,
  EnergyDay,
  EnergyLive,
  Installation,
  Mode,
  Room,
  Routine,
  SurplusView,
  TraitInfo,
} from "@/lib/api"
import { api, isDeviceAction } from "@/lib/api"
import { CHARGER_STATUS, chargerStatus } from "@/lib/charger"
import { coverState, coverText } from "@/lib/covers"
import { deviceType, DeviceTypeIcon } from "@/lib/device-icons"
import {
  errorText,
  formatTemp,
  kwh,
  MODE_LABEL,
  number,
  watts,
} from "@/lib/format"
import { t, tn } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import { Sparkline } from "@/components/sparkline"
import { WeatherCard } from "@/components/weather-card"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Switch } from "@/components/ui/switch"
import type { EnergyFlow, GroupConfig } from "./model"
import { chartFields, FLOW_LABEL, groupDevices, shown } from "./model"

/** What the blocks read: fetched once by the page. */
export type OverviewData = {
  installation: Installation
  devices: DeviceView[]
  rooms: Room[]
  live: EnergyLive | undefined
  today: EnergyDay | undefined
  surplus: SurplusView | undefined
  routines: Routine[]
  traits: TraitInfo[]
}

export const OverviewContext = createContext<OverviewData | null>(null)

function useOverview() {
  const data = useContext(OverviewContext)
  if (!data) throw new Error("overview data missing")
  return data
}

const state = (d: DeviceView) => (d.state ?? {}) as Record<string, unknown>
const num = (v: unknown) => (typeof v === "number" ? v : null)

/** A card's tint: what dominates in it (heating, cooling, charging), nothing when all is off. */
function tint(devices: DeviceView[]) {
  const zones = devices.filter(
    (d) => d.kind === "climate.zone" && state(d).on === true
  )
  if (zones.some((d) => state(d).mode === "heating")) return "var(--heat)"
  if (zones.some((d) => state(d).mode === "cooling")) return "var(--cool)"
  if (
    devices.some(
      (d) =>
        d.kind === "ev.charger" &&
        chargerStatus(state(d) as Parameters<typeof chargerStatus>[0]) ===
          "charging"
    )
  )
    return "var(--primary)"
  return null
}

const glow = (color: string | null) =>
  color
    ? {
        backgroundImage: `radial-gradient(80% 120% at 0% 0%, color-mix(in oklch, ${color} 14%, transparent), transparent 70%)`,
      }
    : undefined

/** What a line says of a device, and the switch it offers. */
function describe(d: DeviceView): {
  text: string | null
  value: string | null
  toggle: { trait: string; on: boolean } | null
} {
  const s = state(d)
  if (!d.available)
    return { text: t("indisponible"), value: null, toggle: null }
  switch (d.kind) {
    case "climate.zone": {
      const on = s.on === true
      const temp = num(s.room_temp)
      const mode =
        typeof s.mode === "string" ? MODE_LABEL[s.mode as Mode] : null
      return {
        text: on ? mode : t("Éteinte"),
        value:
          on && num(s.setpoint) !== null
            ? `${formatTemp(temp)} → ${formatTemp(num(s.setpoint))}`
            : formatTemp(temp),
        toggle: { trait: "climate.zone", on },
      }
    }
    case "light": {
      const on = s.on === true
      const b = num(s.brightness)
      return {
        text: on && b !== null ? `${Math.round(b)} %` : null,
        value: null,
        toggle: { trait: "light", on },
      }
    }
    case "switch.relay": {
      const p = num(s.power)
      return {
        text: p !== null && s.on === true ? watts(p) : null,
        value: null,
        toggle: { trait: "switch.relay", on: s.on === true },
      }
    }
    case "ev.charger": {
      const status = chargerStatus(s as Parameters<typeof chargerStatus>[0])
      const p = num(s.power)
      return {
        text: CHARGER_STATUS[status] ?? null,
        value: [
          p !== null && p > 0 ? watts(p) : null,
          num(s.session_energy) ? kwh(num(s.session_energy)) : null,
        ]
          .filter(Boolean)
          .join(" · "),
        toggle: null,
      }
    }
    case "cover.position":
      return { text: null, value: coverText(coverState(d)), toggle: null }
    case "ventilation.room":
    case "ventilation.unit": {
      const level = num(s.level)
      const flow = num(s.flow)
      return {
        text: null,
        value:
          level !== null
            ? `${level} %`
            : flow !== null
              ? `${number(flow, 0)} m³/h`
              : null,
        toggle: null,
      }
    }
    default: {
      const p = num(s.power)
      return { text: null, value: p !== null ? watts(p) : null, toggle: null }
    }
  }
}

function DeviceLine({ d, large }: { d: DeviceView; large?: boolean }) {
  const qc = useQueryClient()
  const { text, value, toggle } = describe(d)
  const act = useMutation({
    mutationFn: (on: boolean) => api.act(d.id, toggle!.trait, "set", { on }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["devices"] }),
    onError: (e) => toast.error(errorText(e)),
  })
  const type = deviceType(d)
  const active = toggle?.on ?? false
  return (
    <div
      className={cn(
        "flex items-center gap-3",
        large ? "py-1" : "border-t border-border/60 py-3 first:border-t-0"
      )}
    >
      <DeviceTypeIcon
        of={type}
        className={cn(
          "size-4 shrink-0",
          active ? "text-warning" : "text-muted-foreground"
        )}
        aria-hidden
      />
      <span className="min-w-0 flex-1 truncate">
        {d.name}
        {text && <span className="text-muted-foreground"> · {text}</span>}
      </span>
      {value && (
        <span className="tabular shrink-0 text-sm text-muted-foreground">
          {value}
        </span>
      )}
      {toggle && (
        <Switch
          checked={toggle.on}
          disabled={!d.can_control || !d.available}
          loading={act.isPending}
          onCheckedChange={(on) => act.mutate(on)}
          aria-label={d.name}
        />
      )}
    </div>
  )
}

function Title({
  children,
  meta,
  to,
}: {
  children: React.ReactNode
  meta?: string | null
  to?: string
}) {
  return (
    <div className="mb-1 flex items-baseline justify-between gap-3">
      <h3 className="font-heading text-lg font-semibold">
        {to ? (
          <Link
            to={to}
            className="inline-flex items-center gap-1 hover:underline"
          >
            {children}
            <ChevronRight className="size-4" aria-hidden />
          </Link>
        ) : (
          children
        )}
      </h3>
      {meta && (
        <span className="tabular truncate text-sm text-muted-foreground">
          {meta}
        </span>
      )}
    </div>
  )
}

/** Where the devices of a type are handled in the menu. */
const TYPE_PAGE: Record<string, string> = {
  climate: "/climatisation",
  light: "/lumieres",
  shutter: "/volets",
  window: "/volets",
  door: "/volets",
  ventilation: "/ventilation",
  "room-air": "/ventilation",
  charger: "/recharge",
  solar: "/solaire",
  energy: "/energie",
}

function GroupBlock({ config }: { config: GroupConfig }) {
  const { devices, rooms } = useOverview()
  const list = groupDevices(config, devices).filter(shown)
  const room = rooms.find((r) => r.id === config.room_id)
  const sample = devices.find(
    (d) => config.type !== undefined && deviceType(d).id === config.type
  )
  const title =
    config.type !== undefined
      ? sample
        ? deviceType(sample).label
        : t("Appareils")
      : (room?.name ?? t("Sans pièce"))
  const temps = list
    .filter((d) => d.kind === "climate.zone")
    .map((d) => num(state(d).room_temp))
    .filter((v): v is number => v !== null)
  const meta =
    temps.length > 0
      ? temps.length > 1
        ? t("{temp} moy.", {
            temp: formatTemp(temps.reduce((a, b) => a + b, 0) / temps.length),
          })
        : formatTemp(temps[0])
      : tn(list.length, "{n} appareil", "{n} appareils")
  return (
    <Card className="h-full rounded-3xl" style={glow(tint(list))}>
      <CardContent>
        <Title
          meta={meta}
          to={config.type !== undefined ? TYPE_PAGE[config.type] : undefined}
        >
          {title}
        </Title>
        {list.length === 0 ? (
          <p className="py-3 text-sm text-muted-foreground">
            {t("Aucun appareil.")}
          </p>
        ) : (
          list.map((d) => <DeviceLine key={d.id} d={d} />)
        )}
      </CardContent>
    </Card>
  )
}

function DeviceBlock({ id }: { id: number }) {
  const { devices } = useOverview()
  const d = devices.find((x) => x.id === id)
  return (
    <Card className="h-full rounded-3xl" style={glow(d ? tint([d]) : null)}>
      <CardContent>
        {d ? (
          <DeviceLine d={d} large />
        ) : (
          <p className="text-sm text-muted-foreground">
            {t("Appareil indisponible")}
          </p>
        )}
      </CardContent>
    </Card>
  )
}

function EnergyBlock({ flow }: { flow: EnergyFlow }) {
  const { live, today } = useOverview()
  const f = {
    solar: live?.solar,
    home: live?.home,
    battery: live?.battery,
    grid: live?.grid,
  }
  const v = f[flow] ?? null
  const sub =
    flow === "solar"
      ? today?.totals.production != null
        ? t("{kwh} aujourd'hui", { kwh: kwh(today.totals.production) })
        : null
      : flow === "home"
        ? f.solar && f.home
          ? t("{n} % solaire", {
              n: Math.round((Math.min(f.solar, f.home) / f.home) * 100),
            })
          : null
        : flow === "battery"
          ? v === null
            ? null
            : v > 20
              ? t("en charge")
              : v < -20
                ? t("en décharge")
                : t("au repos")
          : v === null
            ? null
            : v < 0
              ? t("injecte")
              : t("soutire")
  const color =
    flow === "solar"
      ? "text-warning"
      : flow === "battery"
        ? "text-success"
        : "text-muted-foreground"
  return (
    <Card className="h-full rounded-3xl">
      <CardContent className="grid gap-1">
        <span className={cn("text-sm", color)}>{FLOW_LABEL[flow]}</span>
        <span className="tabular font-heading text-3xl font-bold">
          {v === null ? "–" : watts(flow === "battery" ? Math.abs(v) : v)}
        </span>
        {sub && <span className="text-sm text-muted-foreground">{sub}</span>}
      </CardContent>
    </Card>
  )
}

function SurplusBlock() {
  const { surplus } = useOverview()
  const exported = surplus?.export ?? null
  const doing = (surplus?.loads ?? [])
    .map((l) => l.status?.text)
    .filter(Boolean)
    .join(", ")
  return (
    <Card className="h-full rounded-3xl">
      <CardContent className="flex flex-wrap items-center gap-3">
        <Sun className="size-5 text-warning" aria-hidden />
        <span className="font-medium">
          {exported !== null && exported > 0
            ? t("Surplus solaire de {kw}", { kw: watts(exported) })
            : t("Pas de surplus solaire")}
        </span>
        {doing && (
          <span className="min-w-0 flex-1 truncate text-sm text-muted-foreground">
            {doing}
          </span>
        )}
        <Link
          to="/energie"
          className="ml-auto inline-flex items-center gap-1 text-sm font-medium hover:underline"
        >
          {t("Voir l'énergie")} <ChevronRight className="size-4" aria-hidden />
        </Link>
      </CardContent>
    </Card>
  )
}

function ChartBlock({
  id,
  field,
  hours,
}: {
  id: number
  field: string
  hours: number
}) {
  const { devices, traits } = useOverview()
  const d = devices.find((x) => x.id === id)
  const history = useQuery({
    queryKey: ["device-history", id, hours],
    queryFn: () => api.deviceHistory(id, hours, hours > 24 ? 168 : 96),
    enabled: d !== undefined,
    refetchInterval: 5 * 60_000,
  })
  const info = d
    ? chartFields(d, traits).find((f) => f.name === field)
    : undefined
  const now = d ? num(state(d)[field]) : null
  return (
    <Card className="h-full rounded-3xl">
      <CardContent className="grid gap-2">
        <Title
          meta={
            now !== null
              ? `${number(now, Math.abs(now) < 10 ? 1 : 0)}${info?.unit ? ` ${info.unit}` : ""}`
              : null
          }
        >
          {d
            ? `${d.name} · ${info?.label ?? field}`
            : t("Appareil indisponible")}
        </Title>
        {(history.data?.series[field]?.length ?? 0) > 1 ? (
          <Sparkline
            points={history.data!.series[field]}
            className="h-24 text-primary"
            label={info?.label}
          />
        ) : (
          <p className="grid h-24 place-items-center text-sm text-muted-foreground">
            {history.isLoading ? "…" : t("Pas encore d'historique.")}
          </p>
        )}
        <span className="text-xs text-muted-foreground">
          {hours > 24 ? t("7 derniers jours") : t("24 dernières heures")}
        </span>
      </CardContent>
    </Card>
  )
}

function ScenesBlock({ ids }: { ids?: number[] }) {
  const { routines, installation } = useOverview()
  const run = useMutation({
    mutationFn: (r: Routine) => api.runRoutine(r.id),
    onSuccess: (res, r) => {
      const failed = res.results.filter((x) => !x.ok).length
      if (failed)
        toast.warning(
          t("« {name} » : {failed} action(s) en échec", {
            name: r.name,
            failed,
          })
        )
      else toast.success(t("« {name} » exécutée", { name: r.name }))
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const list = routines.filter((r) =>
    ids && ids.length > 0
      ? ids.includes(r.id)
      : r.actions.some(
          (a) => isDeviceAction(a) || a.installation_id === installation.id
        )
  )
  return (
    <Card className="h-full rounded-3xl">
      <CardContent className="grid gap-3">
        <Title>{t("Scènes")}</Title>
        {list.length === 0 ? (
          <p className="text-sm text-muted-foreground">{t("Aucune scène.")}</p>
        ) : (
          <div className="flex flex-wrap gap-2">
            {list.map((r) => (
              <Button
                key={r.id}
                variant="outline"
                size="sm"
                disabled={run.isPending}
                onClick={() => run.mutate(r)}
              >
                <Play /> {r.name}
              </Button>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  )
}

/** A block of the overview; one of a type this version does not know shows nothing. */
/** While editing, a block with nothing to show yet still shows what it is. */
function Placeholder({ label }: { label: string }) {
  return (
    <Card className="h-full rounded-3xl border-dashed">
      <CardContent className="text-sm text-muted-foreground">
        {label}
      </CardContent>
    </Card>
  )
}

function WeatherBlock({ editing }: { editing: boolean }) {
  const { installation } = useOverview()
  const weather = useQuery({
    queryKey: ["weather", installation.id],
    queryFn: () => api.weather(installation.id),
    refetchInterval: 10 * 60_000,
  })
  if (weather.data?.weather) return <WeatherCard className="h-full" />
  return editing ? (
    <Placeholder label={t("Météo (pas encore de données)")} />
  ) : null
}

export function BlockView({
  block,
  editing = false,
}: {
  block: DashboardBlock
  editing?: boolean
}) {
  const c = block.config
  switch (block.type) {
    case "energy":
      return <EnergyBlock flow={(c.flow as EnergyFlow) ?? "solar"} />
    case "surplus":
      return <SurplusBlock />
    case "group":
      return <GroupBlock config={c as GroupConfig} />
    case "device":
      return <DeviceBlock id={Number(c.device_id)} />
    case "chart":
      return (
        <ChartBlock
          id={Number(c.device_id)}
          field={String(c.field ?? "")}
          hours={Number(c.hours ?? 24)}
        />
      )
    case "weather":
      return <WeatherBlock editing={editing} />
    case "scenes":
      return <ScenesBlock ids={c.routine_ids as number[] | undefined} />
    default:
      return null
  }
}
