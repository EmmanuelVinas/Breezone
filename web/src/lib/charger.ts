import { t } from "@/lib/i18n"

/** States of a charger (`ev.charger@1`), whatever its brand. */
export const CHARGER_STATUS: Record<string, string> = {
  available: t("Disponible"),
  plugged: t("Câble branché"),
  charging: t("En charge"),
  fault: t("Défaut"),
}

export const CHARGER_TONE: Record<string, string> = {
  available: "var(--success)",
  plugged: "var(--warning)",
  charging: "var(--primary)",
  fault: "var(--destructive)",
  paused: "var(--fan)",
}
