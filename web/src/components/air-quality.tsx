import { useState } from "react"
import { useQuery } from "@tanstack/react-query"
import { Leaf } from "lucide-react"
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts"
import type { AirQualityLevel, Reading, Zone } from "@/lib/api"
import { api } from "@/lib/api"
import { AIR_QUALITY_LABEL, AIR_QUALITY_TONE, percent } from "@/lib/format"
import { locale, t } from "@/lib/i18n"
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
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

export const AIR_LEVELS: AirQualityLevel[] = ["good", "regular", "bad"]
const LEVELS = AIR_LEVELS
const LEVEL_OF: Record<number, AirQualityLevel> = {
  1: "good",
  2: "regular",
  3: "bad",
}

const shareConfig = Object.fromEntries(
  LEVELS.map((l) => [
    l,
    { label: AIR_QUALITY_LABEL[l], color: AIR_QUALITY_TONE[l].color },
  ])
) satisfies ChartConfig

const MODE_LABEL = {
  off: t("Purification arrêtée"),
  on: t("Purification en marche"),
  auto: t("Purification automatique"),
}

/** A reading with one value per air quality band, for step areas (see ZoneHistory). */
export type AirSeriesPoint = Reading & {
  air_level: AirQualityLevel | null
  aq_good: number | null
  aq_regular: number | null
  aq_bad: number | null
}

/**
 * Each level is its own step area, 1 where the reading has that level. A band also holds the point
 * after its last reading, so that the step reaches the next reading instead of stopping short.
 */
export function airQualitySeries(readings: Reading[]): AirSeriesPoint[] {
  const levels = readings.map((r) => LEVEL_OF[r.air_quality ?? 0] ?? null)
  return readings.map((r, i) => {
    const band = (l: AirQualityLevel) =>
      levels[i] === l || (levels[i - 1] === l && levels[i] !== null) ? 1 : null
    return {
      ...r,
      air_level: levels[i],
      aq_good: band("good"),
      aq_regular: band("regular"),
      aq_bad: band("bad"),
    }
  })
}

export function AirQualityLegend() {
  return (
    <div className="flex flex-wrap justify-center gap-4 text-xs">
      {LEVELS.map((l) => (
        <span key={l} className="inline-flex items-center gap-1.5">
          <span
            className="size-2.5 rounded-sm"
            style={{ background: AIR_QUALITY_TONE[l].color }}
          />
          {AIR_QUALITY_LABEL[l]}
        </span>
      ))}
    </div>
  )
}

/** Air quality measured by the zone's thermostat: now, over time and day by day. */
export function AirQualityCard({ zone }: { zone: Zone }) {
  const [period, setPeriod] = useState("24")
  const hours = Number(period)
  const key = [zone.installation_id, zone.system_id, zone.zone_id]
  const days = Math.max(1, hours / 24)
  const daily = useQuery({
    queryKey: ["daily", ...key, String(days)],
    queryFn: () => api.daily(zone, days),
    refetchInterval: 300_000,
  })
  const air = zone.air_quality
  if (!air) return null

  const shares = (daily.data ?? []).flatMap((d) => {
    const s = d.air_quality_secs
    const total = s ? s.good + s.regular + s.bad : 0
    if (!s || total === 0) return []
    return [
      {
        label: new Date(`${d.day}T12:00:00`).toLocaleDateString(locale(), {
          day: "2-digit",
          month: "2-digit",
        }),
        good: Math.round((s.good / total) * 100),
        regular: Math.round((s.regular / total) * 100),
        bad: Math.round((s.bad / total) * 100),
      },
    ]
  })
  const totals = (daily.data ?? []).reduce(
    (acc, d) => {
      const s = d.air_quality_secs
      if (s) {
        acc.good += s.good
        acc.measured += s.good + s.regular + s.bad
      }
      return acc
    },
    { good: 0, measured: 0 }
  )

  return (
    <Card className="rounded-3xl">
      <CardHeader className="flex flex-row items-start justify-between gap-4">
        <div>
          <CardTitle>{t("Qualité de l'air")}</CardTitle>
          <CardDescription>
            {t(
              "Mesurée par le thermostat de la zone ; heure par heure dans l'historique"
            )}
          </CardDescription>
        </div>
        <ToggleGroup
          type="single"
          value={period}
          onValueChange={(v) => v && setPeriod(v)}
          variant="outline"
          size="sm"
        >
          <ToggleGroupItem value="24">{t("24 h")}</ToggleGroupItem>
          <ToggleGroupItem value="168">{t("7 j")}</ToggleGroupItem>
          <ToggleGroupItem value="720">{t("30 j")}</ToggleGroupItem>
        </ToggleGroup>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="grid grid-cols-2 gap-2">
          <div className="rounded-2xl bg-muted/60 px-3 py-2">
            <div className="text-xs text-muted-foreground">
              {t("Maintenant")}
            </div>
            <div
              className={cn(
                "flex items-center gap-1.5 text-lg font-semibold",
                air.level && AIR_QUALITY_TONE[air.level].text
              )}
            >
              <Leaf className="size-4" />
              {air.level ? AIR_QUALITY_LABEL[air.level] : t("Pas de mesure")}
            </div>
            {air.mode && (
              <div className="text-xs text-muted-foreground">
                {MODE_LABEL[air.mode]}
              </div>
            )}
          </div>
          <div className="rounded-2xl bg-muted/60 px-3 py-2">
            <div className="text-xs text-muted-foreground">
              {t("Air de bonne qualité")}
            </div>
            <div className="tabular text-lg font-semibold">
              {totals.measured > 0
                ? percent(totals.good / totals.measured)
                : "–"}
            </div>
            <div className="text-xs text-muted-foreground">
              {t("du temps mesuré sur la période")}
            </div>
          </div>
        </div>

        {hours > 24 && shares.length > 0 && (
          <ChartContainer config={shareConfig} className="h-36 w-full">
            <BarChart data={shares} margin={{ left: 0, right: 8 }}>
              <CartesianGrid vertical={false} />
              <XAxis
                dataKey="label"
                tickLine={false}
                axisLine={false}
                minTickGap={8}
              />
              <YAxis
                tickLine={false}
                axisLine={false}
                width={36}
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
                          {AIR_QUALITY_LABEL[name as AirQualityLevel]}
                        </span>
                        <span className="tabular font-medium">
                          {percent(Number(value) / 100)}
                        </span>
                      </div>
                    )}
                  />
                }
              />
              {LEVELS.map((l, i) => (
                <Bar
                  key={l}
                  dataKey={l}
                  stackId="air"
                  fill={`var(--color-${l})`}
                  maxBarSize={20}
                  radius={i === LEVELS.length - 1 ? [4, 4, 0, 0] : 0}
                />
              ))}
            </BarChart>
          </ChartContainer>
        )}
        <AirQualityLegend />
      </CardContent>
    </Card>
  )
}
