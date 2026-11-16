import { t } from "@/lib/i18n"

/** States of a charger (`ev.charger@1`), whatever its brand, pause included. */
export const CHARGER_STATUS: Record<string, string> = {
  available: t("Disponible"),
  plugged: t("Câble branché"),
  charging: t("En charge"),
  paused: t("En pause"),
  fault: t("Défaut"),
}

export const CHARGER_TONE: Record<string, string> = {
  available: "var(--success)",
  plugged: "var(--warning)",
  charging: "var(--primary)",
  fault: "var(--destructive)",
  paused: "var(--fan)",
}

/**
 * The state to show: paused only matters with a car plugged in, and a paused charger may still
 * say "charging" at 0 W.
 */
export function chargerStatus(s: {
  status?: string
  vehicle?: boolean | null
  paused?: boolean
}) {
  const plugged =
    s.vehicle === true || s.status === "plugged" || s.status === "charging"
  if (s.status === "fault") return "fault"
  return s.paused && plugged ? "paused" : (s.status ?? "fault")
}
