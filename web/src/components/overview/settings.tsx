import { useState } from "react"
import { Plus, X } from "lucide-react"
import type { LucideIcon } from "lucide-react"
import type {
  Charted,
  DashboardBlock,
  DashboardPage,
  EnergyLive,
  SurplusView,
  DeviceView,
  Room,
  Routine,
  TraitInfo,
} from "@/lib/api"
import { deviceType, typeOrder } from "@/lib/device-icons"
import { t } from "@/lib/i18n"
import { ROOM_ICONS, roomIconName } from "@/lib/room-icons"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
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
    return true
  })
}

/** Whether a block has what it needs to show something. */
export function complete(b: DashboardBlock) {
  const c = b.config
  if (b.type === "device") return typeof c.device_id === "number"
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

/** Sets up a block: a new one (its type first), or one already on the overview. */
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
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90svh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {initial ? t("Réglages du bloc") : t("Ajouter un bloc")}
          </DialogTitle>
        </DialogHeader>
        <div className="grid gap-4">
          {!initial && (
            <Field label={t("Type de bloc")}>
              <Pick
                value={draft?.type}
                placeholder={t("Choisir")}
                onChange={(type) =>
                  setDraft(
                    newBlock(
                      type as BlockType,
                      type === "energy"
                        ? { flow: measuredFlows(home.live?.roles)[0] }
                        : {}
                    )
                  )
                }
                choices={available(home).map((b) => ({
                  value: b,
                  label: BLOCK_LABEL[b],
                  group: BLOCK_GROUP[b],
                }))}
              />
            </Field>
          )}
          {draft && (
            <Settings
              block={draft}
              home={home}
              page={page}
              onChange={setDraft}
            />
          )}
        </div>
        <DialogFooter>
          <Button
            disabled={!draft || !complete(draft)}
            onClick={() => draft && onDone(draft)}
          >
            {initial ? t("Appliquer") : t("Ajouter")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
