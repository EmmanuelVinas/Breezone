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

/** Without a recorded demand time, a zone this far from its setpoint is taken as in demand (°). */
const MARGIN = 0.5

// Airzone operation modes.
const COOLING = 2
const HEATING = [3, 6]
const AUTO = 7

/**
 * The state of each reading until the next one. In demand comes from the demand time recorded with
 * the next reading (it covers the time since this one) when the installation records it, else from
 * how far the temperature is from the setpoint. Fan and dry modes, and gaps, have no state.
 */
export function zoneStates(readings: Reading[]): (ZoneState | null)[] {
  return readings.map((r, i) => {
    if (r.is_on === null || r.room_temp === null) return null
    if (!r.is_on) return "off"
    const mode = r.mode ?? 0
    if (mode !== COOLING && !HEATING.includes(mode) && mode !== AUTO)
      return null
    const next = readings[i + 1]
    const sp = r.setpoint ?? r.room_temp
    const recorded = next?.demand_secs ?? null
    const tooCold = r.room_temp < sp - MARGIN
    const tooHot = r.room_temp > sp + MARGIN
    const demand = recorded !== null ? recorded > 0 : tooCold || tooHot
    if (!demand) return "comfort"
    if (HEATING.includes(mode)) return "heating"
    if (mode === COOLING) return "cooling"
    return r.room_temp < sp ? "heating" : "cooling"
  })
}

/** Whether the states come from the recorded demand time rather than from the temperature. */
export function measuredStates(readings: Reading[]): boolean {
  return readings.some((r) => r.demand_secs !== null)
}
