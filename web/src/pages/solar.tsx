import { useState } from "react"
import type { ReactNode } from "react"
import { useQuery } from "@tanstack/react-query"
import {
  Area,
  Bar,
  BarChart,
  CartesianGrid,
  ComposedChart,
  Line,
  ReferenceLine,
  XAxis,
  YAxis,
} from "recharts"
import { CalendarDays, CloudSun, Leaf, Sun, UtilityPole } from "lucide-react"
import type { LucideIcon } from "lucide-react"
import { api } from "@/lib/api"
import { forecastAt, useSolarForecast } from "@/lib/forecast"
import { SurplusProgramme } from "@/components/surplus"
import { kwh, percent, stateNumber, time, watts } from "@/lib/format"
import { locale, t, tn } from "@/lib/i18n"
import { PageHeader } from "@/components/app-shell"
import { PeriodPicker } from "@/components/period-picker"
import type { HistoryPeriod } from "@/lib/periods"
import { PERIOD_LABEL, periodsFor } from "@/lib/periods"
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
import { SolarPanels } from "@/components/solar-panels"

const SUN = "var(--warning)"

function sum(values: (number | null)[]) {
  return values.reduce<number>((a, v) => a + (v ?? 0), 0)
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

const curveConfig = {
  solar: { label: t("Production"), color: SUN },
  forecast: { label: t("Prévue"), color: SUN },
  home: { label: t("Consommation"), color: "var(--foreground)" },
} satisfies ChartConfig

/** Today's production, every 15 minutes, beside what the home used and what was forecast. */
export function TodayCurve() {
  const day = useQuery({
    queryKey: ["energy-day", "today"],
    queryFn: () => api.energyDay(),
    refetchInterval: 60_000,
  })
  const forecast = useSolarForecast()
  const forecasts = forecast.power.length > 0
  const points = (day.data?.points ?? []).map((p) => ({
    ts: p.ts,
    solar: p.solar === null ? null : p.solar / 1000,
    forecast: forecasts ? forecastAt(forecast.power, p.ts) / 1000 : null,
    home: p.home === null ? null : p.home / 1000,
  }))
  const measured = points.some((p) => p.solar !== null)
  const from = points[0]?.ts ?? 0
  const until = from + (day.data ? day.data.bucket * points.length : 86_400)
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{t("Aujourd'hui")}</CardTitle>
        <CardDescription>
          {forecasts
            ? t("Production, prévision et consommation toutes les 15 minutes")
            : t("Production et consommation toutes les 15 minutes")}
        </CardDescription>
      </CardHeader>
      <CardContent>
        {day.isLoading ? (
          <Skeleton className="h-64 w-full" />
        ) : !measured && !forecasts ? (
          <div className="flex h-64 items-center justify-center text-center text-sm text-muted-foreground">
            {t(
              "Pas encore de relevés ce jour-là : ils s'accumulent toutes les 5 minutes."
            )}
          </div>
        ) : (
          <ChartContainer
            config={curveConfig}
            className="aspect-auto h-64 w-full"
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
                tickFormatter={(v: number) => watts(v * 1000)}
              />
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
              <Area
                dataKey="solar"
                type="monotone"
                stroke="var(--color-solar)"
                fill="var(--color-solar)"
                fillOpacity={0.22}
                strokeWidth={2}
                connectNulls={false}
                isAnimationActive={false}
              />
              {forecasts && (
                <Line
                  dataKey="forecast"
                  type="monotone"
                  stroke="var(--color-forecast)"
                  strokeWidth={1.5}
                  strokeDasharray="2 3"
                  dot={false}
                  isAnimationActive={false}
                />
              )}
              <Line
                dataKey="home"
                type="monotone"
                stroke="var(--color-home)"
                strokeWidth={1.5}
                strokeDasharray="4 3"
                dot={false}
                connectNulls={false}
                isAnimationActive={false}
              />
            </ComposedChart>
          </ChartContainer>
        )}
      </CardContent>
    </Card>
  )
}

const forecastConfig = {
  power: { label: t("Prévue"), color: SUN },
} satisfies ChartConfig

function Figure({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <div className="truncate text-xs text-muted-foreground">{label}</div>
      <div className="tabular font-heading text-lg font-semibold">{value}</div>
    </div>
  )
}

/** What the panels should give today and tomorrow (Forecast.Solar), and its curve over both days. */
export function ForecastCard() {
  const f = useSolarForecast()
  const start = new Date()
  start.setHours(0, 0, 0, 0)
  const from = start.getTime() / 1000
  const noon = from + 12 * 3600
  const tomorrow = new Date(start)
  tomorrow.setDate(start.getDate() + 1)
  const next = tomorrow.getTime() / 1000
  const until = next + 86_400
  const data = f.power
    .filter(([ts]) => ts >= from && ts <= until)
    .map(([ts, w]) => ({ ts, power: w / 1000 }))
  const tick = (ts: number) =>
    ts === from ? t("aujourd'hui") : ts === next ? t("demain") : time(ts)
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <CloudSun className="size-4" style={{ color: SUN }} />
          {t("Prévision solaire")}
        </CardTitle>
        <CardDescription>
          {t("Selon la météo, l'orientation et la puissance des panneaux")}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {f.loading ? (
          <Skeleton className="h-48 w-full" />
        ) : !f.device ? (
          <p className="text-sm text-muted-foreground">
            {t(
              "Réglez Forecast.Solar dans Réglages › Comptes et services pour voir la production attendue."
            )}
          </p>
        ) : (
          <>
            <div className="grid grid-cols-3 gap-3">
              <Figure label={t("Aujourd'hui")} value={kwh(f.today)} />
              <Figure label={t("Reste à produire")} value={kwh(f.remaining)} />
              <Figure label={t("Demain")} value={kwh(f.tomorrow)} />
            </div>
            {data.length > 0 && (
              <ChartContainer
                config={forecastConfig}
                className="aspect-auto h-40 w-full"
              >
                <ComposedChart data={data} margin={{ left: 0, right: 8 }}>
                  <CartesianGrid vertical={false} />
                  <XAxis
                    dataKey="ts"
                    type="number"
                    domain={[from, until]}
                    scale="time"
                    ticks={[from, noon, next, next + 12 * 3600]}
                    tickFormatter={tick}
                    tickLine={false}
                    axisLine={false}
                  />
                  <YAxis
                    tickLine={false}
                    axisLine={false}
                    width={56}
                    tickFormatter={(v: number) => watts(v * 1000)}
                  />
                  <ChartTooltip
                    content={
                      <ChartTooltipContent
                        labelFormatter={(_, p) => {
                          const ts = Number(p?.[0]?.payload.ts ?? 0)
                          return `${ts >= next ? t("Demain") : t("Aujourd'hui")} ${time(ts)}`
                        }}
                        formatter={(value) => (
                          <div className="flex w-full items-center justify-between gap-4">
                            <span className="text-muted-foreground">
                              {t("Prévue")}
                            </span>
                            <span className="tabular font-medium">
                              {watts(Number(value) * 1000)}
                            </span>
                          </div>
                        )}
                      />
                    }
                  />
                  <ReferenceLine
                    x={f.readAt}
                    stroke="var(--muted-foreground)"
                    strokeDasharray="3 3"
                  />
                  <Area
                    dataKey="power"
                    type="monotone"
                    stroke="var(--color-power)"
                    fill="var(--color-power)"
                    fillOpacity={0.18}
                    strokeWidth={1.5}
                    strokeDasharray="2 3"
                    isAnimationActive={false}
                  />
                </ComposedChart>
              </ChartContainer>
            )}
          </>
        )}
      </CardContent>
    </Card>
  )
}

const barsConfig = {
  self_consumed: { label: t("Autoconsommée"), color: SUN },
  exported: {
    label: t("Injectée"),
    color: "color-mix(in oklch, var(--warning) 40%, transparent)",
  },
} satisfies ChartConfig

/** What the panels gave each day (or month), used on the spot or returned: 30 days to 5 years. */
export function DaysCard() {
  const [period, setPeriod] = useState<HistoryPeriod>("30d")
  const count = period === "5y" ? 5 * 365 + 1 : period === "1y" ? 365 : 30
  const days = useQuery({
    queryKey: ["energy-days", count],
    queryFn: () => api.energyDays(count),
    refetchInterval: 60_000,
  })
  const range = useQuery({
    queryKey: ["energy-range"],
    queryFn: api.energyRange,
    staleTime: 3_600_000,
  })
  const first = range.data?.first_day
  const periods = periodsFor(
    first ? new Date(`${first}T00:00:00`).getTime() / 1000 : null,
    range.dataUpdatedAt / 1000,
    ["30d", "1y", "5y"]
  )
  const shown = (days.data ?? []).filter((d) => !first || d.day >= first)
  // Over a year and more: one bar a month.
  const monthly = period !== "30d"
  const rows = monthly
    ? Object.values(
        shown.reduce<
          Record<
            string,
            { day: string; self: number; out: number; prod: number }
          >
        >((m, d) => {
          const k = d.day.slice(0, 7)
          const r = (m[k] ??= { day: k, self: 0, out: 0, prod: 0 })
          r.self += d.self_consumed ?? d.production ?? 0
          r.out += d.exported ?? 0
          r.prod += d.production ?? 0
          return m
        }, {})
      )
    : shown.map((d) => ({
        day: d.day,
        self: d.self_consumed ?? d.production ?? 0,
        out: d.exported ?? 0,
        prod: d.production ?? 0,
      }))
  const data = rows.map((r) => ({
    day: r.day,
    self_consumed: r.self,
    exported: r.out,
  }))
  const total = sum(rows.map((r) => r.prod))
  const label = (day: string, style: "long" | "short") =>
    day.length === 7
      ? new Date(`${day}-15T12:00:00`).toLocaleDateString(
          locale(),
          style === "long"
            ? { month: "long", year: "numeric" }
            : { month: "short", year: "2-digit" }
        )
      : new Date(`${day}T12:00:00`).toLocaleDateString(
          locale(),
          style === "long"
            ? { weekday: "long", day: "numeric", month: "long" }
            : { day: "2-digit", month: "2-digit" }
        )
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <div className="flex flex-wrap items-start justify-between gap-2">
          <CardTitle>
            {period === "30d"
              ? t("30 derniers jours")
              : t("Par mois, sur {period}", { period: PERIOD_LABEL[period] })}
          </CardTitle>
          <PeriodPicker value={period} periods={periods} onChange={setPeriod} />
        </div>
        <CardDescription>
          {t("{kwh} produits · autoconsommés ou injectés", {
            kwh: kwh(total),
          })}
        </CardDescription>
      </CardHeader>
      <CardContent>
        {days.isLoading ? (
          <Skeleton className="h-56 w-full" />
        ) : (
          <ChartContainer
            config={barsConfig}
            className="aspect-auto h-56 w-full"
          >
            <BarChart data={data} margin={{ left: 0, right: 8 }}>
              <CartesianGrid vertical={false} />
              <XAxis
                dataKey="day"
                tickFormatter={(d: string) => label(d, "short")}
                tickLine={false}
                axisLine={false}
                minTickGap={16}
              />
              <YAxis
                tickLine={false}
                axisLine={false}
                width={64}
                tickFormatter={(v: number) => kwh(v)}
              />
              <ChartTooltip
                content={
                  <ChartTooltipContent
                    labelFormatter={(d) => label(String(d), "long")}
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
              <Bar
                dataKey="self_consumed"
                stackId="p"
                fill="var(--color-self_consumed)"
                isAnimationActive={false}
              />
              <Bar
                dataKey="exported"
                stackId="p"
                fill="var(--color-exported)"
                radius={[6, 6, 0, 0]}
                isAnimationActive={false}
              />
            </BarChart>
          </ChartContainer>
        )}
      </CardContent>
    </Card>
  )
}

export function SolarPage() {
  const devices = useQuery({
    queryKey: ["devices"],
    queryFn: api.devices,
    refetchInterval: 5_000,
  })
  const live = useQuery({
    queryKey: ["energy-live"],
    queryFn: api.energyLive,
    refetchInterval: 5_000,
  })
  const today = useQuery({
    queryKey: ["energy-day", "today"],
    queryFn: () => api.energyDay(),
    refetchInterval: 60_000,
  })
  const forecast = useSolarForecast()
  const all = devices.data ?? []
  const inverters = all.filter((d) => d.traits.includes("solar.inverter@1"))
  const panels = all.filter((d) => d.traits.includes("solar.panel@1"))
  const now0 = new Date()
  const day = `${now0.getFullYear()}-${String(now0.getMonth() + 1).padStart(2, "0")}-${String(now0.getDate()).padStart(2, "0")}`
  // The production as the energy page counts it (a meter on the line, else the inverters).
  const now =
    live.data?.solar ?? sum(inverters.map((i) => stateNumber(i, "power")))
  const totals = today.data?.totals
  // The inverters count their own production since sunrise: better than the history.
  const counted = sum(inverters.map((i) => stateNumber(i, "energy_today")))
  const produced = counted > 0 ? counted : (totals?.production ?? null)
  const selfShare =
    totals?.production && totals.self_consumed !== null
      ? totals.self_consumed / totals.production
      : null

  return (
    <>
      <PageHeader
        title={t("Solaire")}
        description={
          inverters.length
            ? [
                tn(
                  inverters.length,
                  "{n} micro-onduleur",
                  "{n} micro-onduleurs"
                ),
                tn(panels.length, "{n} panneau", "{n} panneaux"),
              ].join(" · ")
            : undefined
        }
      />
      {devices.isLoading ? (
        <Skeleton className="h-96 w-full rounded-3xl" />
      ) : inverters.length === 0 && forecast.device ? (
        <div className="grid gap-6">
          <ForecastCard />
          <TodayCurve />
          <SurplusProgramme />
        </div>
      ) : inverters.length === 0 ? (
        <Card className="rounded-3xl">
          <CardContent className="flex flex-col items-center gap-3 py-12 text-center">
            <span className="grid size-12 place-items-center rounded-2xl bg-warning/15 text-warning">
              <Sun className="size-6" />
            </span>
            <div className="font-heading text-lg font-semibold">
              {t("Aucun onduleur solaire")}
            </div>
            <p className="max-w-md text-sm text-muted-foreground">
              {t(
                "Ajoutez la clé Wi-Fi de vos micro-onduleurs (Hoymiles) dans Réglages › Modules : Breezone la trouve sur le réseau."
              )}
            </p>
          </CardContent>
        </Card>
      ) : (
        <div className="grid gap-6">
          <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
            <Kpi
              icon={Sun}
              label={t("En ce moment")}
              value={watts(now)}
              color={SUN}
            />
            <Kpi
              icon={CalendarDays}
              label={t("Aujourd'hui")}
              value={kwh(produced)}
              sub={
                forecast.today !== null
                  ? t("sur {kwh} prévus", { kwh: kwh(forecast.today) })
                  : undefined
              }
              color={SUN}
            />
            <Kpi
              icon={Leaf}
              label={t("Autoconsommation")}
              value={selfShare === null ? "–" : percent(selfShare)}
              sub={t("de la production utilisée sur place")}
              color="var(--primary)"
            />
            <Kpi
              icon={UtilityPole}
              label={t("Injectée")}
              value={kwh(totals?.exported ?? null)}
              sub={t("rendue au réseau aujourd'hui")}
              color="var(--cool)"
            />
          </div>
          <SolarPanels devices={all} today={day} withCurve />
          <div className="grid gap-4 lg:grid-cols-2">
            <TodayCurve />
            <DaysCard />
          </div>
          {forecast.device && <ForecastCard />}
          <SurplusProgramme />
        </div>
      )}
    </>
  )
}
