import { createElement } from "react"
import type { LucideIcon, LucideProps } from "lucide-react"
import {
  AppWindow,
  Blinds,
  Box,
  Cctv,
  CloudSun,
  DoorOpen,
  Fan,
  Gamepad2,
  Gauge,
  House,
  Lightbulb,
  PlugZap,
  Power,
  SolarPanel,
  Sparkles,
  Thermometer,
  Wind,
  Zap,
} from "lucide-react"
import type { DeviceView } from "@/lib/api"
import { KINDS, TRAIT_KINDS, type KindId } from "@/lib/traits.generated"
import { t } from "@/lib/i18n"

/** A type of device, as lists group them (same icons as the menu). */
export type DeviceType = { id: string; label: string; icon: LucideIcon }

/** In the order of `KINDS` (contracts/ui.json). */
export const TYPES: DeviceType[] = [
  { id: "climate", label: t("Climatisation"), icon: Thermometer },
  { id: "charger", label: t("Recharge"), icon: PlugZap },
  { id: "light", label: t("Lumières"), icon: Lightbulb },
  { id: "shutter", label: t("Volets"), icon: Blinds },
  { id: "window", label: t("Fenêtres"), icon: AppWindow },
  { id: "door", label: t("Portes"), icon: DoorOpen },
  { id: "ventilation", label: t("Ventilation"), icon: Fan },
  { id: "room-air", label: t("Ventilation des pièces"), icon: Wind },
  { id: "switch", label: t("Prises et relais"), icon: Power },
  { id: "solar", label: t("Solaire"), icon: SolarPanel },
  { id: "energy", label: t("Énergie"), icon: Zap },
  { id: "sensor", label: t("Capteurs"), icon: Gauge },
  { id: "camera", label: t("Caméras"), icon: Cctv },
  { id: "remote", label: t("Télécommandes"), icon: Gamepad2 },
  { id: "other", label: t("Autres"), icon: Box },
]

/** What is not a device in the lists of the automations: the home as a whole, the scenes. */
export const HOME_TYPE: DeviceType = {
  id: "home",
  label: t("Maison"),
  icon: House,
}
export const WEATHER_TYPE: DeviceType = {
  id: "weather",
  label: t("Météo"),
  icon: CloudSun,
}
export const SCENE_TYPE: DeviceType = {
  id: "scene",
  label: t("Scènes"),
  icon: Sparkles,
}

/**
 * The type of a device: its kind when it names one (a shutter, a window…), else the first type in
 * list order among those of its traits (`ui.kind` in the catalog), else « other ».
 */
export function deviceType(d: Pick<DeviceView, "kind" | "traits">): DeviceType {
  const direct = TYPES.find((x) => x.id === d.kind && x.id !== "other")
  if (direct) return direct
  const kinds = [d.kind, ...d.traits.map((t) => t.split("@")[0])]
    .map((name) => TRAIT_KINDS[name])
    .filter((k): k is string => !!k)
  const order = (id: string) => {
    const i = KINDS.indexOf(id as KindId)
    return i < 0 ? KINDS.length : i
  }
  const id = kinds.sort((a, b) => order(a) - order(b))[0] ?? "other"
  return TYPES.find((x) => x.id === id) ?? TYPES[TYPES.length - 1]
}

/** The order of the types in a list. */
export function typeOrder(type: DeviceType) {
  return TYPES.indexOf(type)
}

/** The icon of a type of device, as a component declared once. */
export function DeviceTypeIcon({
  of,
  ...props
}: { of: DeviceType } & LucideProps) {
  return createElement(of.icon, props)
}
