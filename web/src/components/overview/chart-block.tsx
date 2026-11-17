import { useState } from "react"
import { useQuery } from "@tanstack/react-query"
import { Bar, CartesianGrid, ComposedChart, Line, XAxis, YAxis } from "recharts"
import { api } from "@/lib/api"
import { dateTime, formatTemp, number, percent, time } from "@/lib/format"
import { locale, t } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import { DailyComfort } from "@/components/daily-comfort"
import { Card, CardContent } from "@/components/ui/card"
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
} from "@/components/ui/chart"
import type { ChartConfig } from "@/components/ui/chart"
import { useOverview } from "./context"
import { chartFields, OUTDOOR, zoneOf } from "./model"

/** How a known measure is drawn (as on the history of a zone); the others take the palette. */
const LOOK: Record<
  string,
  { color: string; dashed?: boolean; step?: boolean }
> = {
  room_temp: { color: "var(--heat)" },
  temperature: { color: "var(--heat)" },
  setpoint: { color: "var(--primary)", dashed: true, step: true },
  humidity: { color: "var(--cool)" },
  [OUTDOOR]: { color: "var(--warning)", dashed: true },
}
const PALETTE = [1, 2, 3, 4, 5].map((i) => `var(--chart-${i})`)

/** A curve of the chart: what it draws, how, and on which axis. */
type Curve = {
  /** Its key in the points of the chart (`s0`, `s1`…): a field name may not be a CSS name. */
  key: string
  field: string
  label: string
  unit: string
  color: string
  dashed?: boolean
  step?: boolean
}

const suffix = (unit: string) =>
  unit === "°C" ? "°" : unit === "%" ? " %" : unit ? ` ${unit}` : ""
const show = (v: number, unit: string) =>
  `${number(v, Math.abs(v) < 10 ? 1 : 0)}${suffix(unit)}`

/** A chart of a device: several of its measures (and the outdoor temperature), or its comfort. */
export function ChartBlock({
  id,
  fields,
  hours,
  view,
  tall,
}: {
  id: number
  fields: string[]
  hours: number
  view: "curves" | "comfort"
  /** The full height of the charts of its page, instead of a compact one. */
  tall: boolean
}) {
  const { devices } = useOverview()
  const d = devices.find((x) => x.id === id)
  const zone = d ? zoneOf(d) : undefined
  if (view === "comfort" && d && zone)
    return tall ? (
      <DailyComfort
        zone={zone}
        title={t("Confort · {name}", { name: d.name })}
        className="h-full"
      />
    ) : (
      <CompactComfort name={d.name} zone={zone} />
    )
  return <Curves id={id} fields={fields} hours={hours} tall={tall} />
}

function Curves({
  id,
  fields,
  hours,
  tall,
}: {
  id: number
  fields: string[]
  hours: number
  tall: boolean
}) {
  const { devices, traits, installation } = useOverview()
  const [hidden, setHidden] = useState<ReadonlySet<string>>(new Set())
  const d = devices.find((x) => x.id === id)
  const own = fields.filter((f) => f !== OUTDOOR)
  const outdoorWanted = fields.includes(OUTDOOR)
  const history = useQuery({
    queryKey: ["device-history", id, hours],
    queryFn: () => api.deviceHistory(id, hours, hours > 24 ? 168 : 96),
    enabled: d !== undefined && own.length > 0,
    refetchInterval: 5 * 60_000,
  })
  const outdoor = useQuery({
    queryKey: ["outdoor", installation.id, String(hours)],
    queryFn: () => api.outdoorHistory(hours, installation.id),
    enabled: outdoorWanted,
    staleTime: 10 * 60_000,
  })
  const infos = d ? chartFields(d, traits) : []
  const curves: Curve[] = fields.map((field, i) => {
    const info = infos.find((f) => f.name === field)
    const look = LOOK[field] ?? { color: PALETTE[i % PALETTE.length] }
    return {
      key: `s${i}`,
      field,
      label: field === OUTDOOR ? t("Extérieur") : (info?.label ?? field),
      unit: field === OUTDOOR ? "°C" : (info?.unit ?? ""),
      ...look,
    }
  })
  const pointsOf = (field: string) =>
    field === OUTDOOR
      ? (outdoor.data ?? [])
      : (history.data?.series[field] ?? [])
  // One row per time any curve has a point: the others join over it.
  const rows = new Map<number, Record<string, number>>()
  for (const c of curves)
    for (const [ts, v] of pointsOf(c.field)) {
      const row = rows.get(ts) ?? { ts }
      row[c.key] = v
      rows.set(ts, row)
    }
  const data = [...rows.values()].sort((a, b) => a.ts - b.ts)
  // The first unit on the left, the second on the right, any other on an axis of its own.
  const units = [...new Set(curves.map((c) => c.unit))]
  const config = Object.fromEntries(
    curves.map((c) => [c.key, { label: c.label, color: c.color }])
  ) satisfies ChartConfig
  const last = (field: string) => pointsOf(field).at(-1)?.[1]
  const loading = history.isLoading || outdoor.isLoading
  const toggle = (key: string) =>
    setHidden((h) => {
      const next = new Set(h)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })
  const tick = (ts: number) =>
    hours > 24
      ? new Date(ts * 1000).toLocaleDateString(locale(), { weekday: "short" })
      : time(ts)

  return (
    <Card className="h-full rounded-3xl">
      <CardContent className="flex flex-col gap-2">
        <div className="flex items-baseline justify-between gap-3">
          <h3 className="truncate font-heading text-lg font-semibold">
            {d?.name ?? t("Appareil indisponible")}
          </h3>
          <span className="shrink-0 text-xs text-muted-foreground">
            {hours > 24 ? t("7 derniers jours") : t("24 dernières heures")}
          </span>
        </div>
        {data.length > 1 ? (
          <ChartContainer
            config={config}
            className={cn("aspect-auto w-full", tall ? "h-72" : "h-32")}
          >
            <ComposedChart data={data} margin={{ left: 0, right: 4, top: 4 }}>
              <CartesianGrid vertical={false} />
              <XAxis
                dataKey="ts"
                type="number"
                scale="time"
                domain={["dataMin", "dataMax"]}
                tickFormatter={tick}
                tickLine={false}
                axisLine={false}
                minTickGap={40}
              />
              {units.map((unit, i) => (
                <YAxis
                  key={unit}
                  yAxisId={unit}
                  orientation={i === 1 ? "right" : "left"}
                  hide={i > 1}
                  width={36}
                  tickLine={false}
                  axisLine={false}
                  domain={
                    unit === "%" ? [0, 100] : ["dataMin - 1", "dataMax + 1"]
                  }
                  tickCount={3}
                  allowDecimals={false}
                  tickFormatter={(v: number) =>
                    `${Math.round(v)}${suffix(unit).trim()}`
                  }
                />
              ))}
              <ChartTooltip
                content={
                  <ChartTooltipContent
                    labelFormatter={(_, p) =>
                      p?.[0] ? dateTime(Number(p[0].payload.ts)) : ""
                    }
                    formatter={(value, name, item) => {
                      const c = curves.find((x) => x.key === name)
                      if (!c) return null
                      return (
                        <div className="flex w-full items-center justify-between gap-4">
                          <span className="flex items-center gap-1.5 text-muted-foreground">
                            <span
                              className="size-2 rounded-[2px]"
                              style={{ background: item.color }}
                            />
                            {c.label}
                          </span>
                          <span className="tabular font-medium">
                            {show(Number(value), c.unit)}
                          </span>
                        </div>
                      )
                    }}
                  />
                }
              />
              {curves.map((c) => (
                <Line
                  key={c.key}
                  yAxisId={c.unit}
                  dataKey={c.key}
                  type={c.step ? "stepAfter" : "monotone"}
                  stroke={`var(--color-${c.key})`}
                  strokeWidth={c.dashed ? 1.5 : 2}
                  strokeDasharray={c.dashed ? "5 4" : undefined}
                  dot={false}
                  connectNulls
                  isAnimationActive={false}
                  hide={hidden.has(c.key)}
                />
              ))}
            </ComposedChart>
          </ChartContainer>
        ) : (
          <p
            className={cn(
              "grid place-items-center",
              tall ? "h-72" : "h-32",
              "text-sm text-muted-foreground"
            )}
          >
            {loading ? "…" : t("Pas encore d'historique.")}
          </p>
        )}
        <div className="flex flex-wrap gap-1.5">
          {curves.map((c) => {
            const v = last(c.field)
            return (
              <button
                key={c.key}
                type="button"
                onClick={() => toggle(c.key)}
                aria-pressed={!hidden.has(c.key)}
                className={cn(
                  "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs transition-opacity",
                  hidden.has(c.key) && "opacity-40"
                )}
              >
                <span
                  className="size-2.5 rounded-sm"
                  style={{ background: c.color }}
                />
                {c.label}
                {v !== undefined && (
                  <span className="tabular font-medium">{show(v, c.unit)}</span>
                )}
              </button>
            )
          })}
        </div>
      </CardContent>
    </Card>
  )
}

const comfortConfig = {
  comfort_pct: { label: t("Confort"), color: "var(--primary)" },
  avg_temp: { label: t("Température moyenne"), color: "var(--heat)" },
} satisfies ChartConfig

/** The comfort of a zone over a week, in one small chart: its share per day, and the mean temperature. */
function CompactComfort({
  name,
  zone,
}: {
  name: string
  zone: { installation_id: number; system_id: number; zone_id: number }
}) {
  const daily = useQuery({
    queryKey: [
      "daily",
      zone.installation_id,
      zone.system_id,
      zone.zone_id,
      "7",
    ],
    queryFn: () => api.daily(zone, 7),
    refetchInterval: 300_000,
  })
  const stats = daily.data ?? []
  const data = stats.map((d) => ({
    label: new Date(`${d.day}T12:00:00`).toLocaleDateString(locale(), {
      weekday: "short",
    }),
    comfort_pct: d.comfort === null ? null : Math.round(d.comfort * 100),
    avg_temp: d.avg_temp,
  }))
  const on = stats.reduce((s, d) => s + d.on_secs, 0)
  const comfortable = stats.reduce((s, d) => s + d.comfort_secs, 0)
  return (
    <Card className="h-full rounded-3xl">
      <CardContent className="flex flex-col gap-2">
        <div className="flex items-baseline justify-between gap-3">
          <h3 className="truncate font-heading text-lg font-semibold">
            {t("Confort · {name}", { name })}
          </h3>
          <span className="shrink-0 text-xs text-muted-foreground">
            {t("7 derniers jours")}
          </span>
        </div>
        {data.length === 0 ? (
          <p className="grid h-32 place-items-center text-sm text-muted-foreground">
            {daily.isLoading ? "…" : t("Pas encore d'historique.")}
          </p>
        ) : (
          <ChartContainer
            config={comfortConfig}
            className="aspect-auto h-32 w-full"
          >
            <ComposedChart data={data} margin={{ left: 0, right: 4, top: 4 }}>
              <CartesianGrid vertical={false} />
              <XAxis
                dataKey="label"
                tickLine={false}
                axisLine={false}
                minTickGap={8}
              />
              <YAxis
                yAxisId="t"
                width={36}
                tickLine={false}
                axisLine={false}
                domain={["dataMin - 1", "dataMax + 1"]}
                tickCount={3}
                allowDecimals={false}
                tickFormatter={(v: number) => `${Math.round(v)}°`}
              />
              <YAxis
                yAxisId="pct"
                orientation="right"
                width={36}
                tickLine={false}
                axisLine={false}
                domain={[0, 100]}
                ticks={[0, 50, 100]}
                tickFormatter={(v: number) => `${v}%`}
              />
              <ChartTooltip
                content={
                  <ChartTooltipContent
                    formatter={(value, name) => (
                      <div className="flex w-full justify-between gap-4">
                        <span className="text-muted-foreground">
                          {name === "comfort_pct"
                            ? t("Confort")
                            : t("Température moyenne")}
                        </span>
                        <span className="tabular font-medium">
                          {name === "comfort_pct"
                            ? percent(Number(value) / 100)
                            : formatTemp(Number(value))}
                        </span>
                      </div>
                    )}
                  />
                }
              />
              <Bar
                yAxisId="pct"
                dataKey="comfort_pct"
                fill="var(--color-comfort_pct)"
                fillOpacity={0.5}
                radius={4}
                maxBarSize={20}
              />
              <Line
                yAxisId="t"
                dataKey="avg_temp"
                type="monotone"
                stroke="var(--color-avg_temp)"
                strokeWidth={2}
                dot={{ r: 2.5 }}
                isAnimationActive={false}
              />
            </ComposedChart>
          </ChartContainer>
        )}
        <div className="flex flex-wrap gap-1.5 text-xs">
          <span className="inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1">
            <span
              className="size-2.5 rounded-sm opacity-60"
              style={{ background: "var(--primary)" }}
            />
            {t("Confort")}
            <span className="tabular font-medium">
              {on > 0 ? percent(comfortable / on) : "–"}
            </span>
          </span>
          <span className="inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1">
            <span
              className="size-2.5 rounded-sm"
              style={{ background: "var(--heat)" }}
            />
            {t("Température moyenne")}
          </span>
        </div>
      </CardContent>
    </Card>
  )
}
