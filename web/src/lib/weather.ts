import type { LucideIcon } from "lucide-react"
import {
  Cloud,
  CloudDrizzle,
  CloudFog,
  CloudLightning,
  CloudRain,
  CloudSnow,
  CloudSun,
  Sun,
} from "lucide-react"
import { t } from "@/lib/i18n"

export type WeatherHour = {
  ts: number
  temperature: number | null
  code: number | null
  precipitation_probability: number | null
}

export type WeatherReport = {
  place: string
  temperature: number | null
  apparent: number | null
  code: number | null
  precipitation: number | null
  wind: number | null
  today_min: number | null
  today_max: number | null
  tomorrow_min: number | null
  tomorrow_max: number | null
  tomorrow_code: number | null
  hours: WeatherHour[]
  read_at: number
}

export type WeatherView = {
  weather: WeatherReport | null
  sun: { sunrise: number | null; sunset: number | null } | null
  /** The server reads the weather (`BREEZONE_WEATHER`); the sun is computed anyway. */
  online?: boolean
}

/** The look of a WMO weather code: an icon and a few words. */
export function weatherLook(code: number | null | undefined): {
  icon: LucideIcon
  label: string
} {
  const c = code ?? 0
  if (c === 0) return { icon: Sun, label: t("Ensoleillé") }
  if (c <= 2) return { icon: CloudSun, label: t("Éclaircies") }
  if (c === 3) return { icon: Cloud, label: t("Couvert") }
  if (c <= 48) return { icon: CloudFog, label: t("Brouillard") }
  if (c <= 57) return { icon: CloudDrizzle, label: t("Bruine") }
  if (c <= 67 || (c >= 80 && c <= 82))
    return { icon: CloudRain, label: t("Pluie") }
  if (c <= 77 || c === 85 || c === 86)
    return { icon: CloudSnow, label: t("Neige") }
  return { icon: CloudLightning, label: t("Orage") }
}
