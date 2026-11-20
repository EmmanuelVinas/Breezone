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
import { t } from "@/lib/i18n"

/** A type of device, as lists group them (same icons as the menu). */
export type DeviceType = { id: string; label: string; icon: LucideIcon }

const TYPES: DeviceType[] = [
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

/** The type of a device: from its kind, else from its traits. */
export function deviceType(d: Pick<DeviceView, "kind" | "traits">): DeviceType {
  const has = (prefix: string) =>
    d.kind.startsWith(prefix) || d.traits.some((x) => x.startsWith(prefix))
  const id =
    d.kind === "shutter" || d.kind === "window" || d.kind === "door"
      ? d.kind
      : has("climate.")
        ? "climate"
        : has("ev.charger")
          ? "charger"
          : has("light")
            ? "light"
            : has("cover.")
              ? "shutter"
              : has("ventilation.unit")
                ? "ventilation"
                : has("ventilation.room")
                  ? "room-air"
                  : has("switch.")
                    ? "switch"
                    : has("solar.")
                      ? "solar"
                      : has("energy.")
                        ? "energy"
                        : has("environment.")
                          ? "sensor"
                          : has("camera")
                            ? "camera"
                            : d.kind === "remote" || has("button")
                              ? "remote"
                              : "other"
  return TYPES.find((x) => x.id === id)!
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
