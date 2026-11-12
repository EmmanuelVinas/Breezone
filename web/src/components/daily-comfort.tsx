import { useState } from "react"
import { useQuery } from "@tanstack/react-query"
import {
  Bar,
  BarChart,
  CartesianGrid,
  ComposedChart,
  Line,
  XAxis,
  YAxis,
} from "recharts"
import type { DayStats, Zone } from "@/lib/api"
import { api } from "@/lib/api"
import { duration, formatTemp, percent } from "@/lib/format"
import { locale, t } from "@/lib/i18n"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import {
  ChartContainer,
  ChartLegend,
  ChartLegendContent,
  ChartTooltip,
  ChartTooltipContent,
} from "@/components/ui/chart"
import type { ChartConfig } from "@/components/ui/chart"
import { Skeleton } from "@/components/ui/skeleton"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

const temperatureConfig = {
  range: { label: t("Min – max"), color: "var(--heat)" },
  avg_temp: { label: t("Température moyenne"), color: "var(--heat)" },
  avg_humidity: { label: t("Humidité moyenne"), color: "var(--cool)" },
} satisfies ChartConfig

const comfortConfig = {
  comfort_pct: { label: t("Confort"), color: "var(--primary)" },
  demand_hours: { label: t("En demande"), color: "var(--heat)" },
} satisfies ChartConfig

function dayLabel(day: string, short: boolean) {
  const d = new Date(`${day}T12:00:00`)
  return d.toLocaleDateString(
    locale(),
    short ? { weekday: "short" } : { day: "2-digit", month: "2-digit" }
  )
}

function Summary({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-2xl bg-muted/60 px-3 py-2">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="tabular text-lg font-semibold">{value}</div>
    </div>
  )
}

/** Comfort of a zone day by day: temperatures, share of the time near the setpoint, demand. */
export function DailyComfort({ zone }: { zone: Zone }) {
  const [days, setDays] = useState("7")
  const daily = useQuery({
    queryKey: [
      "daily",
      zone.installation_id,
      zone.system_id,
      zone.zone_id,
      days,
    ],
    queryFn: () => api.daily(zone, Number(days)),
    refetchInterval: 300_000,
  })
  const stats: DayStats[] = daily.data ?? []
  const short = Number(days) <= 7
  const data = stats.map((d) => ({
    ...d,
    label: dayLabel(d.day, short),
    range:
      d.min_temp !== null && d.max_temp !== null
        ? [d.min_temp, d.max_temp]
        : null,
    comfort_pct: d.comfort === null ? null : Math.round(d.comfort * 100),
    demand_hours:
      d.demand_secs === null ? null : Math.round(d.demand_secs / 360) / 10,
  }))
  const humidity = stats.some((d) => d.avg_humidity !== null)
  const on = stats.reduce((s, d) => s + d.on_secs, 0)
  const comfortable = stats.reduce((s, d) => s + d.comfort_secs, 0)
  const recordedDemand = stats.filter((d) => d.demand_secs !== null)
  const demand = recordedDemand.reduce((s, d) => s + (d.demand_secs ?? 0), 0)

  return (
    <Card className="rounded-3xl">
      <CardHeader className="flex flex-row items-start justify-between gap-4">
        <div>
          <CardTitle>{t("Confort jour par jour")}</CardTitle>
          <CardDescription>
            {t(
              "Part du temps allumée où la température est à ±0,5° de la consigne"
            )}
          </CardDescription>
        </div>
        <ToggleGroup
          type="single"
          value={days}
          onValueChange={(v) => v && setDays(v)}
          variant="outline"
          size="sm"
        >
          <ToggleGroupItem value="7">{t("7 j")}</ToggleGroupItem>
          <ToggleGroupItem value="30">{t("30 j")}</ToggleGroupItem>
        </ToggleGroup>
      </CardHeader>
      <CardContent className="space-y-4">
        {daily.isLoading ? (
          <Skeleton className="h-72 w-full" />
        ) : stats.length === 0 ? (
          <div className="flex h-40 items-center justify-center text-sm text-muted-foreground">
            {t(
              "Pas encore assez de relevés : ils s'accumulent toutes les 5 minutes."
            )}
          </div>
        ) : (
          <>
            <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
              <Summary
                label={t("Confort")}
                value={on > 0 ? percent(comfortable / on) : "–"}
              />
              <Summary
                label={t("Allumée, par jour")}
                value={duration(on / stats.length)}
              />
              {recordedDemand.length > 0 && (
                <Summary
                  label={t("En demande, par jour")}
                  value={duration(demand / recordedDemand.length)}
                />
              )}
            </div>
            <ChartContainer config={temperatureConfig} className="h-44 w-full">
              <ComposedChart data={data} margin={{ left: 0, right: 8 }}>
                <CartesianGrid vertical={false} />
                <XAxis
                  dataKey="label"
                  tickLine={false}
                  axisLine={false}
                  minTickGap={8}
                />
                <YAxis
                  yAxisId="t"
                  tickLine={false}
                  axisLine={false}
                  width={36}
                  domain={["dataMin - 1", "dataMax + 1"]}
                  tickFormatter={(v: number) => `${Math.round(v)}°`}
                />
                {humidity && (
                  <YAxis
                    yAxisId="h"
                    orientation="right"
                    tickLine={false}
                    axisLine={false}
                    width={36}
                    domain={[0, 100]}
                    ticks={[0, 50, 100]}
                    tickFormatter={(v: number) => `${v}%`}
                  />
                )}
                <ChartTooltip
                  content={
                    <ChartTooltipContent
                      formatter={(value, name) => (
                        <div className="flex w-full justify-between gap-4">
                          <span className="text-muted-foreground">
                            {name === "range"
                              ? t("Min – max")
                              : name === "avg_humidity"
                                ? t("Humidité moyenne")
                                : t("Température moyenne")}
                          </span>
                          <span className="tabular font-medium">
                            {Array.isArray(value)
                              ? `${formatTemp(Number(value[0]))} – ${formatTemp(Number(value[1]))}`
                              : name === "avg_humidity"
                                ? percent(Number(value) / 100)
                                : formatTemp(Number(value))}
                          </span>
                        </div>
                      )}
                    />
                  }
                />
                <Bar
                  yAxisId="t"
                  dataKey="range"
                  minPointSize={4}
                  fill="var(--color-range)"
                  fillOpacity={0.25}
                  radius={6}
                  maxBarSize={28}
                />
                <Line
                  yAxisId="t"
                  dataKey="avg_temp"
                  type="monotone"
                  stroke="var(--color-avg_temp)"
                  strokeWidth={2}
                  dot={{ r: 2.5 }}
                />
                {humidity && (
                  <Line
                    yAxisId="h"
                    dataKey="avg_humidity"
                    type="monotone"
                    stroke="var(--color-avg_humidity)"
                    strokeWidth={1.5}
                    strokeDasharray="4 3"
                    dot={false}
                  />
                )}
                <ChartLegend content={<ChartLegendContent />} />
              </ComposedChart>
            </ChartContainer>
            <ChartContainer config={comfortConfig} className="h-40 w-full">
              <BarChart data={data} margin={{ left: 0, right: 8 }}>
                <CartesianGrid vertical={false} />
                <XAxis
                  dataKey="label"
                  tickLine={false}
                  axisLine={false}
                  minTickGap={8}
                />
                <YAxis
                  yAxisId="pct"
                  tickLine={false}
                  axisLine={false}
                  width={36}
                  domain={[0, 100]}
                  ticks={[0, 50, 100]}
                  tickFormatter={(v: number) => `${v}%`}
                />
                {recordedDemand.length > 0 && (
                  <YAxis
                    yAxisId="h"
                    orientation="right"
                    tickLine={false}
                    axisLine={false}
                    width={32}
                    tickFormatter={(v: number) => `${v} h`}
                  />
                )}
                <ChartTooltip
                  content={
                    <ChartTooltipContent
                      formatter={(value, name) => (
                        <div className="flex w-full justify-between gap-4">
                          <span className="text-muted-foreground">
                            {name === "comfort_pct"
                              ? t("Confort")
                              : t("En demande")}
                          </span>
                          <span className="tabular font-medium">
                            {name === "comfort_pct"
                              ? percent(Number(value) / 100)
                              : duration(Number(value) * 3600)}
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
                  radius={4}
                  maxBarSize={20}
                />
                <ChartLegend content={<ChartLegendContent />} />
                {recordedDemand.length > 0 && (
                  <Bar
                    yAxisId="h"
                    dataKey="demand_hours"
                    fill="var(--color-demand_hours)"
                    fillOpacity={0.6}
                    radius={4}
                    maxBarSize={20}
                  />
                )}
              </BarChart>
            </ChartContainer>
          </>
        )}
      </CardContent>
    </Card>
  )
}
