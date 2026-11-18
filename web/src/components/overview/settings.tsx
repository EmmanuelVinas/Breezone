import { useMemo, useState } from "react"
import { ArrowDown, ArrowUp, ChevronLeft, Plus, X } from "lucide-react"
import type { LucideIcon } from "lucide-react"
import type {
  Charted,
  DashboardBlock,
  DashboardPage,
  DashboardSize,
  EnergyLive,
  SurplusView,
  DeviceView,
  Installation,
  Room,
  Routine,
  TraitInfo,
} from "@/lib/api"
import { deviceType, typeOrder } from "@/lib/device-icons"
import { t } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import { ROOM_ICONS, roomIconName } from "@/lib/room-icons"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  DEFAULT_KPIS,
  KPI_IDS,
  KPI_LABEL,
  type KpiId,
} from "@/components/energy/sections"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Label } from "@/components/ui/label"
import { Switch } from "@/components/ui/switch"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import type { BlockType, EnergyFlow } from "./model"
import { BlockView } from "./blocks"
import {
  BLOCK_GROUP,
  BLOCK_LABEL,
  block as newBlock,
  chartFields,
  chartSeries,
  FLOW_LABEL,
  measuredFlows,
  OUTDOOR,
  shown,
  zoneOf,
} from "./model"

type Home = {
  devices: DeviceView[]
  rooms: Room[]
  routines: Routine[]
  traits: TraitInfo[]
  charted: Charted[]
  live: EnergyLive | undefined
  surplus: SurplusView | undefined
  priced: boolean
  /** This server measures energy (the parts of the energy page have something to show). */
  energy: boolean
  /** The installation shown: its zones. */
  installation?: Installation
  /** A Linky account gives what Enedis measured. */
  linky?: boolean
}

/** A choice of a list: its value, its words, its icon, and the heading it is listed under. */
type Choice = {
  value: string
  label: string
  icon?: LucideIcon
  group?: string
}

function Field({
  label,
  children,
}: {
  label: string
  children: React.ReactNode
}) {
  return (
    <div className="grid gap-2">
      <Label>{label}</Label>
      {children}
    </div>
  )
}

function Pick({
  value,
  onChange,
  choices,
  placeholder,
}: {
  value: string | undefined
  onChange: (v: string) => void
  choices: Choice[]
  placeholder?: string
}) {
  const groups = [...new Set(choices.map((c) => c.group ?? ""))]
  const item = (c: Choice) => (
    <SelectItem key={c.value} value={c.value}>
      <span className="inline-flex items-center gap-2">
        {c.icon && (
          <c.icon className="size-4 text-muted-foreground" aria-hidden />
        )}
        {c.label}
      </span>
    </SelectItem>
  )
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger className="w-full">
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent>
        {groups.map((g) =>
          g ? (
            <SelectGroup key={g}>
              <SelectLabel>{g}</SelectLabel>
              {choices.filter((c) => c.group === g).map(item)}
            </SelectGroup>
          ) : (
            choices.filter((c) => !c.group).map(item)
          )
        )}
      </SelectContent>
    </Select>
  )
}

/** Devices as choices, grouped by type (in the order of the menu), with the icon of their type. */
function deviceChoices(devices: DeviceView[]): Choice[] {
  return [...devices]
    .sort(
      (a, b) =>
        typeOrder(deviceType(a)) - typeOrder(deviceType(b)) ||
        a.name.localeCompare(b.name)
    )
    .map((d) => ({
      value: String(d.id),
      label: d.name,
      icon: deviceType(d).icon,
      group: deviceType(d).label,
    }))
}

/** The fields of a device a chart can show: those with a history, with their label. */
function chartedFields(d: DeviceView, home: Home) {
  const has = new Set(
    home.charted.find((c) => c.device_id === d.id)?.fields ?? []
  )
  return chartFields(d, home.traits).filter((f) => has.has(f.name))
}

/** The blocks worth adding: those with something to show in this home. */
function available(home: Home): BlockType[] {
  return (Object.keys(BLOCK_LABEL) as BlockType[]).filter((b) => {
    if (b === "energy") return measuredFlows(home.live?.roles).length > 0
    if (b === "surplus") return (home.surplus?.loads.length ?? 0) > 0
    if (b === "chart") return home.charted.length > 0
    if (b === "scenes") return home.routines.length > 0
    if (b === "price") return home.priced
    // The parts of the energy page: with a measure of the energy of this server.
    if (b.startsWith("energy_")) return home.energy
    // The parts of the other pages: with the devices they show.
    const has = (trait: string) =>
      home.devices.some((d) => d.traits.includes(trait))
    if (b === "zone_history" || b === "air_quality")
      return home.devices.some((d) => zoneOf(d) !== undefined)
    if (b === "light_scenes") return has("light@1")
    if (b === "charger" || b === "charger_curve") return has("ev.charger@1")
    if (b.startsWith("solar_")) return has("solar.inverter@1")
    if (b === "ventilation_house") return has("ventilation.unit@1")
    if (b === "linky") return !!home.linky
    return true
  })
}

/** Whether a block has what it needs to show something. */
export function complete(b: DashboardBlock) {
  const c = b.config
  if (b.type === "device") return typeof c.device_id === "number"
  if (b.type === "zone_history" || b.type === "air_quality")
    return typeof c.device_id === "number"
  if (b.type === "chart")
    return (
      typeof c.device_id === "number" &&
      (c.view === "comfort" || chartSeries(c).length > 0)
    )
  if (b.type === "group") return "room_id" in c || "type" in c
  if (b.type === "energy" && Array.isArray(c.flows)) return c.flows.length > 0
  return true
}

/** The settings of one block: what it shows. */
function Settings({
  block,
  home,
  page,
  onChange,
}: {
  block: DashboardBlock
  home: Home
  page: DashboardPage
  onChange: (b: DashboardBlock) => void
}) {
  const c = block.config
  const set = (patch: Record<string, unknown>) =>
    onChange({ ...block, config: { ...c, ...patch } })
  const devices = home.devices.filter(shown)
  switch (block.type as BlockType) {
    case "energy": {
      // Only what the home measures (no battery offered without one).
      const all = measuredFlows(home.live?.roles)
      const split = Array.isArray(c.flows)
      const flows = new Set((c.flows as EnergyFlow[] | undefined) ?? [])
      return (
        <>
          <Field label={t("Affichage")}>
            <Pick
              value={split ? "split" : "one"}
              onChange={(v) =>
                onChange({
                  ...block,
                  config:
                    v === "split"
                      ? { flows: all }
                      : { flow: (c.flow as EnergyFlow | undefined) ?? all[0] },
                })
              }
              choices={[
                {
                  value: "one",
                  label: t("Une mesure, avec sa courbe du jour"),
                },
                { value: "split", label: t("Plusieurs mesures côte à côte") },
              ]}
            />
          </Field>
          {split ? (
            <Field label={t("Mesures")}>
              <div className="grid gap-2">
                {all.map((f) => (
                  <label key={f} className="flex items-center gap-2 text-sm">
                    <Checkbox
                      checked={flows.has(f)}
                      onCheckedChange={(v) =>
                        set({
                          flows: all.filter((x) =>
                            x === f ? v === true : flows.has(x)
                          ),
                        })
                      }
                    />
                    {FLOW_LABEL[f]}
                  </label>
                ))}
              </div>
            </Field>
          ) : (
            <Field label={t("Mesure")}>
              <Pick
                value={String(c.flow ?? all[0])}
                onChange={(flow) => set({ flow })}
                choices={all.map((f) => ({ value: f, label: FLOW_LABEL[f] }))}
              />
            </Field>
          )}
        </>
      )
    }
    case "group": {
      const types = [
        ...new Map(
          devices.map((d) => [deviceType(d).id, deviceType(d)])
        ).values(),
      ].sort((a, b) => typeOrder(a) - typeOrder(b))
      const key =
        c.type !== undefined
          ? `type:${c.type}`
          : "room_id" in c
            ? `room:${c.room_id ?? "none"}`
            : undefined
      const hidden = new Set((c.hidden as number[] | undefined) ?? [])
      const charts = (c.charts as Record<string, string> | undefined) ?? {}
      const members = devices.filter((d) =>
        c.type !== undefined
          ? deviceType(d).id === c.type
          : "room_id" in c && d.room_id === (c.room_id ?? null)
      )
      return (
        <>
          <Field label={t("Ce que la carte rassemble")}>
            <Pick
              value={key}
              placeholder={t("Choisir")}
              onChange={(k) => {
                const [kind, v] = k.split(":")
                onChange({
                  ...block,
                  config:
                    kind === "type"
                      ? { type: v }
                      : { room_id: v === "none" ? null : Number(v) },
                })
              }}
              choices={[
                ...home.rooms.map((r): Choice => ({
                  value: `room:${r.id}`,
                  label: r.name,
                  icon: ROOM_ICONS[roomIconName(r)],
                  group: t("Pièces"),
                })),
                {
                  value: "room:none",
                  label: t("Sans pièce"),
                  icon: ROOM_ICONS.house,
                  group: t("Pièces"),
                },
                ...types.map((ty): Choice => ({
                  value: `type:${ty.id}`,
                  label: ty.label,
                  icon: ty.icon,
                  group: t("Types d'appareils"),
                })),
              ]}
            />
          </Field>
          {members.length > 0 && (
            <Field label={t("Appareils montrés, et leur courbe")}>
              <div className="grid gap-2">
                {members.map((d) => {
                  const fields = chartedFields(d, home)
                  const Icon = deviceType(d).icon
                  return (
                    <div key={d.id} className="flex items-center gap-2 text-sm">
                      <Checkbox
                        checked={!hidden.has(d.id)}
                        onCheckedChange={(v) => {
                          const next = new Set(hidden)
                          if (v) next.delete(d.id)
                          else next.add(d.id)
                          set({ hidden: [...next] })
                        }}
                        aria-label={d.name}
                      />
                      <Icon
                        className="size-4 text-muted-foreground"
                        aria-hidden
                      />
                      <span className="min-w-0 flex-1 truncate">{d.name}</span>
                      {fields.length > 0 && !hidden.has(d.id) && (
                        <div className="w-40">
                          <Pick
                            value={charts[String(d.id)] ?? "none"}
                            onChange={(v) => {
                              const next = { ...charts }
                              if (v === "none") delete next[String(d.id)]
                              else next[String(d.id)] = v
                              set({ charts: next })
                            }}
                            choices={[
                              { value: "none", label: t("Sans courbe") },
                              ...fields.map((f) => ({
                                value: f.name,
                                label: f.label,
                              })),
                            ]}
                          />
                        </div>
                      )}
                    </div>
                  )
                })}
              </div>
            </Field>
          )}
        </>
      )
    }
    case "device":
      return (
        <Field label={t("Appareil")}>
          <Pick
            value={c.device_id !== undefined ? String(c.device_id) : undefined}
            placeholder={t("Choisir")}
            onChange={(v) => set({ device_id: Number(v) })}
            choices={deviceChoices(devices)}
          />
        </Field>
      )
    case "linky":
      return (
        <Field label={t("Période")}>
          <Pick
            value={String(c.days ?? 30)}
            onChange={(v) => set({ days: Number(v) })}
            choices={[
              { value: "7", label: t("7 jours") },
              { value: "30", label: t("30 jours") },
              { value: "90", label: t("90 jours") },
              { value: "365", label: t("Année") },
            ]}
          />
        </Field>
      )
    case "zone_history":
    case "air_quality":
      return (
        <Field label={t("Zone")}>
          <Pick
            value={c.device_id !== undefined ? String(c.device_id) : undefined}
            placeholder={t("Choisir")}
            onChange={(v) => set({ device_id: Number(v) })}
            choices={deviceChoices(
              home.devices.filter((d) => zoneOf(d) !== undefined)
            )}
          />
        </Field>
      )
    case "charger":
    case "charger_curve": {
      const chargers = home.devices.filter((d) =>
        d.traits.includes("ev.charger@1")
      )
      // With one charger, nothing to choose: the block follows it.
      if (chargers.length < 2)
        return (
          <p className="text-sm text-muted-foreground">
            {t("Ce bloc n'a pas de réglage.")}
          </p>
        )
      return (
        <Field label={t("Borne")}>
          <Pick
            value={String(c.device_id ?? chargers[0].id)}
            onChange={(v) => set({ device_id: Number(v) })}
            choices={deviceChoices(chargers)}
          />
        </Field>
      )
    }
    case "chart": {
      const withData = home.devices.filter(
        (x) => chartedFields(x, home).length > 0
      )
      const d = home.devices.find((x) => x.id === c.device_id)
      const fields = d ? chartedFields(d, home) : []
      const zone = d ? zoneOf(d) : undefined
      const comfort = zone !== undefined && c.view === "comfort"
      const chosen = chartSeries(c)
      // The outdoor temperature goes with the temperatures of a device.
      const offered = [
        ...fields.map((f) => ({ name: f.name, label: f.label })),
        ...(fields.some((f) => f.unit === "°C")
          ? [{ name: OUTDOOR, label: t("Extérieur") }]
          : []),
      ]
      const label = (name: string) =>
        offered.find((f) => f.name === name)?.label ?? name
      const rest = offered.filter((f) => !chosen.includes(f.name))
      const setFields = (next: string[]) =>
        set({ fields: next, field: undefined })
      return (
        <>
          {withData.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              {t("Aucun appareil n'a encore d'historique.")}
            </p>
          ) : (
            <Field label={t("Appareil")}>
              <Pick
                value={
                  c.device_id !== undefined ? String(c.device_id) : undefined
                }
                placeholder={t("Choisir")}
                onChange={(v) => {
                  const next = home.devices.find((x) => x.id === Number(v))
                  const first = next ? chartedFields(next, home)[0] : undefined
                  set({
                    device_id: Number(v),
                    fields: first ? [first.name] : [],
                    field: undefined,
                    view: undefined,
                  })
                }}
                choices={deviceChoices(withData)}
              />
            </Field>
          )}
          {zone && (
            <Field label={t("Affichage")}>
              <Pick
                value={comfort ? "comfort" : "curves"}
                onChange={(v) =>
                  set({ view: v === "comfort" ? "comfort" : undefined })
                }
                choices={[
                  { value: "curves", label: t("Courbes des mesures") },
                  { value: "comfort", label: t("Confort jour par jour") },
                ]}
              />
            </Field>
          )}
          {!comfort && fields.length > 0 && (
            <Field label={t("Courbes")}>
              <div className="flex flex-wrap items-center gap-2">
                {chosen.map((name) => (
                  <span
                    key={name}
                    className="inline-flex items-center gap-1 rounded-full border py-1 pr-1 pl-3 text-sm"
                  >
                    {label(name)}
                    <button
                      type="button"
                      className="rounded-full p-0.5 text-muted-foreground hover:bg-muted hover:text-foreground"
                      aria-label={t("Retirer {name}", { name: label(name) })}
                      onClick={() =>
                        setFields(chosen.filter((x) => x !== name))
                      }
                    >
                      <X className="size-3.5" aria-hidden />
                    </button>
                  </span>
                ))}
                {rest.length > 0 && (
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button
                        variant="outline"
                        size="sm"
                        className="rounded-full"
                      >
                        <Plus /> {t("Ajouter une courbe")}
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="start">
                      {rest.map((f) => (
                        <DropdownMenuItem
                          key={f.name}
                          onSelect={() => setFields([...chosen, f.name])}
                        >
                          {f.label}
                        </DropdownMenuItem>
                      ))}
                    </DropdownMenuContent>
                  </DropdownMenu>
                )}
              </div>
            </Field>
          )}
          {d && (
            <Field label={t("Hauteur")}>
              <Pick
                value={c.tall === true ? "tall" : "compact"}
                onChange={(v) => set({ tall: v === "tall" || undefined })}
                choices={[
                  { value: "compact", label: t("Compacte") },
                  {
                    value: "tall",
                    label: comfort
                      ? t("Grande, comme sur la page de la zone")
                      : t("Grande"),
                  },
                ]}
              />
            </Field>
          )}
          {!comfort && (
            <Field label={t("Période")}>
              <Pick
                value={String(c.hours ?? 24)}
                onChange={(v) => set({ hours: Number(v) })}
                choices={[
                  { value: "24", label: t("24 heures") },
                  { value: "168", label: t("7 jours") },
                ]}
              />
            </Field>
          )}
        </>
      )
    }
    case "energy_kpis":
    case "energy_bars":
    case "energy_breakdown":
    case "energy_measures": {
      const period = (c.period as string | undefined) ?? "page"
      return (
        <>
          <Field label={t("Période")}>
            <Pick
              value={
                page === "energy" ? period : period === "page" ? "day" : period
              }
              onChange={(v) => set({ period: v === "page" ? undefined : v })}
              choices={[
                ...(page === "energy"
                  ? [{ value: "page", label: t("Celle choisie sur la page") }]
                  : []),
                { value: "day", label: t("Aujourd'hui") },
                { value: "week", label: t("7 jours") },
                { value: "month", label: t("30 jours") },
                { value: "year", label: t("Année") },
              ]}
            />
          </Field>
          {block.type === "energy_kpis" && (
            <>
              <KpiOrder
                value={(c.kpis as KpiId[] | undefined) ?? []}
                onChange={(kpis) =>
                  set({ kpis: kpis.length ? kpis : undefined })
                }
              />
              <label className="flex items-center justify-between gap-4 text-sm">
                <span>
                  {t("Courbe dans les tuiles")}
                  <span className="block text-muted-foreground">
                    {t(
                      "La production, la consommation, le réseau et le coût le long de la période."
                    )}
                  </span>
                </span>
                <Switch
                  checked={c.curves === true}
                  onCheckedChange={(v) => set({ curves: v || undefined })}
                />
              </label>
              <label className="flex items-center justify-between gap-4 text-sm">
                <span>
                  {t("Montants en euros")}
                  <span className="block text-muted-foreground">
                    {t(
                      "Le coût, les gains du solaire et ce que vaut chaque énergie, avec un tarif (Réglages › Électricité)."
                    )}
                  </span>
                </span>
                <Switch
                  checked={c.money !== false}
                  onCheckedChange={(v) => set({ money: v ? undefined : false })}
                />
              </label>
            </>
          )}
        </>
      )
    }
    case "scenes": {
      const ids = new Set((c.routine_ids as number[] | undefined) ?? [])
      return (
        <Field label={t("Scènes montrées (aucune cochée : toutes)")}>
          <div className="grid gap-2">
            {home.routines.map((r) => (
              <label key={r.id} className="flex items-center gap-2 text-sm">
                <Checkbox
                  checked={ids.has(r.id)}
                  onCheckedChange={(v) => {
                    const next = new Set(ids)
                    if (v) next.add(r.id)
                    else next.delete(r.id)
                    set({ routine_ids: [...next] })
                  }}
                />
                {r.name}
              </label>
            ))}
          </div>
        </Field>
      )
    }
    default:
      return (
        <p className="text-sm text-muted-foreground">
          {t("Ce bloc n'a pas de réglage.")}
        </p>
      )
  }
}

/** A component of the catalog: the block it adds, under the heading of its page. */
type Entry = {
  key: string
  group: string
  label: string
  block: DashboardBlock
}

/**
 * Every component this home can show, ready to add: each energy figure and each period figure on
 * its own, each room, each type, each device, each chart, the parts of the other pages.
 */
function catalog(home: Home): Entry[] {
  const out: Entry[] = []
  const add = (
    group: string,
    key: string,
    label: string,
    type: BlockType,
    config: Record<string, unknown> = {},
    size?: DashboardSize
  ) => out.push({ key, group, label, block: newBlock(type, config, size) })
  const roles = home.live?.roles
  const devices = home.devices.filter(shown)
  for (const type of available(home)) {
    const group = BLOCK_GROUP[type]
    switch (type) {
      case "energy": {
        const flows = measuredFlows(roles)
        for (const f of flows)
          add(group, `energy:${f}`, FLOW_LABEL[f], type, { flow: f })
        if (flows.length > 1)
          add(
            group,
            "energy:all",
            t("Plusieurs mesures côte à côte"),
            type,
            { flows },
            "half"
          )
        break
      }
      case "energy_kpis": {
        add(group, "kpis", BLOCK_LABEL[type], type)
        const has: Record<KpiId, boolean> = {
          production: !!roles?.solar,
          consumption: true,
          grid: !!roles?.grid,
          self: !!roles?.solar,
          autonomy: !!roles?.grid && !!(roles?.solar || roles?.battery),
          cost: home.priced,
          gains: home.priced && !!roles?.solar,
          balance: home.priced && !!roles?.solar,
          shares: !!roles?.solar,
          battery: !!roles?.battery,
        }
        for (const k of KPI_IDS)
          if (has[k])
            add(group, `kpi:${k}`, KPI_LABEL[k], type, { kpis: [k] }, "quarter")
        break
      }
      case "group": {
        for (const r of home.rooms)
          if (devices.some((d) => d.room_id === r.id))
            add(t("Pièces"), `room:${r.id}`, r.name, type, { room_id: r.id })
        if (devices.some((d) => d.room_id === null))
          add(t("Pièces"), "room:none", t("Sans pièce"), type, {
            room_id: null,
          })
        const types = [
          ...new Map(
            devices.map((d) => [deviceType(d).id, deviceType(d)])
          ).values(),
        ].sort((a, b) => typeOrder(a) - typeOrder(b))
        for (const ty of types)
          add(t("Types d'appareils"), `type:${ty.id}`, ty.label, type, {
            type: ty.id,
          })
        break
      }
      case "device":
        for (const d of devices)
          add(t("Appareils"), `device:${d.id}`, d.name, type, {
            device_id: d.id,
          })
        break
      case "chart":
        for (const d of home.devices) {
          const fields = chartedFields(d, home)
          if (fields.length)
            add(
              t("Graphiques"),
              `chart:${d.id}`,
              `${d.name} · ${fields[0].label}`,
              type,
              {
                device_id: d.id,
                fields: [fields[0].name],
                hours: 24,
              }
            )
        }
        break
      case "zone_history":
      case "air_quality":
        for (const d of home.devices)
          if (zoneOf(d) && (type === "zone_history" || measuresAir(d, home)))
            add(
              group,
              `${type}:${d.id}`,
              `${BLOCK_LABEL[type]} · ${d.name}`,
              type,
              { device_id: d.id }
            )
        break
      default:
        add(group, type, BLOCK_LABEL[type], type)
    }
  }
  return out
}

/**
 * The figures of the period a block shows, in their order: those ticked first, each moved up or
 * down; none ticked shows them all, in the usual order.
 */
function KpiOrder({
  value,
  onChange,
}: {
  value: KpiId[]
  onChange: (kpis: KpiId[]) => void
}) {
  const shown = value.length ? value : DEFAULT_KPIS
  const rest = KPI_IDS.filter((k) => !shown.includes(k))
  const move = (i: number, by: number) => {
    const next = [...shown]
    const [k] = next.splice(i, 1)
    next.splice(i + by, 0, k)
    onChange(next)
  }
  const row = (k: KpiId, i: number | null) => (
    <div key={k} className="flex items-center gap-2 text-sm">
      <Checkbox
        checked={i !== null}
        aria-label={KPI_LABEL[k]}
        onCheckedChange={(v) =>
          onChange(v === true ? [...shown, k] : shown.filter((x) => x !== k))
        }
      />
      <span className={cn("flex-1", i === null && "text-muted-foreground")}>
        {KPI_LABEL[k]}
      </span>
      {i !== null && (
        <>
          <Button
            variant="ghost"
            size="icon-sm"
            disabled={i === 0}
            aria-label={t("Monter")}
            onClick={() => move(i, -1)}
          >
            <ArrowUp />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            disabled={i === shown.length - 1}
            aria-label={t("Descendre")}
            onClick={() => move(i, 1)}
          >
            <ArrowDown />
          </Button>
        </>
      )}
    </div>
  )
  return (
    <Field label={t("Chiffres montrés, dans l'ordre")}>
      <div className="grid gap-1">
        {shown.map((k, i) => row(k, i))}
        {rest.map((k) => row(k, null))}
      </div>
    </Field>
  )
}

/** Whether the zone a device is measures its air (a card of its air has something to show). */
function measuresAir(d: DeviceView, home: Home) {
  const ref = zoneOf(d)
  return !!home.installation?.zones.some(
    (z) =>
      z.system_id === ref?.system_id &&
      z.zone_id === ref?.zone_id &&
      z.air_quality != null
  )
}

/** A block as it shows, smaller, to look at only (nothing in it can be clicked). */
function Preview({
  block,
  tall = false,
}: {
  block: DashboardBlock
  tall?: boolean
}) {
  return (
    <div
      inert
      aria-hidden
      className={cn(
        "relative overflow-hidden rounded-3xl",
        tall ? "max-h-96" : "h-44"
      )}
    >
      <div
        className="origin-top-left"
        style={
          tall ? undefined : { width: "166.667%", transform: "scale(0.6)" }
        }
      >
        <BlockView block={block} editing />
      </div>
      {!tall && (
        <div className="absolute inset-x-0 bottom-0 h-10 bg-gradient-to-t from-popover to-transparent" />
      )}
    </div>
  )
}

/**
 * Sets up a block. A new one is picked among the components of the home, by section on the
 * left and as they look on the right, then set up; one already on the page is set up at once.
 */
export function BlockDialog({
  initial,
  home,
  page,
  onDone,
  onClose,
}: {
  initial?: DashboardBlock
  home: Home
  page: DashboardPage
  onDone: (b: DashboardBlock) => void
  onClose: () => void
}) {
  const [draft, setDraft] = useState<DashboardBlock | undefined>(initial)
  const entries = useMemo(() => (initial ? [] : catalog(home)), [initial, home])
  // The overview's own blocks first, then what gathers devices, then the parts of each page.
  const order = [
    t("Vue d'ensemble"),
    t("Pièces"),
    t("Types d'appareils"),
    t("Appareils"),
    t("Graphiques"),
  ]
  const rank = (g: string) =>
    order.includes(g) ? order.indexOf(g) : order.length
  const groups = [...new Set(entries.map((e) => e.group))].sort(
    (a, b) => rank(a) - rank(b)
  )
  const [group, setGroup] = useState<string | undefined>(groups[0])
  const shownGroup = group && groups.includes(group) ? group : groups[0]

  if (initial)
    return (
      <Dialog open onOpenChange={(o) => !o && onClose()}>
        <DialogContent className="max-h-[90svh] overflow-y-auto sm:max-w-3xl lg:max-w-5xl">
          <DialogHeader>
            <DialogTitle>{t("Réglages du bloc")}</DialogTitle>
          </DialogHeader>
          {draft && (
            <div className="grid gap-4">
              <Preview block={draft} tall />
              <Settings
                block={draft}
                home={home}
                page={page}
                onChange={setDraft}
              />
            </div>
          )}
          <DialogFooter>
            <Button
              disabled={!draft || !complete(draft)}
              onClick={() => draft && onDone(draft)}
            >
              {t("Appliquer")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    )

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="flex h-[88svh] flex-col gap-0 p-0 sm:max-w-6xl">
        <DialogHeader className="border-b p-5">
          <DialogTitle>{t("Ajouter un bloc")}</DialogTitle>
        </DialogHeader>
        <div className="flex min-h-0 flex-1 flex-col md:flex-row">
          <nav className="flex shrink-0 gap-1 overflow-x-auto border-b p-3 md:w-56 md:flex-col md:overflow-y-auto md:border-r md:border-b-0">
            {groups.map((g) => (
              <button
                key={g}
                type="button"
                onClick={() => {
                  setGroup(g)
                  setDraft(undefined)
                }}
                className={cn(
                  "flex shrink-0 items-center justify-between gap-3 rounded-xl px-3 py-2 text-left text-sm font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
                  g === shownGroup && !draft && "bg-accent text-foreground"
                )}
              >
                {g}
                <span className="tabular text-xs text-muted-foreground">
                  {entries.filter((e) => e.group === g).length}
                </span>
              </button>
            ))}
          </nav>
          <div className="min-h-0 flex-1 overflow-y-auto p-5">
            {draft ? (
              <div className="mx-auto grid max-w-2xl gap-5">
                <Button
                  variant="ghost"
                  size="sm"
                  className="justify-self-start"
                  onClick={() => setDraft(undefined)}
                >
                  <ChevronLeft /> {t("Retour aux composants")}
                </Button>
                <Preview block={draft} tall />
                <Settings
                  block={draft}
                  home={home}
                  page={page}
                  onChange={setDraft}
                />
                <Button
                  disabled={!complete(draft)}
                  onClick={() => onDone(draft)}
                >
                  <Plus /> {t("Ajouter")}
                </Button>
              </div>
            ) : entries.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                {t("Rien à ajouter pour l'instant.")}
              </p>
            ) : (
              <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
                {entries
                  .filter((e) => e.group === shownGroup)
                  .map((e) => (
                    <button
                      key={e.key}
                      type="button"
                      onClick={() => setDraft(e.block)}
                      className="group grid content-start gap-2 rounded-3xl border border-transparent p-2 text-left transition-colors hover:border-primary/50 hover:bg-accent/40 focus-visible:border-primary focus-visible:outline-none"
                    >
                      <Preview block={e.block} />
                      <span className="truncate px-2 pb-1 text-sm font-medium">
                        {e.label}
                      </span>
                    </button>
                  ))}
              </div>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
