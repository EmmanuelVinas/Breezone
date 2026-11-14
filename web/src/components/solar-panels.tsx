import { useState } from "react"
import { useQueries } from "@tanstack/react-query"
import {
  Area,
  CartesianGrid,
  ComposedChart,
  Line,
  ReferenceLine,
  XAxis,
  YAxis,
} from "recharts"
import { SolarPanel } from "lucide-react"
import type { DeviceView } from "@/lib/api"
import { api } from "@/lib/api"
import { time } from "@/lib/format"
import { locale, t, tn } from "@/lib/i18n"
import { cn } from "@/lib/utils"
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

const SUN = "var(--warning)"

/** One color per panel, the same in the tiles and the curves. */
const PANEL_COLORS = [
  "var(--chart-1)",
  "var(--chart-2)",
  "var(--chart-3)",
  "var(--chart-4)",
  "var(--primary)",
  "var(--heat)",
  "var(--success)",
  "var(--fan)",
]

function number(v: number, digits: number) {
  return v.toLocaleString(locale(), { maximumFractionDigits: digits })
}

function watts(v: number | null | undefined) {
  if (v === null || v === undefined) return "–"
  return Math.abs(v) >= 1000
    ? `${number(v / 1000, 2)} kW`
    : `${Math.round(v)} W`
}

function kwh(v: number | null | undefined) {
  if (v === null || v === undefined) return "–"
  return `${number(Math.abs(v) < 0.005 ? 0 : v, Math.abs(v) < 10 ? 2 : 1)} kWh`
}

function num(d: DeviceView, key: string) {
  const v = d.state?.[key]
  return typeof v === "number" ? v : null
}

/** The panels of an inverter: their reference ("pv:<serial>:<port>") names it, even unread. */
function panelsOf(inverter: DeviceView, panels: DeviceView[]) {
  const serial = String(inverter.reference).replace(/^inv:/, "")
  return panels.filter(
    (p) =>
      p.source_id === inverter.source_id &&
      String(p.reference).startsWith(`pv:${serial}:`)
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
        {panels.map((p, i) => {
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
                  style={{ color: PANEL_COLORS[i % PANEL_COLORS.length] }}
                />
              </div>
              <div className="tabular font-heading text-xl font-semibold">
                {p.available ? watts(power) : "–"}
              </div>
              <div className="h-1.5 overflow-hidden rounded-full bg-muted">
                <div
                  className="h-full rounded-full"
                  style={{
                    width: `${share * 100}%`,
                    background: PANEL_COLORS[i % PANEL_COLORS.length],
                  }}
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

/** Midnight of a local day ("YYYY-MM-DD"), and of the next one, as timestamps. */
function dayBounds(day: string) {
  const start = new Date(`${day}T00:00:00`)
  const end = new Date(start)
  end.setDate(end.getDate() + 1)
  return [start.getTime() / 1000, end.getTime() / 1000] as const
}

const BUCKET = 900

/** The power of each panel along a local day, a point every 15 minutes. */
function PanelsCurve({
  panels,
  date,
  today,
}: {
  panels: DeviceView[]
  date: string
  today: string
}) {
  const [from, until] = dayBounds(date)
  const [hidden, setHidden] = useState<ReadonlySet<number>>(new Set())
  const histories = useQueries({
    queries: panels.map((p) => ({
      queryKey: ["device-history-day", p.id, date],
      queryFn: () => api.deviceHistoryBetween(p.id, from, until, BUCKET),
      refetchInterval: date === today ? 60_000 : false,
    })),
  })
  const loading = histories.some((h) => h.isLoading)
  const rows = new Map<number, Record<string, number>>()
  histories.forEach((h, i) => {
    for (const [ts, w] of h.data?.series.power ?? []) {
      const row = rows.get(ts) ?? { ts }
      row[`p${panels[i].id}`] = w / 1000
      rows.set(ts, row)
    }
  })
  const data = [...rows.values()]
    .sort((a, b) => a.ts - b.ts)
    .map((r) => ({
      ...r,
      total: panels.reduce((a, p) => a + (r[`p${p.id}`] ?? 0), 0),
    }))
  const config: ChartConfig = {
    total: { label: t("Total des panneaux"), color: SUN },
    ...Object.fromEntries(
      panels.map((p, i) => [
        `p${p.id}`,
        { label: p.name, color: PANEL_COLORS[i % PANEL_COLORS.length] },
      ])
    ),
  }
  const latest = histories.reduce(
    (a, h) => Math.max(a, h.dataUpdatedAt / 1000),
    0
  )
  const toggle = (id: number) =>
    setHidden((h) => {
      const next = new Set(h)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{t("Production par panneau")}</CardTitle>
        <CardDescription>
          {t("Puissance de chaque panneau toutes les 15 minutes")}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex flex-wrap gap-2">
          {panels.map((p, i) => (
            <button
              key={p.id}
              type="button"
              onClick={() => toggle(p.id)}
              aria-pressed={!hidden.has(p.id)}
              className={cn(
                "inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-xs transition-opacity",
                hidden.has(p.id) && "opacity-40"
              )}
            >
              <span
                className="size-2.5 rounded-sm"
                style={{ background: PANEL_COLORS[i % PANEL_COLORS.length] }}
              />
              {p.name}
            </button>
          ))}
          <span className="inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-xs">
            <span
              className="size-2.5 rounded-sm opacity-40"
              style={{ background: SUN }}
            />
            {t("Total des panneaux")}
          </span>
        </div>
        {loading ? (
          <Skeleton className="h-72 w-full" />
        ) : data.length === 0 ? (
          <div className="flex h-72 items-center justify-center text-center text-sm text-muted-foreground">
            {t(
              "Pas encore de relevés ce jour-là : ils s'accumulent toutes les 5 minutes."
            )}
          </div>
        ) : (
          <ChartContainer config={config} className="aspect-auto h-72 w-full">
            <ComposedChart data={data} margin={{ left: 0, right: 8 }}>
              <CartesianGrid vertical={false} />
              <XAxis
                dataKey="ts"
                type="number"
                domain={[from, until]}
                scale="time"
                ticks={Array.from({ length: 9 }, (_, i) => from + i * 3 * 3600)}
                tickFormatter={(ts: number) => time(ts)}
                tickLine={false}
                axisLine={false}
              />
              <YAxis
                tickLine={false}
                axisLine={false}
                width={68}
                tickFormatter={(v: number) => watts(v * 1000)}
              />
              {date === today && latest > from && latest < until && (
                <ReferenceLine
                  x={latest}
                  stroke="var(--muted-foreground)"
                  strokeDasharray="3 3"
                  label={{
                    value: t("maintenant"),
                    position: "insideTopRight",
                    fill: "var(--muted-foreground)",
                    fontSize: 11,
                  }}
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
                          {config[String(name)]?.label}
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
                dataKey="total"
                type="monotone"
                stroke="none"
                fill="var(--color-total)"
                fillOpacity={0.12}
                isAnimationActive={false}
              />
              {panels.map((p) => (
                <Line
                  key={p.id}
                  dataKey={`p${p.id}`}
                  type="monotone"
                  stroke={`var(--color-p${p.id})`}
                  strokeWidth={2}
                  dot={false}
                  connectNulls={false}
                  hide={hidden.has(p.id)}
                  isAnimationActive={false}
                />
              ))}
            </ComposedChart>
          </ChartContainer>
        )}
      </CardContent>
    </Card>
  )
}

/** The solar part of the energy page: each inverter with its panels, and their curves for a day. */
export function SolarPanels({
  devices,
  date,
  today,
  withCurve,
}: {
  devices: DeviceView[]
  date: string
  today: string
  withCurve: boolean
}) {
  const inverters = devices.filter((d) => d.traits.includes("solar.inverter@1"))
  const panels = devices.filter((d) => d.traits.includes("solar.panel@1"))
  if (!inverters.length) return null
  return (
    <div className="grid gap-6">
      {inverters.map((inv) => (
        <InverterCard
          key={inv.id}
          inverter={inv}
          panels={panelsOf(inv, panels)}
        />
      ))}
      {withCurve && panels.length > 0 && (
        <PanelsCurve panels={panels} date={date} today={today} />
      )}
    </div>
  )
}
