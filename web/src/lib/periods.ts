import { time } from "@/lib/format"
import { locale, t } from "@/lib/i18n"

/** The periods a curve over time offers, from the last day to the last five years. */
export type HistoryPeriod = "24h" | "7d" | "30d" | "1y" | "5y"

export const PERIOD_HOURS: Record<HistoryPeriod, number> = {
  "24h": 24,
  "7d": 24 * 7,
  "30d": 24 * 30,
  "1y": 24 * 365,
  "5y": 24 * 365 * 5,
}

export const PERIOD_LABEL: Record<HistoryPeriod, string> = {
  "24h": t("24 h"),
  "7d": t("7 j"),
  "30d": t("30 j"),
  "1y": t("1 an"),
  "5y": t("5 ans"),
}

/**
 * The periods worth offering: a longer one only when the history goes back further than the
 * previous one (no « 5 ans » with a few months of data). `oldest`: the oldest point, `now`: when
 * it was read (seconds; the time of the query, so that rendering stays pure).
 */
export function periodsFor(
  oldest: number | null | undefined,
  now: number,
  all: HistoryPeriod[] = ["24h", "7d", "30d", "1y", "5y"]
): HistoryPeriod[] {
  const age = oldest ? now - oldest : 0
  return all.filter((_, i) => i === 0 || age > PERIOD_HOURS[all[i - 1]] * 3600)
}

/** Points of a curve for a period: about one every 15 min on a day, one a month over five years. */
export function bucketFor(period: HistoryPeriod): number {
  return {
    "24h": 900,
    "7d": 3600,
    "30d": 4 * 3600,
    "1y": 86_400,
    "5y": 30 * 86_400,
  }[period]
}

/** The label of a time on the axis of a curve over `period`. */
export function periodTick(period: HistoryPeriod, ts: number): string {
  const d = new Date(ts * 1000)
  if (period === "24h") return time(ts)
  if (period === "1y") return d.toLocaleDateString(locale(), { month: "short" })
  if (period === "5y")
    return d.toLocaleDateString(locale(), { month: "short", year: "2-digit" })
  return d.toLocaleDateString(locale(), { day: "2-digit", month: "2-digit" })
}

/** The time of a point in the tooltip of a curve over `period`. */
export function periodPoint(period: HistoryPeriod, ts: number): string {
  const d = new Date(ts * 1000)
  if (period === "24h") return time(ts)
  if (period === "5y")
    return d.toLocaleDateString(locale(), { month: "long", year: "numeric" })
  const day = d.toLocaleDateString(locale(), {
    day: "numeric",
    month: "short",
    year: period === "1y" ? "numeric" : undefined,
  })
  return period === "1y" ? day : `${day} ${time(ts)}`
}
