// What a zone was doing between two history points: off, heating, cooling, or on at its setpoint.
import type { Reading } from "@/lib/api"
import { t } from "@/lib/i18n"

export type ZoneState = "off" | "heating" | "cooling" | "comfort"

/** States drawn as bands: none when the zone is off or at its setpoint. */
export const ZONE_STATES: ZoneState[] = ["heating", "cooling"]

export const ZONE_STATE_LABEL: Record<ZoneState, string> = {
  heating: t("Chauffage"),
  cooling: t("Refroidissement"),
  comfort: t("Confort"),
  off: t("Éteinte"),
}

export const ZONE_STATE_COLOR: Record<ZoneState, string> = {
  heating: "var(--heat)",
  cooling: "var(--cool)",
  comfort: "var(--success)",
  off: "var(--muted-foreground)",
}

// Without a recorded demand time, the demand is read on the temperature: a zone heats while its
// temperature rises from below its setpoint, cools while it falls from above it.
/** A rise or a fall smaller than this (°) is noise (the thermostats count in tenths). */
const STEP = 0.3
/** A rise counts as heating when it starts at most this far above the setpoint (°)… */
const ABOVE = 0.2
/** …a fall counts as cooling in auto mode only from this far above it (°): further than the
 * overshoot of a heating, which cools down by itself. */
const AUTO_ABOVE = 1

// Airzone operation modes.
const COOLING = 2
const HEATING = [3, 6]
const AUTO = 7

const kindOf = (r: Reading) =>
  !r.is_on || r.room_temp === null
    ? null
    : HEATING.includes(r.mode ?? 0)
      ? "heat"
      : r.mode === COOLING
        ? "cool"
        : r.mode === AUTO
          ? "auto"
          : null

/**
 * Where the temperature rises or falls steadily, from reading `i` to the next one: a run of steps
 * the same way (flat steps inside it are kept, those at its ends are not) of `STEP` at least.
 */
function trends(readings: Reading[]): ("up" | "down" | null)[] {
  const out: ("up" | "down" | null)[] = readings.map(() => null)
  const temp = (i: number) => readings[i]?.room_temp ?? null
  const together = (i: number) =>
    temp(i) !== null &&
    temp(i + 1) !== null &&
    kindOf(readings[i]) !== null &&
    kindOf(readings[i]) === kindOf(readings[i + 1])
  for (const way of [1, -1] as const) {
    let i = 0
    while (i < readings.length - 1) {
      // The first step the right way.
      if (!together(i) || (temp(i + 1)! - temp(i)!) * way <= 0) {
        i++
        continue
      }
      let end = i + 1
      let last = i + 1
      while (together(end) && (temp(end + 1)! - temp(end)!) * way >= 0) {
        end++
        if ((temp(end)! - temp(end - 1)!) * way > 0) last = end
      }
      if ((temp(last)! - temp(i)!) * way >= STEP - 1e-9)
        for (let k = i; k < last; k++) out[k] = way > 0 ? "up" : "down"
      i = end
    }
  }
  return out
}

/**
 * The state of each reading until the next one. In demand comes from the demand time recorded with
 * the next reading (it covers the time since this one) when the installation records it, else from
 * the temperature: rising from below the setpoint is heating, falling from above it cooling. Fan
 * and dry modes, and gaps, have no state.
 */
export function zoneStates(readings: Reading[]): (ZoneState | null)[] {
  const way = trends(readings)
  // Where each rise or fall started: it decides whether it is the zone's doing.
  const from: (number | null)[] = []
  readings.forEach((r, i) => {
    from.push(
      way[i] && way[i] === way[i - 1] ? from[i - 1] : (r.room_temp ?? null)
    )
  })
  return readings.map((r, i) => {
    if (r.is_on === null || r.room_temp === null) return null
    if (!r.is_on) return "off"
    const kind = kindOf(r)
    if (!kind) return null
    const recorded = readings[i + 1]?.demand_secs ?? null
    const sp = r.setpoint ?? r.room_temp
    const start = from[i] ?? r.room_temp
    if (recorded !== null) {
      if (recorded <= 0) return "comfort"
      if (kind === "heat") return "heating"
      if (kind === "cool") return "cooling"
      return r.room_temp < sp ? "heating" : "cooling"
    }
    const heats = way[i] === "up" && start <= sp + ABOVE
    const cools =
      way[i] === "down" && start >= sp + (kind === "auto" ? AUTO_ABOVE : -ABOVE)
    if (kind !== "cool" && heats) return "heating"
    if (kind !== "heat" && cools) return "cooling"
    return "comfort"
  })
}

/** Whether the states come from the recorded demand time rather than from the temperature. */
export function measuredStates(readings: Reading[]): boolean {
  return readings.some((r) => r.demand_secs !== null)
}
