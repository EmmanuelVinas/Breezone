import { createContext, useContext, useState } from "react"
import { useQuery } from "@tanstack/react-query"
import { api, type DayEnergy, type EnergyDay } from "@/lib/api"
import { kwh, number, time, watts } from "@/lib/format"
import { useChosenInstallation } from "@/lib/selected-installation"
import { tn } from "@/lib/i18n"
import { SurplusCard } from "@/components/surplus"
import { Skeleton } from "@/components/ui/skeleton"
import {
  Breakdown,
  DayBars,
  DayCurve,
  FlowCard,
  dayLabel,
  isoDay,
  Kpis,
  Measures,
  PERIOD_DAYS,
  total,
  type KpiCurves,
  type KpiId,
  type Period,
} from "@/components/energy/sections"

/** The period and the day the energy page shows: its blocks follow them unless set otherwise. */
export const EnergyPeriodContext = createContext<{
  period: Period
  date: string
  setDate: (d: string) => void
} | null>(null)

export const PERIODS: Period[] = ["day", "week", "month", "year", "years"]

/** The period of a block: its own, else the page's, else the day. */
function usePeriod(config: Record<string, unknown>) {
  const page = useContext(EnergyPeriodContext)
  const own = PERIODS.includes(config.period as Period)
    ? (config.period as Period)
    : null
  return {
    period: own ?? page?.period ?? "day",
    // A block with a period of its own shows today.
    date: own ? null : (page?.date ?? null),
    setDate: own ? undefined : page?.setDate,
  }
}

/** The energy of a period: what was measured each day, or along the day shown. */
export function useEnergy(period: Period, date: string) {
  const today = isoDay(new Date())
  // The costs at the prices of the installation chosen.
  const installation = useChosenInstallation()?.id
  const live = useQuery({
    queryKey: ["energy-live"],
    queryFn: api.energyLive,
    refetchInterval: 5_000,
  })
  const count =
    period === "years" ? PERIOD_DAYS.years : period === "year" ? 365 : 30
  const days = useQuery({
    queryKey: ["energy-days", count, installation],
    queryFn: () => api.energyDays(count, installation),
    refetchInterval: 60_000,
  })
  const day = useQuery({
    queryKey: ["energy-day", date, installation],
    queryFn: () => api.energyDay(date, installation),
    enabled: period === "day",
    refetchInterval: date === today ? 60_000 : false,
  })
  const range = useQuery({
    queryKey: ["energy-range"],
    queryFn: api.energyRange,
    staleTime: 3_600_000,
  })
  const firstDay = range.data?.first_day
  // Long periods start at the first day with energy, not years of empty days.
  const shown = (days.data ?? [])
    .slice(-PERIOD_DAYS[period])
    .filter((d) => period !== "years" || !firstDay || d.day >= firstDay)
  const totals =
    period === "day"
      ? day.data?.totals
      : shown.length
        ? total(shown)
        : undefined
  return {
    today,
    live: live.data,
    totals,
    shown,
    days: days.data ?? [],
    day: day.data,
  }
}

type EnergyPoint = EnergyDay["points"][number]

/** The values measured along the day, with their time (the quarters of an hour without any are
 * left out). */
function timed(
  points: EnergyPoint[] | undefined,
  value: (p: EnergyPoint) => number | null
) {
  return (points ?? [])
    .map((p): [number, number | null] => [p.ts, value(p)])
    .filter((p): p is [number, number] => p[1] !== null)
}

/** Local noon of a day ("2026-11-20"), as a time. */
const dayTs = (day: string) => new Date(`${day}T12:00:00`).getTime() / 1000

const Waiting = ({ className = "h-40" }: { className?: string }) => (
  <Skeleton className={`${className} w-full rounded-3xl`} />
)

export function EnergyKpisBlock({
  config,
}: {
  config: Record<string, unknown>
}) {
  const { period, date } = usePeriod(config)
  const e = useEnergy(period, date ?? isoDay(new Date()))
  if (!e.live || !e.totals) return <Waiting className="h-28" />
  let curves: KpiCurves | undefined
  if (config.curves === true) {
    // The power of the day, quarter by quarter; over a period, the energy (or cost) of each day.
    const power = (value: (p: EnergyPoint) => number | null) => ({
      points: timed(e.day?.points, value),
      format: (v: number) => watts(v),
      at: time,
    })
    const daily = (
      value: (d: DayEnergy) => number,
      format: (v: number) => string
    ) => ({
      points: e.shown.map((d): [number, number] => [dayTs(d.day), value(d)]),
      format,
      at: (ts: number) => dayLabel(isoDay(new Date(ts * 1000)), "long"),
    })
    curves =
      period === "day"
        ? {
            production: power((p) => p.solar),
            consumption: power((p) => p.home),
            grid: power((p) => (p.grid === null ? null : Math.max(0, p.grid))),
          }
        : {
            production: daily(
              (d) => d.production ?? 0,
              (v) => kwh(v)
            ),
            consumption: daily(
              (d) => d.consumption ?? 0,
              (v) => kwh(v)
            ),
            grid: daily(
              (d) => d.imported ?? 0,
              (v) => kwh(v)
            ),
            cost: daily(
              (d) => (d.cost ? d.cost.energy + d.cost.subscription : 0),
              // In the currency of the contract: set by the tile.
              (v) => number(v, 2)
            ),
          }
  }
  return (
    <Kpis
      totals={e.totals}
      live={e.live}
      showMoney={config.money !== false}
      curves={curves}
      only={Array.isArray(config.kpis) ? (config.kpis as KpiId[]) : undefined}
    />
  )
}

export function EnergyFlowBlock() {
  const e = useEnergy("day", isoDay(new Date()))
  const devices = useQuery({ queryKey: ["devices"], queryFn: api.devices })
  if (!e.live) return <Waiting />
  const panels = (devices.data ?? []).filter((d) =>
    d.traits.includes("solar.panel@1")
  ).length
  return (
    <FlowCard
      live={e.live}
      solarSub={panels ? tn(panels, "{n} panneau", "{n} panneaux") : undefined}
    />
  )
}

export function EnergyCurveBlock({
  config,
}: {
  config: Record<string, unknown>
}) {
  const page = usePeriod(config)
  const today = isoDay(new Date())
  const [own, setOwn] = useState(today)
  const date = page.date ?? own
  const e = useEnergy("day", date)
  if (!e.live) return <Waiting />
  return (
    <DayCurve
      date={date}
      today={today}
      onDate={page.setDate ?? setOwn}
      live={e.live}
    />
  )
}

export function EnergyBarsBlock({
  config,
}: {
  config: Record<string, unknown>
}) {
  const { period, date } = usePeriod(config)
  const e = useEnergy(period, date ?? isoDay(new Date()))
  if (!e.live) return <Waiting />
  // On a day, the last two weeks around it.
  return (
    <DayBars
      days={period === "day" ? e.days.slice(-14) : e.shown}
      live={e.live}
    />
  )
}

export function EnergyBreakdownBlock({
  config,
}: {
  config: Record<string, unknown>
}) {
  const { period, date } = usePeriod(config)
  const e = useEnergy(period, date ?? isoDay(new Date()))
  if (!e.totals) return <Waiting />
  return <Breakdown totals={e.totals} period={period} />
}

export function EnergyMeasuresBlock({
  config,
}: {
  config: Record<string, unknown>
}) {
  const { period, date } = usePeriod(config)
  const e = useEnergy(period, date ?? isoDay(new Date()))
  const devices = useQuery({ queryKey: ["devices"], queryFn: api.devices })
  if (!e.totals) return <Waiting />
  return (
    <Measures devices={devices.data ?? []} totals={e.totals} period={period} />
  )
}

export function EnergySurplusBlock() {
  return <SurplusCard />
}
