import type {
  DashboardBlock,
  DashboardLayout,
  DashboardSize,
  DeviceView,
  Room,
  TraitField,
  TraitInfo,
} from "@/lib/api"
import { deviceType, typeOrder } from "@/lib/device-icons"
import { lang, t } from "@/lib/i18n"

/** The kinds of blocks of the overview, as the server knows them. */
export type BlockType =
  "energy" | "surplus" | "group" | "device" | "chart" | "weather" | "scenes"

export type EnergyFlow = "solar" | "home" | "battery" | "grid"

export const BLOCK_LABEL: Record<BlockType, string> = {
  energy: t("Mesure d'énergie"),
  surplus: t("Surplus solaire"),
  group: t("Pièce ou type d'appareils"),
  device: t("Appareil"),
  chart: t("Graphique"),
  weather: t("Météo"),
  scenes: t("Scènes"),
}

export const FLOW_LABEL: Record<EnergyFlow, string> = {
  solar: t("Solaire"),
  home: t("Maison"),
  battery: t("Batterie"),
  grid: t("Réseau"),
}

export const SIZE_LABEL: Record<DashboardSize, string> = {
  quarter: "¼",
  half: "½",
  full: t("Pleine largeur"),
}

/** The size a new block of this type starts with. */
export const DEFAULT_SIZE: Record<BlockType, DashboardSize> = {
  energy: "quarter",
  surplus: "full",
  group: "half",
  device: "quarter",
  chart: "half",
  weather: "full",
  scenes: "full",
}

export const newId = () => Math.random().toString(36).slice(2, 10)

export const block = (
  type: BlockType,
  config: Record<string, unknown> = {},
  size: DashboardSize = DEFAULT_SIZE[type]
): DashboardBlock => ({ id: newId(), type, size, config })

/** What a group block gathers: the devices of a room (`null`: without a room), or of a type. */
export type GroupConfig = {
  room_id?: number | null
  type?: string
  /** Devices left out of the card. */
  hidden?: number[]
}

export function groupDevices(config: GroupConfig, devices: DeviceView[]) {
  const hidden = new Set(config.hidden ?? [])
  return devices
    .filter((d) =>
      config.type !== undefined
        ? deviceType(d).id === config.type
        : d.room_id === (config.room_id ?? null)
    )
    .filter((d) => !hidden.has(d.id))
}

/** Devices worth a line on a card: what a remote or a panel of a solar inverter is not. */
export function shown(d: DeviceView) {
  return (
    !["remote", "other"].includes(deviceType(d).id) &&
    !d.traits.includes("solar.panel@1")
  )
}

/**
 * The overview composed from what the home has, when nobody arranged one: the energy figures of
 * the measures it has, the solar surplus when it is steered, then a card per room (or per type of
 * device), the weather and the scenes.
 */
export function generate(
  by: "room" | "type",
  home: {
    devices: DeviceView[]
    rooms: Room[]
    roles: Partial<
      Record<"grid" | "solar" | "battery" | "consumption", boolean>
    >
    surplus: boolean
    scenes: boolean
  }
): DashboardLayout {
  const blocks: DashboardBlock[] = []
  const { roles } = home
  if (roles.solar) blocks.push(block("energy", { flow: "solar" }))
  if (roles.grid || roles.consumption)
    blocks.push(block("energy", { flow: "home" }))
  if (roles.battery) blocks.push(block("energy", { flow: "battery" }))
  if (roles.grid) blocks.push(block("energy", { flow: "grid" }))
  if (home.surplus) blocks.push(block("surplus"))
  const devices = home.devices.filter(shown)
  if (by === "room") {
    for (const r of home.rooms)
      if (devices.some((d) => d.room_id === r.id))
        blocks.push(block("group", { room_id: r.id }))
    if (devices.some((d) => d.room_id === null))
      blocks.push(block("group", { room_id: null }))
  } else {
    const types = [
      ...new Map(
        devices.map((d) => [deviceType(d).id, deviceType(d)])
      ).values(),
    ]
    types.sort((a, b) => typeOrder(a) - typeOrder(b))
    for (const type of types) blocks.push(block("group", { type: type.id }))
  }
  blocks.push(block("weather"))
  if (home.scenes) blocks.push(block("scenes"))
  return { version: 1, blocks }
}

/** The fields of a device worth a chart: its measures kept in history, with their label. */
export function chartFields(d: DeviceView, traits: TraitInfo[]) {
  const out: { name: string; label: string; unit?: string }[] = []
  for (const id of d.traits) {
    const trait = traits.find((x) => `${x.trait}@${x.version}` === id)
    for (const f of trait?.state ?? [])
      if (
        numeric(f) &&
        f.history !== false &&
        !out.some((o) => o.name === f.name)
      )
        out.push({ name: f.name, label: f.label[lang()], unit: f.unit })
  }
  return out
}

const numeric = (f: TraitField) => f.type === "number" || f.type === "integer"
