import type { LucideIcon } from "lucide-react"
import {
  Droplets,
  Fan,
  Flame,
  Heater,
  Power,
  Snowflake,
  Sparkles,
} from "lucide-react"
import type { AirQualityLevel, Mode, Zone } from "@/lib/api"
import { lang, locale, t } from "@/lib/i18n"

export const MODE_LABEL: Record<Mode, string> = {
  stop: t("Arrêt"),
  cooling: t("Froid"),
  heating: t("Chaud"),
  fan: t("Ventilation"),
  dry: t("Déshumidification"),
  aux_heating: t("Appoint"),
  auto: t("Auto"),
}

export const MODE_ICON: Record<Mode, LucideIcon> = {
  stop: Power,
  cooling: Snowflake,
  heating: Flame,
  fan: Fan,
  dry: Droplets,
  aux_heating: Heater,
  auto: Sparkles,
}

/** Tailwind classes (text / background tint) per mode, using the theme mode colors. */
export const MODE_TONE: Record<
  Mode,
  { text: string; bg: string; ring: string }
> = {
  stop: { text: "text-muted-foreground", bg: "bg-muted", ring: "ring-border" },
  cooling: { text: "text-cool", bg: "bg-cool/15", ring: "ring-cool/40" },
  heating: { text: "text-heat", bg: "bg-heat/15", ring: "ring-heat/40" },
  fan: { text: "text-fan", bg: "bg-fan/15", ring: "ring-fan/40" },
  dry: { text: "text-dry", bg: "bg-dry/15", ring: "ring-dry/40" },
  aux_heating: { text: "text-heat", bg: "bg-heat/15", ring: "ring-heat/40" },
  auto: { text: "text-primary", bg: "bg-primary/15", ring: "ring-primary/40" },
}

export const AIR_QUALITY_LABEL: Record<AirQualityLevel, string> = {
  good: t("Bonne"),
  regular: t("Moyenne"),
  bad: t("Mauvaise"),
}

/** Text color class and chart color of each air quality level. */
export const AIR_QUALITY_TONE: Record<
  AirQualityLevel,
  { text: string; color: string }
> = {
  good: { text: "text-success", color: "var(--success)" },
  regular: { text: "text-warning", color: "var(--warning)" },
  bad: { text: "text-destructive", color: "var(--destructive)" },
}

export const DAY_LABEL = [
  t("Lundi"),
  t("Mardi"),
  t("Mercredi"),
  t("Jeudi"),
  t("Vendredi"),
  t("Samedi"),
  t("Dimanche"),
]
export const DAY_SHORT = DAY_LABEL.map((d) => d.charAt(0))

export function formatTemp(v: number | null | undefined, digits = 1) {
  if (v === null || v === undefined) return "–"
  const n = v.toFixed(digits)
  return `${lang() === "en" ? n : n.replace(".", ",")}°`
}

export function zoneKey(z: {
  installation_id: number
  system_id: number
  zone_id: number
}) {
  return `${z.installation_id}-${z.system_id}-${z.zone_id}`
}

/** Effective tone of a zone: muted when off. */
export function zoneTone(z: Zone) {
  if (!z.on || !z.mode) return MODE_TONE.stop
  return MODE_TONE[z.mode]
}

const rtf = new Intl.RelativeTimeFormat(locale(), { numeric: "auto" })

export function relativeTime(ts: number | null | undefined) {
  if (!ts) return t("jamais")
  const diff = ts - Date.now() / 1000
  const abs = Math.abs(diff)
  if (abs < 45) return diff < 0 ? t("à l'instant") : t("dans un instant")
  if (abs < 3600) return rtf.format(Math.round(diff / 60), "minute")
  if (abs < 86400) return rtf.format(Math.round(diff / 3600), "hour")
  return rtf.format(Math.round(diff / 86400), "day")
}

export function dateTime(ts: number) {
  return new Date(ts * 1000).toLocaleString(locale(), {
    dateStyle: "short",
    timeStyle: "short",
  })
}

export function time(ts: number) {
  return new Date(ts * 1000).toLocaleTimeString(locale(), {
    hour: "2-digit",
    minute: "2-digit",
  })
}

/** Groups zones by their Breezone group, keeping server order. */
export function groupZones(zones: Zone[]) {
  const groups: { name: string | null; zones: Zone[] }[] = []
  for (const z of zones) {
    let g = groups.find((x) => x.name === z.group)
    if (!g) {
      g = { name: z.group, zones: [] }
      groups.push(g)
    }
    g.zones.push(z)
  }
  return groups
}

/**
 * The master zone of each system (it carries the system's mode), and where its mode selector goes:
 * on the row of the group holding all the system's zones, or in the installation header when the
 * system spans several groups.
 */
export function systemModePlacement(zones: Zone[]) {
  const masters = zones.filter(
    (z, i, all) =>
      z.is_master &&
      z.modes.length > 0 &&
      all.findIndex((o) => o.is_master && o.system_id === z.system_id) === i
  )
  const byGroup = new Map<string | null, Zone[]>()
  const header: Zone[] = []
  for (const m of masters) {
    const groups = new Set(
      zones.filter((z) => z.system_id === m.system_id).map((z) => z.group)
    )
    if (groups.size === 1) {
      const [group] = groups
      byGroup.set(group, [...(byGroup.get(group) ?? []), m])
    } else {
      header.push(m)
    }
  }
  return { byGroup, header, systems: masters.length }
}

export function clampSetpoint(z: Zone, value: number) {
  const min = z.min_temp ?? 15
  const max = z.max_temp ?? 30
  const step = z.step || 0.5
  return Math.min(max, Math.max(min, Math.round(value / step) * step))
}

/** Where an installation is reached: its WebServer address, or the Breezone server relaying it. */
export function installationAddress(i: {
  host: string
  port: number
  node?: { name: string }
}) {
  return i.node ? t("via {name}", { name: i.node.name }) : `${i.host}:${i.port}`
}

/** A duration in seconds: "45 min", "6 h 05". */
export function duration(secs: number) {
  const minutes = Math.round(secs / 60)
  if (minutes < 60) return t("{n} min", { n: minutes })
  const h = Math.floor(minutes / 60)
  const m = minutes % 60
  return m ? `${h} h ${String(m).padStart(2, "0")}` : `${h} h`
}

/** A share between 0 and 1 as a whole percentage: "82 %". */
export function percent(v: number | null | undefined) {
  if (v === null || v === undefined) return "–"
  return lang() === "en"
    ? `${Math.round(v * 100)}%`
    : `${Math.round(v * 100)} %`
}
