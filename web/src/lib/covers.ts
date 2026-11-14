import type { DeviceView } from "@/lib/api"
import { t } from "@/lib/i18n"

/** The state of a shutter, a blind, a roof window or a door, as `cover.position@1` describes it. */
export type CoverState = {
  position?: number
  target?: number
  moving?: "opening" | "closing" | "stopped"
}

export const isCover = (d: DeviceView) => d.traits.includes("cover.position@1")

export const coverState = (d: DeviceView) => (d.state ?? {}) as CoverState

/** What the device is: its driver says it (a HomeKit shutter, a VELUX window…). */
export function coverKind(d: DeviceView) {
  if (d.kind === "window") return t("Fenêtre de toit")
  if (d.kind === "door") return t("Porte")
  return t("Volet roulant")
}

export function coverText(s: CoverState) {
  if (s.moving === "opening")
    return typeof s.target === "number"
      ? t("S'ouvre… {n} %", { n: s.target })
      : t("S'ouvre…")
  if (s.moving === "closing")
    return typeof s.target === "number"
      ? t("Se ferme… {n} %", { n: s.target })
      : t("Se ferme…")
  if (typeof s.position !== "number") return "–"
  if (s.position <= 0) return t("Fermé")
  if (s.position >= 100) return t("Ouvert")
  return t("Ouvert à {n} %", { n: s.position })
}

export const isOpen = (d: DeviceView) =>
  d.available && (coverState(d).position ?? 0) > 0
