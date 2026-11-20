import { Link } from "react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { ChevronRight, Play, Sun } from "lucide-react"
import { toast } from "sonner"
import type {
  DashboardBlock,
  DeviceView,
  EnergyDay,
  EnergyLive,
  Mode,
  Routine,
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
import { sensorText } from "@/lib/sensors"
import {
  PriceCompact,
  PriceNow,
} from "@/components/electricity/electricity-tab"
import { usePrice, useTariff } from "@/components/electricity/hooks"
import { ZoneCard } from "@/components/zone-card"
import { LIGHT_BRAND, LightCard, LightPanel, LightScenes } from "@/pages/lights"
import { ShutterCard } from "@/pages/shutters"
import { House, isRoom, isUnit, RoomCard } from "@/pages/ventilation"
import { ChargerCard, PowerCurve } from "@/pages/charging"
import { DaysCard, TodayCurve } from "@/pages/solar"
import { SolarPanels } from "@/components/solar-panels"
import { ZoneHistory } from "@/components/zone-history"
import { AirQualityCard } from "@/components/air-quality"
import { currentUser } from "@/lib/api"
import { RoomIcon } from "@/lib/room-icons"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Switch } from "@/components/ui/switch"
import { ChartBlock, VentilationRoomsChart } from "./chart-block"
import {
  EnergyBarsBlock,
  EnergyBreakdownBlock,
  EnergyCurveBlock,
  EnergyFlowBlock,
  EnergyKpisBlock,
  EnergyMeasuresBlock,
  EnergySurplusBlock,
} from "./energy-blocks"
import { useOverview } from "./context"
import type { EnergyFlow, GroupConfig } from "./model"
import {
  chartSeries,
  FLOW_LABEL,
  groupDevices,
  measuredFlows,
  shown,
  zoneOf,
} from "./model"

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

/** A small picture of a shutter: its slats cover what is closed. */
function ShutterGlyph({ position }: { position: number | null }) {
  const closed =
    position === null ? 0.5 : 1 - Math.min(100, Math.max(0, position)) / 100
  const slats = Math.round(closed * 5)
  return (
    <svg viewBox="0 0 16 16" className="size-4 shrink-0" aria-hidden>
      <rect
        x="1.5"
        y="1.5"
        width="13"
        height="13"
        rx="3"
        fill="none"
        stroke="currentColor"
        strokeOpacity="0.5"
      />
      {Array.from({ length: slats }, (_, i) => (
        <rect
          key={i}
          x="3"
          y={3 + i * 2.2}
          width="10"
          height="1.4"
          rx="0.5"
          fill="currentColor"
        />
      ))}
    </svg>
  )
}

/** What a device shows on a line: its state in words, its value, how its icon looks. */
function look(d: DeviceView): {
  text: string | null
  value: string | null
  tone: string | null
  shutter: number | null | undefined
} {
  const s = state(d)
  const none = { text: null, value: null, tone: null, shutter: undefined }
  if (!d.available) return { ...none, text: t("indisponible") }
  switch (deviceType(d).id) {
    case "climate": {
      const on = s.on === true
      const temp = num(s.room_temp)
      const mode =
        typeof s.mode === "string" ? MODE_LABEL[s.mode as Mode] : null
      return {
        ...none,
        text: on ? mode : t("Éteinte"),
        value:
          on && num(s.setpoint) !== null
            ? `${formatTemp(temp)} → ${formatTemp(num(s.setpoint))}`
            : formatTemp(temp),
        tone: !on ? null : s.mode === "cooling" ? "text-cool" : "text-heat",
      }
    }
    case "light": {
      const on = s.on === true
      const b = num(s.brightness)
      return {
        ...none,
        text: on
          ? b !== null
            ? `${Math.round(b)} %`
            : t("allumée")
          : t("éteinte"),
        tone: on ? "text-warning" : null,
      }
    }
    case "switch": {
      const on = s.on === true
      const p = num(s.power)
      return {
        ...none,
        text: on ? t("allumé") : t("éteint"),
        value: on && p !== null ? watts(p) : null,
        tone: on ? "text-warning" : null,
      }
    }
    case "shutter":
    case "window":
    case "door": {
      const c = coverState(d)
      return {
        ...none,
        value: coverText(c),
        shutter: typeof c.position === "number" ? c.position : null,
      }
    }
    case "charger": {
      const status = chargerStatus(s as Parameters<typeof chargerStatus>[0])
      const p = num(s.power)
      return {
        ...none,
        text: CHARGER_STATUS[status] ?? null,
        value:
          [
            p !== null && p > 0 ? watts(p) : null,
            num(s.session_energy) ? kwh(num(s.session_energy)) : null,
          ]
            .filter(Boolean)
            .join(" · ") || null,
        tone: status === "charging" ? "text-primary" : null,
      }
    }
    case "ventilation":
    case "room-air": {
      const level = num(s.level)
      const flow = num(s.flow)
      return {
        ...none,
        value:
          level !== null
            ? `${level} %`
            : flow !== null
              ? `${number(flow, 0)} m³/h`
              : null,
      }
    }
    case "sensor":
      return { ...none, value: sensorText(s) }
    default: {
      const p = num(s.power)
      return { ...none, value: p !== null ? watts(p) : null }
    }
  }
}

/** A device on a line of a card: what it is now, and its curve when the card shows one. Only to
 * look at: a device is driven from its own card. */
function DeviceLine({ d, curve }: { d: DeviceView; curve?: string }) {
  const { text, value, tone, shutter } = look(d)
  const history = useQuery({
    queryKey: ["device-history", d.id, 24],
    queryFn: () => api.deviceHistory(d.id, 24, 96),
    enabled: curve !== undefined,
    refetchInterval: 5 * 60_000,
  })
  const points = curve ? (history.data?.series[curve] ?? []) : []
  return (
    <div className="flex items-center gap-3 border-t border-border/60 py-3 first:border-t-0">
      <span className={cn("shrink-0", tone ?? "text-muted-foreground")}>
        {shutter !== undefined ? (
          <ShutterGlyph position={shutter} />
        ) : (
          <DeviceTypeIcon of={deviceType(d)} className="size-4" aria-hidden />
        )}
      </span>
      <span className="min-w-0 flex-1 truncate">
        {d.name}
        {text && <span className="text-muted-foreground"> · {text}</span>}
      </span>
      {points.length > 1 && (
        <Sparkline points={points} className="h-6 w-20 shrink-0 text-primary" />
      )}
      {value && (
        <span className="tabular shrink-0 text-sm text-muted-foreground">
          {value}
        </span>
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
    .filter((d) => deviceType(d).id === "climate")
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
          {room && config.type === undefined ? (
            <span className="inline-flex items-center gap-2">
              <RoomIcon
                room={room}
                className="size-4 text-muted-foreground"
                aria-hidden
              />
              {title}
            </span>
          ) : (
            title
          )}
        </Title>
        {list.length === 0 ? (
          <p className="py-3 text-sm text-muted-foreground">
            {t("Aucun appareil.")}
          </p>
        ) : (
          list.map((d) => (
            <DeviceLine
              key={d.id}
              d={d}
              curve={config.charts?.[String(d.id)]}
            />
          ))
        )}
      </CardContent>
    </Card>
  )
}

/**
 * A remote whose buttons are pressed from the overview: each starts the automations set on it, as
 * the remote itself would (what the remote does in its own app is not asked).
 */
function RemoteCard({ device }: { device: DeviceView }) {
  const qc = useQueryClient()
  const automations = useQuery({
    queryKey: ["automations"],
    queryFn: api.automations,
    staleTime: 60_000,
    retry: false,
  })
  const press = useMutation({
    mutationFn: (button: number) =>
      api.act(device.id, "button", "press", { button, press: "single" }),
    onSuccess: (_, button) => {
      toast.success(t("Bouton {n} appuyé", { n: button }))
      qc.invalidateQueries({ queryKey: ["devices"] })
      qc.invalidateQueries({ queryKey: ["automations"] })
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const count = Math.max(1, Number(state(device).buttons) || 1)
  const started = (n: number) =>
    (automations.data ?? [])
      .filter(
        (a) =>
          a.enabled &&
          a.triggers.some(
            (x) =>
              x.kind === "button" &&
              x.device_id === device.id &&
              (x.button == null || x.button === n) &&
              (x.press == null || x.press === "single")
          )
      )
      .map((a) => a.name)
  const usable = device.can_control && device.available
  return (
    <Card className="h-full rounded-3xl">
      <CardContent className="grid gap-3">
        <div className="flex items-center gap-2">
          <DeviceTypeIcon
            of={deviceType(device)}
            className="size-4 text-muted-foreground"
          />
          <span className="min-w-0 flex-1 truncate font-medium">
            {device.name}
          </span>
        </div>
        <div className="grid grid-cols-2 gap-2">
          {Array.from({ length: count }, (_, i) => i + 1).map((n) => {
            const names = started(n)
            return (
              <Button
                key={n}
                variant="outline"
                className="h-auto min-h-14 flex-col items-start gap-0.5 rounded-2xl px-3 py-2 text-left whitespace-normal"
                disabled={!usable || press.isPending}
                onClick={() => press.mutate(n)}
                title={
                  names.length
                    ? names.join(" · ")
                    : t("Aucun automatisme sur ce bouton")
                }
              >
                <span className="font-medium">{t("Bouton {n}", { n })}</span>
                <span className="line-clamp-2 text-xs font-normal text-muted-foreground">
                  {names.length ? names.join(" · ") : t("Aucun automatisme")}
                </span>
              </Button>
            )
          })}
        </div>
      </CardContent>
    </Card>
  )
}

/** A measure with its curve of the last 24 hours, its value large (as the figures of a page). */
function MeasureTile({ d, curve }: { d: DeviceView; curve: string }) {
  const { value, tone } = look(d)
  const history = useQuery({
    queryKey: ["device-history", d.id, 24],
    queryFn: () => api.deviceHistory(d.id, 24, 96),
    refetchInterval: 5 * 60_000,
  })
  const points = history.data?.series[curve] ?? []
  return (
    <Card className="h-full rounded-3xl">
      <CardContent className="grid h-full content-start gap-1">
        <div
          className={cn(
            "flex items-center gap-2 text-sm font-medium",
            tone ?? "text-primary"
          )}
        >
          <DeviceTypeIcon of={deviceType(d)} className="size-4" aria-hidden />
          <span className="min-w-0 truncate">{d.name}</span>
        </div>
        <div className="tabular font-heading text-3xl font-semibold">
          {d.available ? (value ?? "–") : t("Indisponible")}
        </div>
        {points.length > 1 ? (
          <Sparkline
            points={points}
            className={cn("mt-2 h-14 w-full", tone ?? "text-primary")}
            label={t("Dernières 24 h")}
          />
        ) : (
          <p className="text-xs text-muted-foreground">
            {t("La courbe vient avec l'historique.")}
          </p>
        )}
      </CardContent>
    </Card>
  )
}

/** A device driven from the overview: the card of its own page, or its line with its switch. */
function DeviceBlock({ id, curve }: { id: number; curve?: string }) {
  const { devices, installation } = useOverview()
  const qc = useQueryClient()
  const d = devices.find((x) => x.id === id)
  const toggleTrait =
    d &&
    (deviceType(d).id === "light"
      ? "light"
      : deviceType(d).id === "switch"
        ? "switch.relay"
        : deviceType(d).id === "climate"
          ? "climate.zone"
          : null)
  const act = useMutation({
    mutationFn: (on: boolean) => api.act(id, toggleTrait!, "set", { on }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["devices"] }),
    onError: (e) => toast.error(errorText(e)),
  })
  if (!d)
    return (
      <Card className="h-full rounded-3xl">
        <CardContent className="text-sm text-muted-foreground">
          {t("Appareil indisponible")}
        </CardContent>
      </Card>
    )
  const noop = () => {}
  switch (deviceType(d).id) {
    case "shutter":
    case "window":
    case "door":
      return (
        <ShutterCard
          device={d}
          selected={false}
          onSelect={noop}
          canDrive={d.can_control}
          fill
        />
      )
    case "light":
      return (
        <div className="h-full *:h-full">
          <LightCard
            light={d}
            selected={false}
            onSelect={noop}
            canDrive={d.can_control}
          />
        </div>
      )
    case "room-air":
      return (
        <div className="h-full *:h-full">
          <RoomCard
            device={d}
            demanding={false}
            selected={false}
            onSelect={noop}
            canDrive={d.can_control}
          />
        </div>
      )
    case "remote":
      return <RemoteCard device={d} />
    case "climate": {
      const ref = d.reference as { system_id?: number; zone_id?: number }
      const zone = installation.zones.find(
        (z) => z.system_id === ref.system_id && z.zone_id === ref.zone_id
      )
      if (zone)
        return (
          <div className="h-full *:h-full">
            <ZoneCard zone={zone} readOnly={!d.can_control} />
          </div>
        )
      break
    }
  }
  if (curve) return <MeasureTile d={d} curve={curve} />
  const on = state(d).on === true
  return (
    <Card className="h-full rounded-3xl" style={glow(tint([d]))}>
      <CardContent className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <DeviceLine d={d} />
        </div>
        {toggleTrait && (
          <Switch
            checked={on}
            disabled={!d.can_control || !d.available}
            loading={act.isPending}
            onCheckedChange={(v) => act.mutate(v)}
            aria-label={d.name}
          />
        )}
      </CardContent>
    </Card>
  )
}

/** What an energy flow is now: its value, a word on it, its colour. */
function figure(
  flow: EnergyFlow,
  live: EnergyLive | undefined,
  today: EnergyDay | undefined
) {
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
  return {
    value: v === null ? "–" : watts(flow === "battery" ? Math.abs(v) : v),
    sub,
    color:
      flow === "solar"
        ? "text-warning"
        : flow === "battery"
          ? "text-success"
          : flow === "grid"
            ? "text-cool"
            : "text-primary",
  }
}

/** One energy flow, with its curve of the day at the bottom. */
function EnergyBlock({ flow }: { flow: EnergyFlow }) {
  const { live, today } = useOverview()
  const { value, sub, color } = figure(flow, live, today)
  const points = (today?.points ?? [])
    .map((p): [number, number | null] => [p.ts, p[flow]])
    .filter((p): p is [number, number] => p[1] !== null)
  return (
    <Card className="h-full rounded-3xl">
      <CardContent className="flex h-full flex-col gap-1">
        <div className="grid gap-1">
          <span
            className={cn(
              "text-sm",
              flow === "home" || flow === "grid"
                ? "text-muted-foreground"
                : color
            )}
          >
            {FLOW_LABEL[flow]}
          </span>
          <span className="tabular font-heading text-3xl font-bold">
            {value}
          </span>
          {sub && <span className="text-sm text-muted-foreground">{sub}</span>}
        </div>
        {points.length > 1 && (
          <Sparkline
            points={points}
            className={cn("mt-auto h-14 pt-2", color)}
            label={t("Aujourd'hui")}
          />
        )}
      </CardContent>
    </Card>
  )
}

/** Several energy flows in one block, side by side. */
function EnergyFiguresBlock({ flows: chosen }: { flows: EnergyFlow[] }) {
  const { live, today } = useOverview()
  // A flow the home has no measure of (no battery yet) is left out.
  const measured = measuredFlows(live?.roles)
  const flows = chosen.filter((f) => measured.includes(f))
  return (
    <Card className="h-full rounded-3xl">
      <CardContent className="grid h-full grid-cols-2 gap-x-4 gap-y-3">
        {flows.map((flow) => {
          const { value, sub, color } = figure(flow, live, today)
          return (
            <div key={flow} className="grid content-start gap-0.5">
              <span
                className={cn(
                  "text-xs",
                  flow === "home" || flow === "grid"
                    ? "text-muted-foreground"
                    : color
                )}
              >
                {FLOW_LABEL[flow]}
              </span>
              <span className="tabular font-heading text-xl font-bold">
                {value}
              </span>
              {sub && (
                <span className="truncate text-xs text-muted-foreground">
                  {sub}
                </span>
              )}
            </div>
          )
        })}
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

/** The price of electricity now, the next hours, and the Tempo days. */
function PriceBlock() {
  const price = usePrice()
  const tariff = useTariff()
  if (!price.data?.now) return null
  return (
    <PriceNow
      price={price.data}
      tariff={tariff.data?.versions.at(-1)?.tariff}
      className="h-full"
    />
  )
}

/** One light, or the lights of a room together, with their brightness, whites and colours. */
function LightPanelBlock({ config }: { config: Record<string, unknown> }) {
  const { devices, rooms } = useOverview()
  const isLight = (d: DeviceView) => d.traits.includes("light@1")
  const room =
    typeof config.room_id === "number"
      ? rooms.find((r) => r.id === config.room_id)
      : undefined
  const lights = room
    ? devices.filter((d) => isLight(d) && d.room_id === room.id)
    : devices.filter((d) => isLight(d) && d.id === config.device_id)
  if (lights.length === 0)
    return (
      <Card className="h-full rounded-3xl">
        <CardContent className="text-sm text-muted-foreground">
          {t("Lumière indisponible")}
        </CardContent>
      </Card>
    )
  const one = lights[0]
  const subtitle = room
    ? t("{n} lumière(s)", { n: lights.length })
    : [
        rooms.find((r) => r.id === one.room_id)?.name,
        LIGHT_BRAND[one.source] ?? one.source,
      ]
        .filter(Boolean)
        .join(" · ")
  return (
    <LightPanel
      lights={lights}
      title={room ? room.name : one.name}
      subtitle={subtitle}
      temperature={config.temperature !== false}
      colors={config.colors !== false}
      compact
      className="h-full"
    />
  )
}

function PriceCompactBlock() {
  const price = usePrice()
  if (!price.data?.now) return null
  return <PriceCompact price={price.data} className="h-full" />
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

/** The zone of the installation a device is (a block of a zone's history or air). */
function useZone(id: number) {
  const { devices, installation } = useOverview()
  const d = devices.find((x) => x.id === id)
  const ref = d ? zoneOf(d) : undefined
  return ref
    ? installation.zones.find(
        (z) => z.system_id === ref.system_id && z.zone_id === ref.zone_id
      )
    : undefined
}

function ZoneHistoryBlock({ id }: { id: number }) {
  const zone = useZone(id)
  return zone ? <ZoneHistory zone={zone} /> : <Missing />
}

function AirQualityBlock({ id }: { id: number }) {
  const zone = useZone(id)
  return zone ? <AirQualityCard zone={zone} /> : <Missing />
}

function Missing() {
  return <Placeholder label={t("Appareil indisponible")} />
}

/** A charger (the one chosen, else the first): its card of the charging page, or its curve. */
function ChargerBlock({ id, curve }: { id?: number; curve: boolean }) {
  const { devices } = useOverview()
  const chargers = devices.filter((d) => d.traits.includes("ev.charger@1"))
  const d = chargers.find((x) => x.id === id) ?? chargers[0]
  // When the devices were last read: the end of a charge is counted from it.
  const readAt =
    useQuery({ queryKey: ["devices"], queryFn: api.devices }).dataUpdatedAt /
    1000
  if (!d) return <Missing />
  return curve ? (
    <PowerCurve device={d} />
  ) : (
    <ChargerCard device={d} readAt={readAt} />
  )
}

function SolarPanelsBlock() {
  const { devices } = useOverview()
  const now = new Date()
  const day = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`
  return <SolarPanels devices={devices} today={day} withCurve />
}

function VentilationBlock() {
  const { devices } = useOverview()
  const unit = devices.find(isUnit)
  if (!unit) return <Missing />
  const rooms = devices.filter(
    (d) => isRoom(d) && d.source_id === unit.source_id
  )
  return <House unit={unit} rooms={rooms} canDrive={unit.can_control} />
}

function LightScenesBlock() {
  const scenes = useQuery({
    queryKey: ["scenes"],
    queryFn: api.scenes,
    staleTime: 60_000,
  })
  const list = scenes.data ?? []
  if (list.length === 0) return <Placeholder label={t("Aucune scène.")} />
  return (
    <LightScenes scenes={list} canDrive={currentUser()?.role === "admin"} />
  )
}

/** What Enedis measured (Linky) day by day, what it cost, and how much the modules measure of it. */
function LinkyBlock({ days }: { days: number }) {
  const data = useQuery({
    queryKey: ["linky", days],
    queryFn: () => api.linky(days),
    staleTime: 30 * 60_000,
  })
  const list = data.data?.days ?? []
  const used = list.filter((d) => d.consumption !== null)
  if (used.length === 0)
    return <Placeholder label={t("Pas encore de relevé Linky.")} />
  const total = used.reduce((n, d) => n + (d.consumption ?? 0), 0)
  const costs = used.filter((d) => d.cost !== null)
  const cost = costs.reduce((n, d) => n + (d.cost ?? 0), 0)
  const estimated = costs.some((d) => d.estimated)
  const max = Math.max(...used.map((d) => d.consumption ?? 0), 0.001)
  // What the modules measured of what the meter counted, over the days both know.
  const both = used.filter((d) => d.measured !== null)
  const share =
    both.length > 0
      ? both.reduce((n, d) => n + (d.measured ?? 0), 0) /
        Math.max(
          both.reduce((n, d) => n + (d.consumption ?? 0), 0),
          0.001
        )
      : null
  return (
    <Card className="h-full rounded-3xl">
      <CardContent className="grid h-full gap-3">
        <Title meta={t("{n} jours", { n: used.length })}>
          {t("Linky · Enedis")}
        </Title>
        <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
          <span className="tabular font-heading text-2xl font-semibold">
            {kwh(total)}
          </span>
          {costs.length > 0 && (
            <span className="tabular text-muted-foreground">
              {estimated ? "≈ " : ""}
              {cost.toLocaleString(undefined, {
                style: "currency",
                currency: "EUR",
              })}
            </span>
          )}
        </div>
        <div
          className="flex h-20 items-end gap-px"
          aria-label={t("Consommation jour par jour")}
        >
          {used.map((d) => (
            <div
              key={d.day}
              title={`${d.day} · ${kwh(d.consumption)}`}
              className="min-w-0 flex-1 rounded-t-sm bg-cool/70"
              style={{ height: `${((d.consumption ?? 0) / max) * 100}%` }}
            />
          ))}
        </div>
        <p className="text-xs text-muted-foreground">
          {share !== null
            ? t(
                "Relevé par Enedis, le lendemain. Vos modules en mesurent {n} %.",
                { n: Math.round(share * 100) }
              )
            : t("Relevé par Enedis, le lendemain.")}
        </p>
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
      return Array.isArray(c.flows) ? (
        <EnergyFiguresBlock flows={c.flows as EnergyFlow[]} />
      ) : (
        <EnergyBlock flow={(c.flow as EnergyFlow) ?? "solar"} />
      )
    case "surplus":
      return <SurplusBlock />
    case "group":
      return <GroupBlock config={c as GroupConfig} />
    case "device":
      return (
        <DeviceBlock
          id={Number(c.device_id)}
          curve={typeof c.curve === "string" ? c.curve : undefined}
        />
      )
    case "chart":
      return (
        <ChartBlock
          id={Number(c.device_id)}
          fields={chartSeries(c)}
          hours={Number(c.hours ?? 24)}
          view={c.view === "comfort" ? "comfort" : "curves"}
          tall={c.tall === true}
        />
      )
    case "weather":
      return <WeatherBlock editing={editing} />
    case "scenes":
      return <ScenesBlock ids={c.routine_ids as number[] | undefined} />
    case "price":
      return <PriceBlock />
    case "price_now":
      return <PriceCompactBlock />
    case "energy_kpis":
      return <EnergyKpisBlock config={c} />
    case "energy_flow":
      return <EnergyFlowBlock />
    case "energy_curve":
      return <EnergyCurveBlock config={c} />
    case "energy_bars":
      return <EnergyBarsBlock config={c} />
    case "energy_breakdown":
      return <EnergyBreakdownBlock config={c} />
    case "energy_measures":
      return <EnergyMeasuresBlock config={c} />
    case "energy_surplus":
      return <EnergySurplusBlock />
    case "zone_history":
      return <ZoneHistoryBlock id={Number(c.device_id)} />
    case "air_quality":
      return <AirQualityBlock id={Number(c.device_id)} />
    case "light_scenes":
      return <LightScenesBlock />
    case "light_panel":
      return <LightPanelBlock config={c} />
    case "charger":
    case "charger_curve":
      return (
        <ChargerBlock
          id={typeof c.device_id === "number" ? c.device_id : undefined}
          curve={block.type === "charger_curve"}
        />
      )
    case "solar_today":
      return <TodayCurve />
    case "solar_days":
      return <DaysCard />
    case "solar_panels":
      return <SolarPanelsBlock />
    case "ventilation_house":
      return <VentilationBlock />
    case "ventilation_rooms":
      return (
        <VentilationRoomsChart
          field={typeof c.field === "string" ? c.field : "humidity"}
          hours={Number(c.hours ?? 24)}
          tall={c.tall === true}
        />
      )
    // An empty cell, to leave room: seen only while arranging.
    case "linky":
      return <LinkyBlock days={Number(c.days ?? 30)} />
    case "spacer":
      return editing ? (
        <Placeholder label={t("Espace vide")} />
      ) : (
        <div aria-hidden className="h-full" />
      )
    default:
      return null
  }
}
