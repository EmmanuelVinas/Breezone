import type { DayColor } from "@/lib/api"
import { t } from "@/lib/i18n"

export const TEMPO_COLOR: Record<DayColor, string> = {
  blue: "#3b82f6",
  white: "#e5e7eb",
  red: "#ef4444",
}

export const TEMPO_LABEL: Record<DayColor, string> = {
  blue: t("Bleu"),
  white: t("Blanc"),
  red: t("Rouge"),
}
