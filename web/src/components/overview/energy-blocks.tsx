import { createContext, useContext, useState } from "react"
import { useQuery } from "@tanstack/react-query"
import { api } from "@/lib/api"
import { tn } from "@/lib/i18n"
import { SurplusCard } from "@/components/surplus"
import { Skeleton } from "@/components/ui/skeleton"
import {
  Breakdown,
  DayBars,
  DayCurve,
  FlowCard,
  isoDay,
  Kpis,
  Measures,
  PERIOD_DAYS,
  total,
  type KpiCurves,
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
  const live = useQuery({
    queryKey: ["energy-live"],
    queryFn: api.energyLive,
    refetchInterval: 5_000,
  })
  const count =
    period === "years" ? PERIOD_DAYS.years : period === "year" ? 365 : 30
  const days = useQuery({
    queryKey: ["energy-days", count],
    queryFn: () => api.energyDays(count),
    refetchInterval: 60_000,
  })
  const day = useQuery({
    queryKey: ["energy-day", date],
    queryFn: () => api.energyDay(date),
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

/** The values measured along the day (the quarters of an hour without any are left out). */
function measured<P>(points: P[] | undefined, value: (p: P) => number | null) {
  return (points ?? []).map(value).filter((v): v is number => v !== null)
}

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
  if (config.curves === true)
    curves =
      period === "day"
        ? {
            production: measured(e.day?.points, (p) => p.solar),
            consumption: measured(e.day?.points, (p) => p.home),
            grid: measured(e.day?.points, (p) =>
              p.grid === null ? null : Math.max(0, p.grid)
            ),
          }
        : {
            production: e.shown.map((d) => d.production ?? 0),
            consumption: e.shown.map((d) => d.consumption ?? 0),
            grid: e.shown.map((d) => d.imported ?? 0),
            cost: e.shown.map((d) =>
              d.cost ? d.cost.energy + d.cost.subscription : 0
            ),
          }
  return (
    <Kpis
      totals={e.totals}
      live={e.live}
      showMoney={config.money !== false}
      curves={curves}
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
