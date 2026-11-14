import type { ReactNode } from "react"
import { useQuery } from "@tanstack/react-query"
import {
  Area,
  Bar,
  BarChart,
  CartesianGrid,
  ComposedChart,
  Line,
  XAxis,
  YAxis,
} from "recharts"
import { CalendarDays, Leaf, SolarPanel, Sun, UtilityPole } from "lucide-react"
import type { LucideIcon } from "lucide-react"
import type { DeviceView } from "@/lib/api"
import { api } from "@/lib/api"
import { percent, time } from "@/lib/format"
import { locale, t, tn } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import { PageHeader } from "@/components/app-shell"
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
import { kwh, watts } from "@/pages/energy"

const SUN = "var(--warning)"

function number(v: number, digits: number) {
  return v.toLocaleString(locale(), { maximumFractionDigits: digits })
}

function num(d: DeviceView, key: string) {
  const v = d.state?.[key]
  return typeof v === "number" ? v : null
}

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
  home: { label: t("Consommation"), color: "var(--foreground)" },
} satisfies ChartConfig

/** Today's production, every 15 minutes, beside what the home used. */
function TodayCurve() {
  const day = useQuery({
    queryKey: ["energy-day", "today"],
    queryFn: () => api.energyDay(),
    refetchInterval: 60_000,
  })
  const points = (day.data?.points ?? []).map((p) => ({
    ts: p.ts,
    solar: p.solar === null ? null : p.solar / 1000,
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
          {t("Production et consommation toutes les 15 minutes")}
        </CardDescription>
      </CardHeader>
      <CardContent>
        {day.isLoading ? (
          <Skeleton className="h-64 w-full" />
        ) : !measured ? (
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

const barsConfig = {
  self_consumed: { label: t("Autoconsommée"), color: SUN },
  exported: {
    label: t("Injectée"),
    color: "color-mix(in oklch, var(--warning) 40%, transparent)",
  },
} satisfies ChartConfig

/** What the panels gave each day of the last 30, used on the spot or returned. */
function DaysCard() {
  const days = useQuery({
    queryKey: ["energy-days", 30],
    queryFn: () => api.energyDays(30),
    refetchInterval: 60_000,
  })
  const data = (days.data ?? []).map((d) => ({
    day: d.day,
    self_consumed: d.self_consumed ?? d.production ?? 0,
    exported: d.exported ?? 0,
  }))
  const total = sum((days.data ?? []).map((d) => d.production))
  const label = (day: string, style: "long" | "short") =>
    new Date(`${day}T12:00:00`).toLocaleDateString(
      locale(),
      style === "long"
        ? { weekday: "long", day: "numeric", month: "long" }
        : { day: "2-digit", month: "2-digit" }
    )
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{t("30 derniers jours")}</CardTitle>
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

/** An inverter, and its panels laid out like a roof: the brighter, the more it gives now. */
function InverterCard({
  inverter,
  panels,
}: {
  inverter: DeviceView
  panels: DeviceView[]
}) {
  const peak = Math.max(1, ...panels.map((p) => num(p, "power") ?? 0))
  const facts = [
    num(inverter, "temperature") !== null &&
      `${number(num(inverter, "temperature")!, 1)} °C`,
    num(inverter, "voltage") !== null &&
      `${number(num(inverter, "voltage")!, 1)} V`,
    num(inverter, "frequency") !== null &&
      `${number(num(inverter, "frequency")!, 2)} Hz`,
  ].filter(Boolean)
  return (
    <Card className="rounded-3xl">
      <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-3">
        <div>
          <CardTitle>{inverter.name}</CardTitle>
          <CardDescription>
            {[tn(panels.length, "{n} panneau", "{n} panneaux"), ...facts].join(
              " · "
            )}
          </CardDescription>
        </div>
        <div className="text-right">
          <div
            className="tabular font-heading text-2xl font-semibold"
            style={{ color: SUN }}
          >
            {inverter.available ? watts(num(inverter, "power")) : "–"}
          </div>
          <div className="text-xs text-muted-foreground">
            {t("{kwh} aujourd'hui", {
              kwh: kwh(num(inverter, "energy_today")),
            })}
          </div>
        </div>
      </CardHeader>
      <CardContent className="grid grid-cols-2 gap-3 md:grid-cols-4">
        {panels.map((p) => {
          const power = num(p, "power") ?? 0
          const share = power / peak
          return (
            <div
              key={p.id}
              className={cn(
                "flex flex-col gap-2 rounded-2xl border p-3",
                !p.available && "opacity-50"
              )}
              style={{
                background: `linear-gradient(160deg, color-mix(in oklch, ${SUN} ${Math.round(8 + share * 22)}%, var(--card)), var(--card))`,
                borderColor: `color-mix(in oklch, ${SUN} ${Math.round(15 + share * 35)}%, transparent)`,
              }}
            >
              <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
                <span className="truncate">{p.name}</span>
                <SolarPanel
                  className="size-3.5 shrink-0"
                  style={{ color: SUN }}
                />
              </div>
              <div className="tabular font-heading text-xl font-semibold">
                {p.available ? watts(power) : "–"}
              </div>
              <div className="h-1.5 overflow-hidden rounded-full bg-muted">
                <div
                  className="h-full rounded-full"
                  style={{ width: `${share * 100}%`, background: SUN }}
                />
              </div>
              <div className="tabular grid gap-0.5 text-xs text-muted-foreground">
                <span>
                  {[
                    num(p, "voltage") !== null &&
                      `${number(num(p, "voltage")!, 1)} V`,
                    num(p, "current") !== null &&
                      `${number(num(p, "current")!, 2)} A`,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </span>
                <span>
                  {t("{kwh} aujourd'hui", { kwh: kwh(num(p, "energy_today")) })}
                </span>
                <span>
                  {t("{kwh} au total", { kwh: kwh(num(p, "energy_total")) })}
                </span>
              </div>
            </div>
          )
        })}
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
  const all = devices.data ?? []
  const inverters = all.filter((d) => d.traits.includes("solar.inverter@1"))
  const panels = all.filter((d) => d.traits.includes("solar.panel@1"))
  const panelsOf = (inv: DeviceView) =>
    panels.filter(
      (p) =>
        p.source_id === inv.source_id &&
        p.state?.inverter === String(inv.reference).replace(/^inv:/, "")
    )
  // The production as the energy page counts it (a meter on the line, else the inverters).
  const now = live.data?.solar ?? sum(inverters.map((i) => num(i, "power")))
  const totals = today.data?.totals
  // The inverters count their own production since sunrise: better than the history.
  const counted = sum(inverters.map((i) => num(i, "energy_today")))
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
          {inverters.map((inv) => (
            <InverterCard key={inv.id} inverter={inv} panels={panelsOf(inv)} />
          ))}
          <div className="grid gap-4 lg:grid-cols-2">
            <TodayCurve />
            <DaysCard />
          </div>
        </div>
      )}
    </>
  )
}
