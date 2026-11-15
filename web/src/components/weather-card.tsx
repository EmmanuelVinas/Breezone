import { useQuery } from "@tanstack/react-query"
import { Sunrise, Sunset, Wind } from "lucide-react"
import { api } from "@/lib/api"
import { time } from "@/lib/format"
import { t } from "@/lib/i18n"
import { weatherLook } from "@/lib/weather"
import { Card, CardContent } from "@/components/ui/card"

const deg = (v: number | null | undefined) =>
  v === null || v === undefined ? "–" : `${Math.round(v)}°`

/** The weather of the home: now, today, the sun and the next hours (none without a city). */
export function WeatherCard() {
  const view = useQuery({
    queryKey: ["weather"],
    queryFn: api.weather,
    refetchInterval: 10 * 60_000,
  }).data
  const w = view?.weather
  if (!w) return null
  const now = weatherLook(w.code)
  const Icon = now.icon
  return (
    <Card className="mb-6 rounded-3xl bg-[radial-gradient(80%_120%_at_0%_0%,color-mix(in_oklch,var(--cool)_14%,transparent),transparent_70%)]">
      <CardContent className="flex flex-wrap items-center gap-x-8 gap-y-4">
        <div className="flex items-center gap-4">
          <Icon className="size-12 text-cool" aria-hidden />
          <div>
            <div className="text-sm text-muted-foreground">{w.place}</div>
            <div className="tabular font-heading text-4xl font-bold tracking-tight">
              {deg(w.temperature)}
            </div>
            <div className="text-sm text-muted-foreground">
              {[
                now.label,
                t("max {max} · min {min}", {
                  max: deg(w.today_max),
                  min: deg(w.today_min),
                }),
              ].join(" · ")}
            </div>
          </div>
        </div>
        <div className="grid gap-1 text-sm text-muted-foreground">
          {view?.sun?.sunrise && view.sun.sunset && (
            <span className="inline-flex items-center gap-1.5">
              <Sunrise className="size-4 text-warning" />{" "}
              {time(view.sun.sunrise)}
              <Sunset className="ml-2 size-4 text-warning" />{" "}
              {time(view.sun.sunset)}
            </span>
          )}
          {w.wind !== null && (
            <span className="inline-flex items-center gap-1.5">
              <Wind className="size-4" />
              {t("{n} km/h", { n: Math.round(w.wind) })}
              {w.apparent !== null &&
                ` · ${t("ressenti {t}", { t: deg(w.apparent) })}`}
            </span>
          )}
        </div>
        <div className="flex min-w-0 flex-1 [scrollbar-width:none] gap-3 overflow-x-auto">
          {w.hours
            .filter((_, i) => i % 3 === 0)
            .slice(0, 8)
            .map((h) => {
              const HourIcon = weatherLook(h.code).icon
              return (
                <div
                  key={h.ts}
                  className="flex shrink-0 flex-col items-center gap-1 text-xs"
                >
                  <span className="text-muted-foreground">{time(h.ts)}</span>
                  <HourIcon
                    className="size-5"
                    aria-label={weatherLook(h.code).label}
                  />
                  <span className="tabular font-medium">
                    {deg(h.temperature)}
                  </span>
                </div>
              )
            })}
        </div>
      </CardContent>
    </Card>
  )
}
