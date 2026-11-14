import { useState, type ReactNode } from "react"
import { Link } from "react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  Area,
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  ComposedChart,
  Line,
  Pie,
  PieChart,
  ReferenceLine,
  XAxis,
  YAxis,
} from "recharts"
import {
  BatteryCharging,
  ChevronLeft,
  ChevronRight,
  Download,
  Gauge,
  Home,
  Leaf,
  Plug,
  ShieldCheck,
  Sun,
  UtilityPole,
  Zap,
} from "lucide-react"
import type { LucideIcon } from "lucide-react"
import type { DayEnergy, DeviceView, EnergyLive, MeasureRole } from "@/lib/api"
import { api, currentUser } from "@/lib/api"
import { percent, time } from "@/lib/format"
import { inverterCounting, isInverter } from "@/lib/solar"
import { locale, t, tn } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import { PageHeader } from "@/components/app-shell"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
} from "@/components/ui/chart"
import type { ChartConfig } from "@/components/ui/chart"
import { Skeleton } from "@/components/ui/skeleton"
import { Switch } from "@/components/ui/switch"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

type Period = "day" | "week" | "month" | "year"

const PERIOD_DAYS: Record<Period, number> = {
  day: 1,
  week: 7,
  month: 30,
  year: 365,
}

/** Products providing the measures, as named in the page header. */
const DRIVER_LABEL: Record<string, string> = {
  shelly: "Shelly",
  hoymiles: "Hoymiles",
  trydan: "V2C Trydan",
}

/** Downloads rows as a CSV file (separator `;`, read by spreadsheets in every language). */
function downloadCsv(name: string, rows: (string | number | null)[][]) {
  const text = rows
    .map((r) => r.map((v) => (v === null ? "" : String(v))).join(";"))
    .join("\n")
  const url = URL.createObjectURL(new Blob([text], { type: "text/csv" }))
  const a = document.createElement("a")
  a.href = url
  a.download = name
  a.click()
  URL.revokeObjectURL(url)
}

const COLOR = {
  solar: "var(--warning)",
  home: "var(--foreground)",
  grid: "var(--cool)",
  battery: "var(--success)",
  other: "var(--muted-foreground)",
}

/** Colors of the consumers in the breakdown. */
const CONSUMER_COLORS = [
  "var(--chart-1)",
  "var(--chart-2)",
  "var(--chart-3)",
  "var(--chart-4)",
  "var(--primary)",
  "var(--heat)",
]

const ROLE_LABEL: Record<MeasureRole, string> = {
  grid: t("Réseau (toute la maison)"),
  solar: t("Production solaire"),
  battery: t("Batterie"),
  consumption: t("Consommation"),
}

function number(v: number, digits: number) {
  return v.toLocaleString(locale(), { maximumFractionDigits: digits })
}

export function watts(v: number | null | undefined) {
  if (v === null || v === undefined) return "–"
  return Math.abs(v) >= 1000
    ? `${number(v / 1000, 2)} kW`
    : `${Math.round(v)} W`
}

export function kwh(v: number | null | undefined) {
  if (v === null || v === undefined) return "–"
  if (Math.abs(v) < 0.005) v = 0
  return `${number(v, Math.abs(v) < 10 ? 2 : 1)} kWh`
}

function isoDay(d: Date) {
  const pad = (n: number) => String(n).padStart(2, "0")
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
}

function shiftDay(day: string, by: number) {
  const d = new Date(`${day}T12:00:00`)
  d.setDate(d.getDate() + by)
  return isoDay(d)
}

function dayLabel(day: string, style: "long" | "short") {
  return new Date(`${day}T12:00:00`).toLocaleDateString(
    locale(),
    style === "long"
      ? { weekday: "long", day: "numeric", month: "long" }
      : { day: "2-digit", month: "2-digit" }
  )
}

function sum(values: (number | null)[]) {
  const present = values.filter((v): v is number => v !== null)
  return present.length ? present.reduce((a, b) => a + b, 0) : null
}

/** The energy of several days added up (the breakdown merges each consumer). */
function total(days: DayEnergy[]): DayEnergy {
  const field = (k: keyof DayEnergy) =>
    sum(days.map((d) => d[k] as number | null))
  const consumers: DayEnergy["consumers"] = []
  for (const c of days.flatMap((d) => d.consumers)) {
    const same = consumers.find(
      (x) =>
        x.name === c.name &&
        x.device_id === c.device_id &&
        x.room_id === c.room_id
    )
    if (same) same.energy += c.energy
    else consumers.push({ ...c })
  }
  consumers.sort((a, b) => b.energy - a.energy)
  const devices: DayEnergy["devices"] = []
  for (const d of days.flatMap((x) => x.devices)) {
    const same = devices.find((x) => x.device_id === d.device_id)
    if (same) {
      same.energy_in += d.energy_in
      same.energy_out += d.energy_out
    } else devices.push({ ...d })
  }
  return {
    day: days[days.length - 1]?.day ?? "",
    imported: field("imported"),
    exported: field("exported"),
    production: field("production"),
    charged: field("charged"),
    discharged: field("discharged"),
    consumption: field("consumption"),
    self_consumed: field("self_consumed"),
    other: field("other"),
    consumers,
    devices,
  }
}

function ratio(a: number | null, b: number | null) {
  return a === null || b === null || b <= 0
    ? null
    : Math.min(1, Math.max(0, a / b))
}

function Kpi({
  icon: Icon,
  label,
  value,
  sub,
  color,
}: {
  icon: LucideIcon
  label: string
  value: string
  sub?: ReactNode
  color: string
}) {
  return (
    <Card className="gap-2 rounded-3xl py-4">
      <CardContent className="space-y-1 px-4">
        <div
          className="flex items-center gap-1.5 text-xs font-medium"
          style={{ color }}
        >
          <Icon className="size-3.5" />
          {label}
        </div>
        <div className="tabular font-heading text-2xl font-semibold">
          {value}
        </div>
        {sub && <div className="text-xs text-muted-foreground">{sub}</div>}
      </CardContent>
    </Card>
  )
}

function Kpis({ totals, live }: { totals: DayEnergy; live: EnergyLive }) {
  const { roles } = live
  const selfShare = ratio(totals.self_consumed, totals.production)
  const autonomy =
    roles.grid && (roles.solar || roles.battery)
      ? ratio(
          totals.consumption === null || totals.imported === null
            ? null
            : totals.consumption - totals.imported,
          totals.consumption
        )
      : null
  return (
    <div className="mb-6 grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-5">
      {roles.solar && (
        <Kpi
          icon={Sun}
          label={t("Production")}
          value={kwh(totals.production)}
          color={COLOR.solar}
        />
      )}
      <Kpi
        icon={Home}
        label={t("Consommation")}
        value={kwh(totals.consumption)}
        sub={
          roles.solar && totals.self_consumed !== null
            ? t("dont {kwh} solaires", { kwh: kwh(totals.self_consumed) })
            : undefined
        }
        color={COLOR.home}
      />
      {roles.grid && (
        <Kpi
          icon={UtilityPole}
          label={t("Réseau électrique")}
          value={kwh(totals.imported)}
          sub={
            totals.exported
              ? t("{kwh} injectés", { kwh: kwh(totals.exported) })
              : t("tirés du réseau")
          }
          color={COLOR.grid}
        />
      )}
      {roles.solar && selfShare !== null && (
        <Kpi
          icon={Leaf}
          label={t("Autoconsommation")}
          value={percent(selfShare)}
          sub={t("de la production utilisée sur place")}
          color="var(--primary)"
        />
      )}
      {autonomy !== null && (
        <Kpi
          icon={ShieldCheck}
          label={t("Autonomie")}
          value={percent(autonomy)}
          sub={t("de la consommation couverte")}
          color={COLOR.battery}
        />
      )}
      {roles.battery && (
        <Kpi
          icon={BatteryCharging}
          label={t("Batterie")}
          value={kwh(totals.discharged)}
          sub={t("restitués · {kwh} chargés", { kwh: kwh(totals.charged) })}
          color={COLOR.battery}
        />
      )}
    </div>
  )
}

/** A link of the flow diagram; the dots move toward where the energy goes. */
function FlowLink({
  power,
  toward,
  color,
}: {
  power: number | null
  toward: "left" | "right"
  color: string
}) {
  const idle = power === null || Math.abs(power) < 5
  return (
    <div
      aria-hidden
      className="fz-flow hidden h-2 min-w-10 flex-1 md:block"
      style={{ color }}
      data-reverse={toward === "left" ? "" : undefined}
      data-idle={idle ? "" : undefined}
    />
  )
}

function FlowNode({
  icon: Icon,
  label,
  value,
  sub,
  to,
  color,
}: {
  icon: LucideIcon
  label: string
  value: string
  sub?: string
  /** The page that details it. */
  to?: string
  color: string
}) {
  return (
    <div
      className="w-full rounded-2xl border px-3.5 py-3 md:w-48"
      style={{
        background: `color-mix(in oklch, ${color} 10%, var(--card))`,
        borderColor: `color-mix(in oklch, ${color} 35%, transparent)`,
      }}
    >
      <div
        className="flex items-center gap-1.5 text-xs font-medium"
        style={{ color }}
      >
        <Icon className="size-3.5" />
        <span className="truncate">{label}</span>
      </div>
      <div className="tabular font-heading text-xl font-semibold">{value}</div>
      {sub && (
        <div className="truncate text-xs text-muted-foreground">
          {to ? (
            <Link to={to} className="hover:text-foreground hover:underline">
              {sub} ›
            </Link>
          ) : (
            sub
          )}
        </div>
      )}
    </div>
  )
}

function FlowCard({ live, solarSub }: { live: EnergyLive; solarSub?: string }) {
  const { roles } = live
  const sources: ReactNode[] = []
  if (roles.solar)
    sources.push(
      <div key="solar" className="flex items-center">
        <FlowNode
          icon={Sun}
          label={t("Solaire")}
          value={watts(live.solar)}
          sub={solarSub}
          to={solarSub ? "/solaire" : undefined}
          color={COLOR.solar}
        />
        <FlowLink power={live.solar} toward="right" color={COLOR.solar} />
      </div>
    )
  if (roles.battery)
    sources.push(
      <div key="battery" className="flex items-center">
        <FlowNode
          icon={BatteryCharging}
          label={t("Batterie")}
          value={watts(live.battery)}
          sub={
            live.battery === null || Math.abs(live.battery) < 5
              ? t("au repos")
              : live.battery > 0
                ? t("en charge")
                : t("restitue")
          }
          color={COLOR.battery}
        />
        <FlowLink
          power={live.battery}
          toward={(live.battery ?? 0) > 0 ? "left" : "right"}
          color={COLOR.battery}
        />
      </div>
    )
  if (roles.grid)
    sources.push(
      <div key="grid" className="flex items-center">
        <FlowNode
          icon={UtilityPole}
          label={t("Réseau électrique")}
          value={watts(live.grid)}
          sub={(live.grid ?? 0) < 0 ? t("injectés") : t("tirés du réseau")}
          color={COLOR.grid}
        />
        <FlowLink
          power={live.grid}
          toward={(live.grid ?? 0) < 0 ? "left" : "right"}
          color={COLOR.grid}
        />
      </div>
    )
  const consumers = [
    ...live.consumers.slice(0, 4).map((c, i) => ({
      key: `${c.name}-${c.device_id}-${c.room_id}`,
      label: c.name,
      power: c.power,
      color: CONSUMER_COLORS[i % CONSUMER_COLORS.length],
    })),
    ...(live.other !== null && live.consumers.length > 0
      ? [
          {
            key: "other",
            label: t("Autres usages"),
            power: live.other,
            color: COLOR.other,
          },
        ]
      : []),
  ]
  const covered =
    live.home && live.grid !== null && (roles.solar || roles.battery)
      ? ratio(live.home - Math.max(0, live.grid), live.home)
      : null
  const surplus = (live.grid ?? 0) < 0 ? -(live.grid ?? 0) : null

  return (
    <Card className="mb-6 rounded-3xl">
      <CardHeader className="flex flex-row items-start justify-between gap-4">
        <div>
          <CardTitle>{t("En ce moment")}</CardTitle>
          <CardDescription>
            {t(
              "Mis à jour toutes les 5 s · les points vont dans le sens de l'énergie"
            )}
          </CardDescription>
        </div>
        {surplus !== null && (
          <Badge variant="outline" className="gap-1.5">
            <span className="size-2 rounded-full bg-success" />
            {t("Surplus {power}", { power: watts(surplus) })}
          </Badge>
        )}
      </CardHeader>
      <CardContent>
        <div className="grid items-center gap-4 md:grid-cols-[1fr_auto_1fr] md:gap-0">
          <div className="flex flex-col gap-3">{sources}</div>
          <div className="mx-auto grid size-40 place-items-center rounded-full border-8 border-muted bg-card text-center">
            <div>
              <div className="text-xs text-muted-foreground">{t("Maison")}</div>
              <div className="tabular font-heading text-2xl font-semibold">
                {watts(live.home)}
              </div>
              {covered !== null && (
                <div className="text-xs text-primary">
                  {t("{share} couverts", { share: percent(covered) })}
                </div>
              )}
            </div>
          </div>
          <div className="flex flex-col gap-3">
            {consumers.map((c) => (
              <div key={c.key} className="flex items-center">
                <FlowLink power={c.power} toward="right" color={c.color} />
                <FlowNode
                  icon={Plug}
                  label={c.label}
                  value={watts(c.power)}
                  color={c.color}
                />
              </div>
            ))}
          </div>
        </div>
      </CardContent>
    </Card>
  )
}

const curveConfig = {
  solar: { label: t("Production"), color: COLOR.solar },
  home: { label: t("Consommation"), color: COLOR.home },
  grid: { label: t("Réseau (+ tiré / − injecté)"), color: COLOR.grid },
  battery: {
    label: t("Batterie (+ charge / − restitue)"),
    color: COLOR.battery,
  },
} satisfies ChartConfig

function DayCurve({
  date,
  today,
  onDate,
  live,
}: {
  date: string
  today: string
  onDate: (d: string) => void
  live: EnergyLive
}) {
  const day = useQuery({
    queryKey: ["energy-day", date],
    queryFn: () => api.energyDay(date),
    refetchInterval: date === today ? 60_000 : false,
  })
  const kw = (v: number | null) => (v === null ? null : v / 1000)
  const points = (day.data?.points ?? []).map((p) => ({
    ts: p.ts,
    solar: kw(p.solar),
    home: kw(p.home),
    grid: kw(p.grid),
    battery: kw(p.battery),
  }))
  const filled = points.filter((p) => p.home !== null || p.grid !== null).length
  const measured = filled > 0
  // A lone point draws no line: dots until the day has a few.
  const dot = filled < 4 ? { r: 2.5 } : false
  const from = points[0]?.ts ?? 0
  const until = from + (day.data ? day.data.bucket * points.length : 86_400)
  const now = day.dataUpdatedAt / 1000
  const series = (
    Object.keys(curveConfig) as (keyof typeof curveConfig)[]
  ).filter((k) => k === "home" || live.roles[k])

  return (
    <Card className="mb-6 rounded-3xl">
      <CardHeader className="flex flex-row items-start justify-between gap-4">
        <div>
          <CardTitle className="first-letter:uppercase">
            {date === today ? t("Aujourd'hui") : dayLabel(date, "long")}
          </CardTitle>
          <CardDescription>
            {t("Puissance toutes les 15 minutes")}
          </CardDescription>
        </div>
        <div className="flex items-center gap-1">
          <Button
            variant="outline"
            size="icon"
            aria-label={t("Jour précédent")}
            onClick={() => onDate(shiftDay(date, -1))}
          >
            <ChevronLeft />
          </Button>
          <Button
            variant="outline"
            size="icon"
            aria-label={t("Jour suivant")}
            disabled={date >= today}
            onClick={() => onDate(shiftDay(date, 1))}
          >
            <ChevronRight />
          </Button>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        {day.isLoading ? (
          <Skeleton className="h-72 w-full" />
        ) : !measured ? (
          <div className="flex h-72 items-center justify-center text-center text-sm text-muted-foreground">
            {t(
              "Pas encore de relevés ce jour-là : ils s'accumulent toutes les 5 minutes."
            )}
          </div>
        ) : (
          <ChartContainer
            config={curveConfig}
            className="aspect-auto h-72 w-full"
          >
            <ComposedChart data={points} margin={{ left: 0, right: 8 }}>
              <CartesianGrid vertical={false} />
              <XAxis
                dataKey="ts"
                type="number"
                domain={[from, until]}
                scale="time"
                tickFormatter={(ts: number) => time(ts)}
                tickLine={false}
                axisLine={false}
                minTickGap={48}
              />
              <YAxis
                tickLine={false}
                axisLine={false}
                width={56}
                tickFormatter={(v: number) => `${number(v, 1)} kW`}
              />
              <ReferenceLine y={0} stroke="var(--border)" />
              {date === today && now < until && (
                <ReferenceLine
                  x={now}
                  stroke="var(--muted-foreground)"
                  strokeDasharray="3 3"
                />
              )}
              <ChartTooltip
                content={
                  <ChartTooltipContent
                    labelFormatter={(_, p) =>
                      p?.[0] ? time(Number(p[0].payload.ts)) : ""
                    }
                    formatter={(value, name, item) => (
                      <div className="flex w-full items-center justify-between gap-4">
                        <span className="flex items-center gap-1.5 text-muted-foreground">
                          <span
                            className="size-2 rounded-[2px]"
                            style={{ background: item.color }}
                          />
                          {curveConfig[name as keyof typeof curveConfig]?.label}
                        </span>
                        <span className="tabular font-medium">
                          {watts(Number(value) * 1000)}
                        </span>
                      </div>
                    )}
                  />
                }
              />
              {live.roles.solar && (
                <Area
                  dataKey="solar"
                  type="monotone"
                  stroke="var(--color-solar)"
                  fill="var(--color-solar)"
                  fillOpacity={0.18}
                  strokeWidth={1.5}
                  dot={dot}
                  connectNulls={false}
                  isAnimationActive={false}
                />
              )}
              {live.roles.grid && (
                <Line
                  dataKey="grid"
                  type="monotone"
                  stroke="var(--color-grid)"
                  strokeWidth={1.5}
                  dot={dot}
                  connectNulls={false}
                  isAnimationActive={false}
                />
              )}
              {live.roles.battery && (
                <Line
                  dataKey="battery"
                  type="monotone"
                  stroke="var(--color-battery)"
                  strokeWidth={1.5}
                  strokeDasharray="4 3"
                  dot={false}
                  connectNulls={false}
                  isAnimationActive={false}
                />
              )}
              <Line
                dataKey="home"
                type="monotone"
                stroke="var(--color-home)"
                strokeWidth={2}
                dot={dot}
                connectNulls={false}
                isAnimationActive={false}
              />
            </ComposedChart>
          </ChartContainer>
        )}
        <Legend
          items={series.map((k) => ({
            label: curveConfig[k].label,
            color: curveConfig[k].color,
          }))}
        />
      </CardContent>
    </Card>
  )
}

function Legend({ items }: { items: { label: string; color: string }[] }) {
  return (
    <div className="flex flex-wrap justify-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
      {items.map((i) => (
        <span key={i.label} className="inline-flex items-center gap-1.5">
          <span
            className="size-2.5 rounded-sm"
            style={{ background: i.color }}
          />
          {i.label}
        </span>
      ))}
    </div>
  )
}

const barsConfig = {
  self_consumed: { label: t("Autoconsommée"), color: COLOR.solar },
  discharged: { label: t("Batterie"), color: COLOR.battery },
  from_grid: { label: t("Réseau électrique"), color: COLOR.grid },
  consumption: { label: t("Consommation"), color: COLOR.grid },
  exported: {
    label: t("Injecté"),
    color: "color-mix(in oklch, var(--warning) 40%, transparent)",
  },
} satisfies ChartConfig

/** Days grouped by month ("YYYY-MM"), for a year. */
function byMonth(days: DayEnergy[]): DayEnergy[] {
  const months: DayEnergy[][] = []
  for (const d of days) {
    const last = months[months.length - 1]
    if (last && last[0].day.slice(0, 7) === d.day.slice(0, 7)) last.push(d)
    else months.push([d])
  }
  return months.map((m) => ({ ...total(m), day: m[0].day.slice(0, 7) }))
}

function barLabel(day: string, style: "long" | "short") {
  if (day.length === 7)
    return new Date(`${day}-15T12:00:00`).toLocaleDateString(
      locale(),
      style === "long" ? { month: "long", year: "numeric" } : { month: "short" }
    )
  return dayLabel(day, style)
}

/**
 * With a solar production: what it gave each day, used on the spot or returned (as the mockup).
 * Without: the energy the home used each day.
 */
function DayBars({ days, live }: { days: DayEnergy[]; live: EnergyLive }) {
  const { roles } = live
  const rows = days.length > 62 ? byMonth(days) : days
  const data = rows.map((d) => ({
    day: d.day,
    self_consumed: roles.solar ? (d.self_consumed ?? 0) : null,
    discharged: null as number | null,
    from_grid: null as number | null,
    consumption: roles.solar ? null : d.consumption,
    exported: roles.solar ? (d.exported ?? 0) : null,
  }))
  const keys = (
    ["self_consumed", "discharged", "from_grid", "consumption"] as const
  ).filter((k) => data.some((d) => d[k] !== null))
  const withExport = roles.solar
  const top = Math.max(
    0,
    ...data.map((d) => keys.reduce((a, k) => a + (d[k] ?? 0), 0)),
    ...data.map((d) => d.exported ?? 0)
  )
  const digits = top < 1 ? 2 : top < 10 ? 1 : 0
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{rows !== days ? t("Par mois") : t("Par jour")}</CardTitle>
        <CardDescription>
          {roles.solar
            ? t("Production : autoconsommée et injectée")
            : t("Énergie consommée")}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        <ChartContainer config={barsConfig} className="aspect-auto h-64 w-full">
          <BarChart data={data} margin={{ left: 0, right: 8 }}>
            <CartesianGrid vertical={false} />
            <XAxis
              dataKey="day"
              tickFormatter={(d: string) => barLabel(d, "short")}
              tickLine={false}
              axisLine={false}
              minTickGap={16}
            />
            <YAxis
              tickLine={false}
              axisLine={false}
              width={76}
              tickFormatter={(v: number) => `${number(v, digits)} kWh`}
            />
            <ChartTooltip
              content={
                <ChartTooltipContent
                  labelFormatter={(d) => barLabel(String(d), "long")}
                  formatter={(value, name, item) => (
                    <div className="flex w-full items-center justify-between gap-4">
                      <span className="flex items-center gap-1.5 text-muted-foreground">
                        <span
                          className="size-2 rounded-[2px]"
                          style={{ background: item.color }}
                        />
                        {barsConfig[name as keyof typeof barsConfig]?.label}
                      </span>
                      <span className="tabular font-medium">
                        {kwh(Number(value))}
                      </span>
                    </div>
                  )}
                />
              }
            />
            {keys.map((k, i) => (
              <Bar
                key={k}
                dataKey={k}
                stackId="used"
                fill={`var(--color-${k})`}
                radius={i === keys.length - 1 && !withExport ? [6, 6, 0, 0] : 0}
                isAnimationActive={false}
              />
            ))}
            {withExport && (
              <Bar
                dataKey="exported"
                stackId="used"
                fill="var(--color-exported)"
                radius={[6, 6, 0, 0]}
                isAnimationActive={false}
              />
            )}
          </BarChart>
        </ChartContainer>
        <Legend
          items={[...keys, ...(withExport ? (["exported"] as const) : [])].map(
            (k) => ({ label: barsConfig[k].label, color: barsConfig[k].color })
          )}
        />
      </CardContent>
    </Card>
  )
}

function Breakdown({ totals, period }: { totals: DayEnergy; period: Period }) {
  const parts = [
    ...totals.consumers.map((c, i) => ({
      name: c.name,
      energy: c.energy,
      color: CONSUMER_COLORS[i % CONSUMER_COLORS.length],
    })),
    ...(totals.other !== null && totals.other > 0
      ? [{ name: t("Autres usages"), energy: totals.other, color: COLOR.other }]
      : []),
  ].filter((p) => p.energy > 0)
  const all = parts.reduce((a, p) => a + p.energy, 0)
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{t("Où va l'énergie")}</CardTitle>
        <CardDescription>
          {period === "day"
            ? t("Consommation du jour par usage")
            : t("Consommation de la période par usage")}
        </CardDescription>
      </CardHeader>
      <CardContent>
        {all <= 0 ? (
          <div className="flex h-44 items-center justify-center text-sm text-muted-foreground">
            {t("Rien de mesuré sur la période.")}
          </div>
        ) : (
          <div className="flex flex-col items-center gap-4 sm:flex-row">
            <div className="relative size-44 shrink-0">
              <ChartContainer config={{}} className="size-44">
                <PieChart>
                  <Pie
                    data={parts}
                    dataKey="energy"
                    nameKey="name"
                    innerRadius={56}
                    outerRadius={80}
                    strokeWidth={2}
                    stroke="var(--card)"
                    isAnimationActive={false}
                  >
                    {parts.map((p) => (
                      <Cell key={p.name} fill={p.color} />
                    ))}
                  </Pie>
                </PieChart>
              </ChartContainer>
              <div className="pointer-events-none absolute inset-0 grid place-items-center text-center">
                <div>
                  <div className="tabular font-heading text-xl font-semibold">
                    {number(all, all < 1 ? 2 : all < 10 ? 1 : 0)}
                  </div>
                  <div className="text-xs text-muted-foreground">kWh</div>
                </div>
              </div>
            </div>
            <ul className="w-full min-w-0 flex-1 divide-y text-sm">
              {parts.map((p) => (
                <li
                  key={p.name}
                  className="flex items-center justify-between gap-3 py-2"
                >
                  <span className="flex min-w-0 items-center gap-2">
                    <span
                      className="size-2.5 shrink-0 rounded-full"
                      style={{ background: p.color }}
                    />
                    <span className="break-words">{p.name}</span>
                  </span>
                  <span className="tabular shrink-0 text-muted-foreground">
                    {percent(p.energy / all)} · {kwh(p.energy)}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        )}
      </CardContent>
    </Card>
  )
}

/** A production or a consumption by its size: its sign only tells how the clamp is fitted. */
function shownPower(d: DeviceView) {
  const p = d.state?.power
  if (typeof p !== "number") return null
  return d.role === "grid" || d.role === "battery" ? p : Math.abs(p)
}

function roleText(
  d: DeviceView,
  devices: DeviceView[],
  rooms: Map<number, string>
) {
  const measured = devices.find((x) => x.id === d.measures.device_id)
  // As the server counts them: an inverter is a production, a meter of an inverter too.
  if (isInverter(d)) {
    const c = inverterCounting(d, devices)
    if (!c.counted)
      return t("Non comptée : « {name} » mesure déjà la production", {
        name: c.overlap.name,
      })
    return c.by
      ? t("Production solaire, comptée par « {name} »", { name: c.by.name })
      : ROLE_LABEL.solar
  }
  if (d.role && isInverter(measured))
    return t("Production de {name}", { name: measured!.name })
  if (!d.role) return t("Non affectée")
  if (d.role !== "consumption") return ROLE_LABEL[d.role]
  const target =
    measured?.name ??
    (d.measures.room_id !== null ? rooms.get(d.measures.room_id) : undefined)
  return target
    ? t("Consommation de {name}", { name: target })
    : t("Autres usages")
}

function Measures({
  devices,
  totals,
  period,
}: {
  devices: DeviceView[]
  totals: DayEnergy
  period: Period
}) {
  const qc = useQueryClient()
  const admin = currentUser()?.role === "admin"
  const rooms = useQuery({ queryKey: ["rooms"], queryFn: api.rooms })
  const roomNames = new Map((rooms.data ?? []).map((r) => [r.id, r.name]))
  const relay = useMutation({
    mutationFn: ({ id, on }: { id: number; on: boolean }) =>
      api.act(id, "switch.relay", "set", { on }),
    onSettled: () => qc.invalidateQueries({ queryKey: ["devices"] }),
  })
  const measures = devices.filter((d) => d.traits.includes("energy.power@1"))
  const phases = measures.find((d) => typeof d.state?.power_a === "number")
  return (
    <Card className="rounded-3xl">
      <CardHeader className="flex flex-row items-start justify-between gap-4">
        <div>
          <CardTitle>{t("Appareils mesurés")}</CardTitle>
          <CardDescription>
            {tn(measures.length, "{n} mesure", "{n} mesures")}
          </CardDescription>
        </div>
        {admin && (
          <Button asChild variant="outline" size="sm">
            <Link to="/settings?tab=modules">{t("Affecter les voies")}</Link>
          </Button>
        )}
      </CardHeader>
      <CardContent className="space-y-4">
        {phases && (
          <div className="grid grid-cols-3 gap-2">
            {(["a", "b", "c"] as const).map((p) => (
              <div key={p} className="rounded-2xl bg-muted/60 px-3 py-2">
                <div className="text-xs text-muted-foreground">
                  {t("Phase {p}", { p: p.toUpperCase() })}
                </div>
                <div className="tabular text-lg font-semibold">
                  {watts(phases.state?.[`power_${p}`] as number)}
                </div>
              </div>
            ))}
          </div>
        )}
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead className="text-left text-xs text-muted-foreground">
              <tr>
                <th className="py-2 pr-3 font-medium">{t("Mesure")}</th>
                <th className="py-2 pr-3 font-medium">{t("Usage")}</th>
                <th className="py-2 pr-3 text-right font-medium">
                  {t("Puissance")}
                </th>
                <th className="py-2 pr-3 text-right font-medium">
                  {period === "day" ? t("Aujourd'hui") : t("Période")}
                </th>
                <th className="py-2 text-right font-medium">{t("Relais")}</th>
              </tr>
            </thead>
            <tbody className="divide-y">
              {measures.map((d) => {
                const e = totals.devices.find((x) => x.device_id === d.id)
                const on = d.state?.on
                return (
                  <tr key={d.id} className={cn(!d.available && "opacity-50")}>
                    <td className="py-2.5 pr-3 font-medium">{d.name}</td>
                    <td className="py-2.5 pr-3 text-muted-foreground">
                      {roleText(d, devices, roomNames)}
                    </td>
                    <td className="tabular py-2.5 pr-3 text-right">
                      {d.available ? watts(shownPower(d)) : "–"}
                    </td>
                    <td className="tabular py-2.5 pr-3 text-right">
                      {e ? kwh(Math.max(e.energy_in, e.energy_out)) : "–"}
                    </td>
                    <td className="py-2.5 text-right">
                      {d.traits.includes("switch.relay@1") &&
                        typeof on === "boolean" && (
                          <Switch
                            checked={on}
                            disabled={!admin || !d.available || relay.isPending}
                            onCheckedChange={(v) =>
                              relay.mutate({ id: d.id, on: v })
                            }
                            aria-label={t("Relais de {name}", { name: d.name })}
                          />
                        )}
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      </CardContent>
    </Card>
  )
}

function NoMeasure() {
  const admin = currentUser()?.role === "admin"
  return (
    <Card className="rounded-3xl">
      <CardContent className="flex flex-col items-center gap-3 py-12 text-center">
        <span className="grid size-12 place-items-center rounded-2xl bg-warning/15 text-warning">
          <Zap className="size-6" />
        </span>
        <div className="font-heading text-lg font-semibold">
          {t("Aucune mesure d'énergie")}
        </div>
        <p className="max-w-md text-sm text-muted-foreground">
          {t(
            "Ajoutez un compteur (Shelly Pro 3EM, Pro EM, prise…) dans Réglages › Modules, puis dites ce que mesure chaque voie : le réseau, la production solaire, ou la consommation d'un appareil."
          )}
        </p>
        {admin && (
          <Button asChild>
            <Link to="/settings?tab=modules">
              {t("Ouvrir Réglages › Modules")}
            </Link>
          </Button>
        )}
      </CardContent>
    </Card>
  )
}

export function EnergyPage() {
  const today = isoDay(new Date())
  const [period, setPeriod] = useState<Period>("day")
  const [date, setDate] = useState(today)
  const live = useQuery({
    queryKey: ["energy-live"],
    queryFn: api.energyLive,
    refetchInterval: 5_000,
  })
  const devices = useQuery({
    queryKey: ["devices"],
    queryFn: api.devices,
    refetchInterval: 5_000,
  })
  const count = period === "year" ? 365 : 30
  const days = useQuery({
    queryKey: ["energy-days", count],
    queryFn: () => api.energyDays(count),
    refetchInterval: 60_000,
  })
  const dayTotals = useQuery({
    queryKey: ["energy-day", date],
    queryFn: () => api.energyDay(date),
    enabled: period === "day",
    refetchInterval: date === today ? 60_000 : false,
  })
  const l = live.data
  const anyRole = l && Object.values(l.roles).some(Boolean)
  const shown = (days.data ?? []).slice(-PERIOD_DAYS[period])
  const totals =
    period === "day"
      ? dayTotals.data?.totals
      : shown.length
        ? total(shown)
        : undefined
  const all = devices.data ?? []
  const measured = all.filter(
    (d) => d.source_id !== null && d.traits.includes("energy.power@1")
  )
  const products = [
    ...new Set(
      measured
        .filter((d) => d.role)
        .map((d) => DRIVER_LABEL[d.source] ?? d.source)
    ),
  ]
  const panels = all.filter((d) => d.traits.includes("solar.panel@1")).length
  const exportCsv = () => {
    if (period === "day" && dayTotals.data)
      downloadCsv(`breezone-energie-${date}.csv`, [
        ["heure", "reseau_w", "solaire_w", "batterie_w", "maison_w"],
        ...dayTotals.data.points.map((p) => [
          time(p.ts),
          p.grid,
          p.solar,
          p.battery,
          p.home,
        ]),
      ])
    else
      downloadCsv(`breezone-energie-${shown[0]?.day ?? today}-${today}.csv`, [
        [
          "jour",
          "consommation_kwh",
          "importe_kwh",
          "injecte_kwh",
          "production_kwh",
          "autoconsomme_kwh",
          "charge_kwh",
          "decharge_kwh",
        ],
        ...shown.map((d) => [
          d.day,
          d.consumption,
          d.imported,
          d.exported,
          d.production,
          d.self_consumed,
          d.charged,
          d.discharged,
        ]),
      ])
  }

  return (
    <>
      <PageHeader
        title={t("Énergie")}
        description={
          measured.length
            ? [
                ...products,
                tn(measured.length, "{n} mesure", "{n} mesures"),
              ].join(" · ")
            : undefined
        }
        actions={
          anyRole && (
            <>
              <ToggleGroup
                type="single"
                variant="outline"
                size="sm"
                value={period}
                onValueChange={(v) => v && setPeriod(v as Period)}
              >
                <ToggleGroupItem value="day">{t("Jour")}</ToggleGroupItem>
                <ToggleGroupItem value="week">{t("7 jours")}</ToggleGroupItem>
                <ToggleGroupItem value="month">{t("30 jours")}</ToggleGroupItem>
                <ToggleGroupItem value="year">{t("Année")}</ToggleGroupItem>
              </ToggleGroup>
              <Button variant="outline" size="sm" onClick={exportCsv}>
                <Download /> {t("Exporter CSV")}
              </Button>
            </>
          )
        }
      />
      {!l ? (
        <Skeleton className="h-96 w-full rounded-3xl" />
      ) : !anyRole ? (
        <NoMeasure />
      ) : (
        <>
          {totals ? (
            <Kpis totals={totals} live={l} />
          ) : (
            <Skeleton className="mb-6 h-24 w-full rounded-3xl" />
          )}
          <FlowCard
            live={l}
            solarSub={
              panels ? tn(panels, "{n} panneau", "{n} panneaux") : undefined
            }
          />
          {period === "day" ? (
            <DayCurve date={date} today={today} onDate={setDate} live={l} />
          ) : null}
          <div className="mb-6 grid gap-4 lg:grid-cols-[minmax(0,1.6fr)_minmax(0,1fr)]">
            {period !== "day" ? (
              <DayBars days={shown} live={l} />
            ) : (
              <DayBars days={(days.data ?? []).slice(-14)} live={l} />
            )}
            {totals && <Breakdown totals={totals} period={period} />}
          </div>
          {totals && (
            <Measures
              devices={devices.data ?? []}
              totals={totals}
              period={period}
            />
          )}
          <p className="mt-6 flex items-center justify-center gap-1.5 text-center text-xs text-muted-foreground">
            <Gauge className="size-3.5" />
            {t(
              "Chaque bloc n'apparaît que si une mesure le permet : la production avec une voie « solaire », l'autonomie avec un compteur général."
            )}
          </p>
        </>
      )}
    </>
  )
}
