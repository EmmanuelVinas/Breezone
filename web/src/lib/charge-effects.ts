// What the automations tell a charger, for the friezes of the web.
import type { ChargeEffect } from "@/lib/api"
import { t } from "@/lib/i18n"

/** The colours of a frieze: a profile by its own, the rest by what it is. */
export function effectColor(
  e: ChargeEffect | null | undefined,
  profiles: { id: number; color: string | null }[]
) {
  if (!e) return "transparent"
  switch (e.mode) {
    case "profile":
      return (
        profiles.find((p) => p.id === e.profile_id)?.color ?? "var(--primary)"
      )
    case "solar":
      return "var(--warning)"
    case "now":
      return "var(--primary)"
    case "off":
      return "var(--muted)"
    case "free":
      return "transparent"
  }
}

/** What a stretch of a frieze tells the charger, in a few words. */
export function effectText(
  e: ChargeEffect | null | undefined,
  profiles: { id: number; name: string }[]
) {
  if (!e) return ""
  switch (e.mode) {
    case "profile":
      return (
        profiles.find((p) => p.id === e.profile_id)?.name ??
        t("profil supprimé")
      )
    case "solar":
      return t("Surplus solaire")
    case "now":
      return t("Charger maintenant")
    case "off":
      return t("Ne pas charger")
    case "free":
      return t("La borne décide")
  }
}
