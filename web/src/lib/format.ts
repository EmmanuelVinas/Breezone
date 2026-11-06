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
import type { Mode, Zone } from "@/lib/api"

export const MODE_LABEL: Record<Mode, string> = {
  stop: "Arrêt",
  cooling: "Froid",
  heating: "Chaud",
  fan: "Ventilation",
  dry: "Déshumidification",
  aux_heating: "Appoint",
  auto: "Auto",
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

export const DAY_SHORT = ["L", "M", "M", "J", "V", "S", "D"]
export const DAY_LABEL = [
  "Lundi",
  "Mardi",
  "Mercredi",
  "Jeudi",
  "Vendredi",
  "Samedi",
  "Dimanche",
]

export function formatTemp(v: number | null | undefined, digits = 1) {
  if (v === null || v === undefined) return "–"
  return `${v.toFixed(digits).replace(".", ",")}°`
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

const rtf = new Intl.RelativeTimeFormat("fr", { numeric: "auto" })

export function relativeTime(ts: number | null | undefined) {
  if (!ts) return "jamais"
  const diff = ts - Date.now() / 1000
  const abs = Math.abs(diff)
  if (abs < 45) return diff < 0 ? "à l'instant" : "dans un instant"
  if (abs < 3600) return rtf.format(Math.round(diff / 60), "minute")
  if (abs < 86400) return rtf.format(Math.round(diff / 3600), "hour")
  return rtf.format(Math.round(diff / 86400), "day")
}

export function dateTime(ts: number) {
  return new Date(ts * 1000).toLocaleString("fr-FR", {
    dateStyle: "short",
    timeStyle: "short",
  })
}

export function time(ts: number) {
  return new Date(ts * 1000).toLocaleTimeString("fr-FR", {
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
  return i.node ? `via ${i.node.name}` : `${i.host}:${i.port}`
}
