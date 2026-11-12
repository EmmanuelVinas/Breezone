import { useState } from "react"
import { useQuery } from "@tanstack/react-query"
import {
  Area,
  Brush,
  CartesianGrid,
  ComposedChart,
  Line,
  XAxis,
  YAxis,
} from "recharts"
import type { Reading, Zone } from "@/lib/api"
import { api } from "@/lib/api"
import {
  AIR_QUALITY_LABEL,
  AIR_QUALITY_TONE,
  dateTime,
  formatTemp,
  percent,
  time,
} from "@/lib/format"
import { locale, t } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import {
  AIR_LEVELS,
  airQualitySeries,
  type AirSeriesPoint,
} from "@/components/air-quality"
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
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

/** Longest time between two readings still drawn as one line (they are 5 minutes apart). */
const GAP_SECS = 1800

const chartConfig = {
  room_temp: { label: t("Température"), color: "var(--heat)" },
  setpoint: { label: t("Consigne"), color: "var(--primary)" },
  humidity: { label: t("Humidité"), color: "var(--cool)" },
  ...Object.fromEntries(
    AIR_LEVELS.map((l) => [
      `aq_${l}`,
      { label: AIR_QUALITY_LABEL[l], color: AIR_QUALITY_TONE[l].color },
    ])
  ),
} satisfies ChartConfig

type Series = "room_temp" | "setpoint" | "humidity" | "air"

/** A gap without readings (installation unreachable) breaks the lines instead of joining them. */
function withGaps(readings: Reading[]): Reading[] {
  return readings.flatMap((r, i) => {
    const previous = readings[i - 1]
    if (!previous || r.ts - previous.ts <= GAP_SECS) return [r]
    const gap: Reading = {
      ...r,
      ts: previous.ts + 1,
      room_temp: null,
      humidity: null,
      setpoint: null,
      air_quality: null,
    }
    return [gap, r]
  })
}

/** Temperature, setpoint, humidity and air quality of a zone, with a zoom bar and toggles. */
export function ZoneHistory({ zone }: { zone: Zone }) {
  const [hours, setHours] = useState("24")
  const [hidden, setHidden] = useState<ReadonlySet<Series>>(new Set())
  const history = useQuery({
    queryKey: [
      "history",
      zone.installation_id,
      zone.system_id,
      zone.zone_id,
      hours,
    ],
    queryFn: () => api.history(zone, Number(hours)),
    refetchInterval: 60_000,
  })
  const readings = withGaps(history.data ?? [])
  const air = !!zone.air_quality && readings.some((r) => r.air_quality)
  const data: AirSeriesPoint[] = airQualitySeries(readings).map((r) => ({
    ...r,
    setpoint: r.is_on ? r.setpoint : null,
  }))
  // Several days: the hour too, as a zoom can show the same day twice.
  const tickFormat = (ts: number) =>
    Number(hours) <= 24
      ? time(ts)
      : `${new Date(ts * 1000).toLocaleDateString(locale(), {
          day: "2-digit",
          month: "2-digit",
        })} ${time(ts)}`
  const toggle = (s: Series) =>
    setHidden((h) => {
      const next = new Set(h)
      if (next.has(s)) next.delete(s)
      else next.add(s)
      return next
    })
  const legend: { key: Series; label: string; color: string }[] = [
    { key: "room_temp", label: t("Température"), color: "var(--heat)" },
    { key: "setpoint", label: t("Consigne"), color: "var(--primary)" },
    { key: "humidity", label: t("Humidité"), color: "var(--cool)" },
    ...(air
      ? [
          {
            key: "air" as const,
            label: t("Qualité de l'air"),
            color: "var(--success)",
          },
        ]
      : []),
  ]

  return (
    <Card className="rounded-3xl">
      <CardHeader className="flex flex-row items-start justify-between gap-4">
        <div>
          <CardTitle>{t("Historique")}</CardTitle>
          <CardDescription>
            {t(
              "Glissez les poignées sous le graphique pour zoomer, touchez la légende pour masquer une courbe"
            )}
          </CardDescription>
        </div>
        <ToggleGroup
          type="single"
          value={hours}
          onValueChange={(v) => v && setHours(v)}
          variant="outline"
          size="sm"
        >
          <ToggleGroupItem value="24">{t("24 h")}</ToggleGroupItem>
          <ToggleGroupItem value="168">{t("7 j")}</ToggleGroupItem>
          <ToggleGroupItem value="720">{t("30 j")}</ToggleGroupItem>
        </ToggleGroup>
      </CardHeader>
      <CardContent className="space-y-3">
        {history.isLoading ? (
          <Skeleton className="h-72 w-full" />
        ) : data.length < 2 ? (
          <div className="flex h-72 items-center justify-center text-sm text-muted-foreground">
            {t(
              "Pas encore assez de relevés : ils s'accumulent toutes les 5 minutes."
            )}
          </div>
        ) : (
          <ChartContainer
            // A new period starts unzoomed.
            key={hours}
            config={chartConfig}
            className="aspect-auto h-80 w-full"
          >
            <ComposedChart data={data} margin={{ left: 0, right: 8 }}>
              <CartesianGrid vertical={false} />
              <XAxis
                dataKey="ts"
                type="number"
                domain={["dataMin", "dataMax"]}
                scale="time"
                tickFormatter={tickFormat}
                tickLine={false}
                axisLine={false}
                minTickGap={56}
              />
              <YAxis
                yAxisId="t"
                tickLine={false}
                axisLine={false}
                width={36}
                domain={["dataMin - 1", "dataMax + 1"]}
                tickFormatter={(v: number) => `${Math.round(v)}°`}
              />
              <YAxis
                yAxisId="h"
                orientation="right"
                tickLine={false}
                axisLine={false}
                width={36}
                domain={[0, 100]}
                tickFormatter={(v: number) => `${v}%`}
              />
              {/* Air quality: a band along the bottom of the plot. */}
              <YAxis yAxisId="aq" hide width={0} domain={[0, 16]} />
              <ChartTooltip
                content={
                  <ChartTooltipContent
                    labelFormatter={(_, p) =>
                      p?.[0] ? dateTime(Number(p[0].payload.ts)) : ""
                    }
                    formatter={(value, name, item) => {
                      const key = String(name)
                      if (key.startsWith("aq_")) {
                        // One row for the level of this reading, not one per band.
                        const level = (item.payload as AirSeriesPoint).air_level
                        if (`aq_${level}` !== key) return null
                      }
                      return (
                        <div className="flex w-full items-center justify-between gap-4">
                          <span className="flex items-center gap-1.5 text-muted-foreground">
                            <span
                              className="size-2 rounded-[2px]"
                              style={{ background: item.color }}
                            />
                            {key.startsWith("aq_")
                              ? t("Qualité de l'air")
                              : chartConfig[key as keyof typeof chartConfig]
                                  ?.label}
                          </span>
                          <span className="tabular font-medium">
                            {key === "humidity"
                              ? percent(Number(value) / 100)
                              : key.startsWith("aq_")
                                ? AIR_QUALITY_LABEL[
                                    key.slice(
                                      3
                                    ) as keyof typeof AIR_QUALITY_LABEL
                                  ]
                                : formatTemp(Number(value))}
                          </span>
                        </div>
                      )
                    }}
                  />
                }
              />
              {air &&
                AIR_LEVELS.map((l) => (
                  <Area
                    key={l}
                    yAxisId="aq"
                    dataKey={`aq_${l}`}
                    type="stepAfter"
                    stroke="none"
                    fill={`var(--color-aq_${l})`}
                    fillOpacity={0.9}
                    isAnimationActive={false}
                    hide={hidden.has("air")}
                    activeDot={false}
                  />
                ))}
              <Line
                yAxisId="h"
                dataKey="humidity"
                type="monotone"
                stroke="var(--color-humidity)"
                strokeWidth={1.5}
                dot={false}
                opacity={0.7}
                hide={hidden.has("humidity")}
              />
              <Line
                yAxisId="t"
                dataKey="setpoint"
                type="stepAfter"
                stroke="var(--color-setpoint)"
                strokeWidth={2}
                strokeDasharray="5 4"
                dot={false}
                connectNulls={false}
                hide={hidden.has("setpoint")}
              />
              <Line
                yAxisId="t"
                dataKey="room_temp"
                type="monotone"
                stroke="var(--color-room_temp)"
                strokeWidth={2.5}
                dot={false}
                hide={hidden.has("room_temp")}
              />
              <Brush
                dataKey="ts"
                height={24}
                travellerWidth={8}
                stroke="var(--border)"
                fill="var(--card)"
                tickFormatter={tickFormat}
              />
            </ComposedChart>
          </ChartContainer>
        )}
        <div className="flex flex-wrap justify-center gap-2">
          {legend.map((s) => (
            <button
              key={s.key}
              type="button"
              onClick={() => toggle(s.key)}
              aria-pressed={!hidden.has(s.key)}
              className={cn(
                "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs transition-opacity",
                hidden.has(s.key) && "opacity-40"
              )}
            >
              <span
                className="size-2.5 rounded-sm"
                style={{ background: s.color }}
              />
              {s.label}
            </button>
          ))}
        </div>
      </CardContent>
    </Card>
  )
}
