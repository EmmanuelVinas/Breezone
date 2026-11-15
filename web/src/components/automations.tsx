import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  BookOpen,
  Copy,
  History,
  Pencil,
  Play,
  Plus,
  Trash2,
  Workflow,
} from "lucide-react"
import { toast } from "sonner"
import type {
  Automation,
  AutomationCondition,
  AutomationInput,
  AutomationMatch,
  AutomationMeasure,
  AutomationOp,
  AutomationRun,
  AutomationStep,
  AutomationTrigger,
  DeviceView,
  Mode,
  Routine,
  SunEvent,
  TraitField,
  TraitInfo,
  TraitLabel,
} from "@/lib/api"
import { api, currentUser } from "@/lib/api"
import { MODE_LABEL, relativeTime, dateTime } from "@/lib/format"
import { lang, locale, t } from "@/lib/i18n"
import { useChosenInstallations } from "@/lib/selected-installation"
import type { DeviceType } from "@/lib/device-icons"
import {
  DeviceTypeIcon,
  HOME_TYPE,
  SCENE_TYPE,
  WEATHER_TYPE,
  deviceType,
  typeOrder,
} from "@/lib/device-icons"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Skeleton } from "@/components/ui/skeleton"
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
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog"

const MAX_TRIGGERS = 3
/** Conditions of a rule, those of its groups included. */
const MAX_CONDITIONS = 8
/** Steps of each branch. */
const MAX_ACTIONS = 8

const tl = (l: TraitLabel) => l[lang()]

const MONTHS = Array.from({ length: 12 }, (_, i) =>
  new Date(2024, i, 1).toLocaleString(locale(), { month: "long" })
)
// 1 January 2024 was a Monday.
const DAYS = Array.from({ length: 7 }, (_, i) =>
  new Date(2024, 0, i + 1).toLocaleString(locale(), { weekday: "short" })
)

const FOR_MINUTES = [0, 1, 2, 5, 10, 15, 30, 60]
const SUN_OFFSETS = [-120, -60, -30, -15, 0, 15, 30, 60, 120]
/** Waits offered (minutes). */
const WAITS = [1, 2, 5, 10, 15, 30, 60, 120, 240, 480]
/** "At most" offered for a wait until (minutes). */
const TIMEOUTS = [5, 15, 30, 60, 120, 240, 480, 720, 1440]

/** The eight directions a façade faces, from the north. */
const FACINGS: [number, string][] = [
  [0, t("Nord")],
  [45, t("Nord-est")],
  [90, t("Est")],
  [135, t("Sud-est")],
  [180, t("Sud")],
  [225, t("Sud-ouest")],
  [270, t("Ouest")],
  [315, t("Nord-ouest")],
]

const SUN_EVENTS: [SunEvent, string][] = [
  ["dawn", t("Aube")],
  ["sunrise", t("Lever du soleil")],
  ["sunset", t("Coucher du soleil")],
  ["dusk", t("Crépuscule")],
]

/** "5 min", "2 h", "1 h 30". */
function minutes(m: number) {
  if (m < 60) return t("{n} min", { n: m })
  const h = Math.floor(m / 60)
  return m % 60 ? `${h} h ${String(m % 60).padStart(2, "0")}` : `${h} h`
}

/** What the editor offers: the devices, their traits, the scenes and the energy measures. */
type Home = {
  devices: DeviceView[]
  /** Devices this user may drive from a rule. */
  drivable: Set<number>
  traits: TraitInfo[]
  scenes: Routine[]
  roles: { grid: boolean; solar: boolean; consumption: boolean }
}

function traitsOf(home: Home, d: DeviceView): TraitInfo[] {
  return d.traits.flatMap((id) => {
    const found = home.traits.find((x) => `${x.trait}@${x.version}` === id)
    return found ? [found] : []
  })
}

/** A number worth watching: not a setting, and for a zone what its thermostat measures. */
function isMeasure(trait: string, f: TraitField) {
  if (f.type !== "number" && f.type !== "integer") return false
  if (trait === "climate.zone")
    return ["room_temp", "humidity"].includes(f.name)
  return f.history !== false
}

type MeasureOption = {
  key: string
  label: string
  unit?: string
  type?: DeviceType
  measure: AutomationMeasure
}

/** The devices by type (climate, charger, lights…), in their order within a type. */
function byType(devices: DeviceView[]) {
  return devices
    .map((d, i) => ({ d, i, type: deviceType(d) }))
    .sort((a, b) => typeOrder(a.type) - typeOrder(b.type) || a.i - b.i)
}

function measureKey(m: AutomationMeasure) {
  if (m.kind === "energy") return `energy:${m.flow}`
  if (m.kind === "device") return `device:${m.device_id}:${m.field}`
  if (m.kind === "weather") return `weather:${m.field}`
  return "outdoor"
}

function measureOptions(home: Home, current?: AutomationMeasure) {
  const energy = (
    flow: "export" | "import" | "solar" | "home",
    label: string
  ): MeasureOption => ({
    key: `energy:${flow}`,
    label,
    unit: "W",
    type: HOME_TYPE,
    measure: { kind: "energy", flow },
  })
  type WeatherField = Extract<AutomationMeasure, { kind: "weather" }>["field"]
  const weather = (
    field: WeatherField,
    label: string,
    unit: string
  ): MeasureOption => ({
    key: `weather:${field}`,
    label,
    unit,
    type: WEATHER_TYPE,
    measure: { kind: "weather", field },
  })
  const out: MeasureOption[] = []
  if (home.roles.grid) {
    out.push(energy("export", t("Injection au réseau")))
    out.push(energy("import", t("Soutirage du réseau")))
  }
  if (home.roles.solar) out.push(energy("solar", t("Production solaire")))
  if (home.roles.grid || home.roles.consumption)
    out.push(energy("home", t("Consommation de la maison")))
  out.push({
    key: "outdoor",
    label: t("Température extérieure"),
    unit: "°C",
    type: WEATHER_TYPE,
    measure: { kind: "outdoor" },
  })
  out.push(weather("rain_next_hour", t("Risque de pluie dans l'heure"), "%"))
  out.push(weather("precipitation", t("Pluie"), "mm"))
  out.push(weather("wind", t("Vent"), "km/h"))
  out.push(weather("tomorrow_min", t("Minimale de demain"), "°C"))
  out.push(weather("tomorrow_max", t("Maximale de demain"), "°C"))
  for (const { d, type } of byType(home.devices))
    for (const trait of traitsOf(home, d))
      for (const f of trait.state)
        if (isMeasure(trait.trait, f))
          out.push({
            key: `device:${d.id}:${f.name}`,
            label: `${d.name} · ${tl(f.label)}`,
            unit: f.unit,
            type,
            measure: { kind: "device", device_id: d.id, field: f.name },
          })
  // A measure the home no longer offers stays in the list of the rule that uses it.
  if (current && !out.some((o) => o.key === measureKey(current)))
    out.unshift({
      key: measureKey(current),
      label: t("Mesure indisponible"),
      measure: current,
    })
  return out
}

type StateOption = {
  key: string
  label: string
  type: DeviceType
  device_id: number
  field: TraitField
}

function stateOptions(home: Home): StateOption[] {
  return byType(home.devices).flatMap(({ d, type }) =>
    traitsOf(home, d).flatMap((trait) =>
      trait.state
        .filter((f) => f.type === "bool" || f.type === "enum")
        .map((f) => ({
          key: `${d.id}:${f.name}`,
          label: `${d.name} · ${tl(f.label)}`,
          type,
          device_id: d.id,
          field: f,
        }))
    )
  )
}

function firstValue(f: TraitField): boolean | string {
  return f.type === "bool" ? true : f.values![0]
}

function valueLabel(field: TraitField | undefined, v: string) {
  return field?.name === "mode" && v in MODE_LABEL ? MODE_LABEL[v as Mode] : v
}

/** A choice of a list: its key, its label, and the type it is grouped under (with its icon). */
type Option = [string, string, DeviceType?]

function OptionLabel({ label, type }: { label: string; type?: DeviceType }) {
  return (
    <>
      {type && (
        <DeviceTypeIcon
          of={type}
          className="text-muted-foreground"
          aria-hidden
        />
      )}
      {label}
    </>
  )
}

function Pick({
  value,
  onChange,
  options,
  placeholder,
  className = "w-full",
}: {
  value: string
  onChange: (v: string) => void
  options: Option[]
  placeholder?: string
  className?: string
}) {
  // Options of the same type follow each other: one group each, under its name.
  const groups: { type?: DeviceType; options: Option[] }[] = []
  for (const o of options) {
    const last = groups.at(-1)
    if (last && last.type?.id === o[2]?.id) last.options.push(o)
    else groups.push({ type: o[2], options: [o] })
  }
  const grouped = groups.some((g) => g.type)
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger className={className}>
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent>
        {groups.map((g, i) => (
          <SelectGroup key={`${g.type?.id ?? ""}-${i}`}>
            {grouped && g.type && <SelectLabel>{g.type.label}</SelectLabel>}
            {g.options.map(([k, label, type]) => (
              <SelectItem key={k} value={k}>
                <OptionLabel label={label} type={type} />
              </SelectItem>
            ))}
          </SelectGroup>
        ))}
      </SelectContent>
    </Select>
  )
}

function NumberInput({
  value,
  onChange,
  unit,
  placeholder,
  min,
  max,
}: {
  value: number | undefined
  onChange: (v: number | undefined) => void
  unit?: string
  placeholder?: string
  min?: number
  max?: number
}) {
  return (
    <div className="flex items-center gap-2">
      <Input
        type="number"
        step="any"
        min={min}
        max={max}
        className="w-28"
        placeholder={placeholder}
        value={value === undefined || Number.isNaN(value) ? "" : value}
        onChange={(e) =>
          onChange(e.target.value === "" ? undefined : Number(e.target.value))
        }
      />
      {unit && <span className="text-sm text-muted-foreground">{unit}</span>}
    </div>
  )
}

function DaysPicker({
  days,
  onChange,
}: {
  days: number[]
  onChange: (d: number[]) => void
}) {
  return (
    <div className="flex flex-wrap gap-1">
      {DAYS.map((name, i) => {
        const on = days.includes(i + 1)
        return (
          <Button
            key={i}
            type="button"
            size="sm"
            variant={on ? "default" : "outline"}
            aria-pressed={on}
            onClick={() =>
              onChange(
                on ? days.filter((d) => d !== i + 1) : [...days, i + 1].sort()
              )
            }
          >
            {name}
          </Button>
        )
      })}
    </div>
  )
}

/**
 * A sensible threshold for a measure, so that changing the measure never leaves "500 °C": above
 * or below, by what it measures. Empty (to fill in) when nothing obvious fits.
 */
function suggested(o: MeasureOption | undefined, op: AutomationOp): number {
  const pick = (above: number, below: number) =>
    op === "above" ? above : below
  const m = o?.measure
  if (!m) return NaN
  if (m.kind === "energy")
    return {
      export: pick(500, 100),
      import: pick(3000, 200),
      solar: pick(1500, 200),
      home: pick(3000, 500),
    }[m.flow]
  if (m.kind === "outdoor") return pick(28, 5)
  if (m.kind === "weather")
    return {
      precipitation: pick(0, 0.1),
      wind: pick(50, 10),
      rain_next_hour: pick(70, 20),
      tomorrow_min: pick(20, 0),
      tomorrow_max: pick(33, 10),
    }[m.field]
  if (m.field === "room_temp") return pick(26, 18)
  switch (o.unit) {
    case "°C":
      return pick(26, 18)
    case "%":
      return m.field === "humidity" ? pick(70, 40) : pick(80, 20)
    case "W":
      return pick(1000, 10)
    case "kWh":
      return 10
    case "ppm":
      return pick(1000, 600)
    default:
      return NaN
  }
}

/** The threshold after a change: a suggestion, unless the user typed their own. */
function nextValue(
  value: number,
  before: { option?: MeasureOption; op: AutomationOp },
  after: { option?: MeasureOption; op: AutomationOp }
) {
  const untouched =
    Number.isNaN(value) || value === suggested(before.option, before.op)
  const sameMeasure = before.option?.key === after.option?.key
  return untouched || !sameMeasure ? suggested(after.option, after.op) : value
}

/** A measure, above or below a value. */
function Comparison({
  home,
  measure,
  op,
  value,
  onChange,
}: {
  home: Home
  measure: AutomationMeasure
  op: AutomationOp
  value: number
  onChange: (p: {
    measure: AutomationMeasure
    op: AutomationOp
    value: number
  }) => void
}) {
  const options = measureOptions(home, measure)
  const chosen = options.find((o) => o.key === measureKey(measure))
  return (
    <>
      <Pick
        value={measureKey(measure)}
        onChange={(k) => {
          const next = options.find((o) => o.key === k)!
          onChange({
            measure: next.measure,
            op,
            value: nextValue(
              value,
              { option: chosen, op },
              { option: next, op }
            ),
          })
        }}
        options={options.map((o) => [o.key, o.label, o.type])}
        className="w-full lg:w-72"
      />
      <Pick
        value={op}
        onChange={(v) => {
          const next = v as AutomationOp
          onChange({
            measure,
            op: next,
            value: nextValue(
              value,
              { option: chosen, op },
              { option: chosen, op: next }
            ),
          })
        }}
        options={[
          ["above", t("au-dessus de")],
          ["below", t("en dessous de")],
        ]}
        className="w-full lg:w-40"
      />
      <NumberInput
        value={value}
        unit={chosen?.unit}
        onChange={(v) => onChange({ measure, op, value: v ?? NaN })}
      />
    </>
  )
}

/** A device's state equal to a value (a yes/no or a choice). */
function StatePick({
  home,
  device_id,
  field,
  value,
  onChange,
}: {
  home: Home
  device_id: number
  field: string
  value: boolean | string
  onChange: (p: {
    device_id: number
    field: string
    value: boolean | string
  }) => void
}) {
  const options = stateOptions(home)
  const chosen = options.find((o) => o.key === `${device_id}:${field}`)
  const values: Option[] =
    chosen?.field.type === "enum"
      ? chosen.field.values!.map((v): Option => [
          v,
          valueLabel(chosen.field, v),
        ])
      : [
          ["true", t("Oui")],
          ["false", t("Non")],
        ]
  return (
    <>
      <Pick
        value={`${device_id}:${field}`}
        onChange={(k) => {
          const o = options.find((x) => x.key === k)!
          onChange({
            device_id: o.device_id,
            field: o.field.name,
            value: firstValue(o.field),
          })
        }}
        options={options.map((o) => [o.key, o.label, o.type])}
        className="w-full lg:w-72"
      />
      <Pick
        value={String(value)}
        onChange={(v) =>
          onChange({
            device_id,
            field,
            value: chosen?.field.type === "enum" ? v : v === "true",
          })
        }
        options={values}
        className="w-full lg:w-40"
      />
    </>
  )
}

const ROW =
  "flex flex-col gap-2 rounded-2xl border bg-muted/30 p-3 lg:flex-row lg:flex-wrap lg:items-center"

const forOptions = (): Option[] =>
  FOR_MINUTES.map((m) => [
    String(m),
    m ? t("pendant {n} min", { n: m }) : t("tout de suite"),
  ])

function TriggerEditor({
  home,
  when,
  onChange,
}: {
  home: Home
  when: AutomationTrigger
  onChange: (w: AutomationTrigger) => void
}) {
  const kinds: Option[] = [
    ["measure", t("Une mesure passe un seuil")],
    ["state", t("Un appareil change d'état")],
    ["time", t("À heure fixe")],
    ["sun", t("Le soleil (lever, coucher…)")],
    ["facade", t("Le soleil arrive sur une façade")],
  ]
  const states = stateOptions(home)
  const setKind = (k: string) => {
    if (k === when.kind) return
    const next = newTrigger(k, home)
    if (next) onChange(next)
  }
  return (
    <>
      <Pick
        value={when.kind}
        onChange={setKind}
        options={kinds.filter(([k]) => k !== "state" || states.length > 0)}
        className="w-full lg:w-64"
      />
      {when.kind === "measure" && (
        <>
          <Comparison
            home={home}
            measure={when.measure}
            op={when.op}
            value={when.value}
            onChange={(p) => onChange({ ...when, ...p })}
          />
          <Pick
            value={String(when.for_minutes)}
            onChange={(v) => onChange({ ...when, for_minutes: Number(v) })}
            options={forOptions()}
            className="w-full lg:w-44"
          />
        </>
      )}
      {when.kind === "state" && (
        <>
          <StatePick
            home={home}
            device_id={when.device_id}
            field={when.field}
            value={when.value}
            onChange={(p) => onChange({ ...when, ...p })}
          />
          <Pick
            value={String(when.for_minutes)}
            onChange={(v) => onChange({ ...when, for_minutes: Number(v) })}
            options={forOptions()}
            className="w-full lg:w-44"
          />
        </>
      )}
      {when.kind === "time" && (
        <Input
          type="time"
          className="w-32"
          value={when.time}
          onChange={(e) => onChange({ ...when, time: e.target.value })}
        />
      )}
      {when.kind === "sun" && (
        <>
          <Pick
            value={when.event}
            onChange={(v) => onChange({ ...when, event: v as SunEvent })}
            options={SUN_EVENTS}
            className="w-full lg:w-52"
          />
          <Pick
            value={String(when.offset_minutes)}
            onChange={(v) => onChange({ ...when, offset_minutes: Number(v) })}
            options={SUN_OFFSETS.map((m) => [
              String(m),
              m === 0
                ? t("à ce moment")
                : m < 0
                  ? t("{n} min avant", { n: -m })
                  : t("{n} min après", { n: m }),
            ])}
            className="w-full lg:w-40"
          />
        </>
      )}
      {when.kind === "facade" && (
        <FacadePick
          facing={when.facing}
          onChange={(facing) => onChange({ ...when, facing })}
        />
      )}
      {(when.kind === "time" || when.kind === "sun") && (
        <div className="grid gap-1">
          <DaysPicker
            days={when.days}
            onChange={(days) => onChange({ ...when, days })}
          />
          <span className="text-xs text-muted-foreground">
            {when.days.length ? "" : t("Aucun jour choisi : tous les jours.")}
          </span>
        </div>
      )}
    </>
  )
}

function FacadePick({
  facing,
  onChange,
}: {
  facing: number
  onChange: (f: number) => void
}) {
  return (
    <Pick
      value={String(facing)}
      onChange={(v) => onChange(Number(v))}
      options={FACINGS.map(([d, name]) => [
        String(d),
        t("façade {name}", { name: name.toLowerCase() }),
      ])}
      className="w-full lg:w-48"
    />
  )
}

function newTrigger(kind: string, home: Home): AutomationTrigger | null {
  switch (kind) {
    case "time":
      return { kind: "time", time: "07:00", days: [] }
    case "sun":
      return { kind: "sun", event: "sunset", offset_minutes: 0, days: [] }
    case "facade":
      return { kind: "facade", facing: 180 }
    case "state": {
      const options = stateOptions(home)
      // A charger that starts charging is the most common change watched.
      const charging = options.find(
        (o) => o.field.name === "status" && o.field.values?.includes("charging")
      )
      const o = charging ?? options[0]
      if (!o) return null
      return {
        kind: "state",
        device_id: o.device_id,
        field: o.field.name,
        value: charging ? "charging" : firstValue(o.field),
        for_minutes: 0,
      }
    }
    default: {
      const first = measureOptions(home)[0]
      return {
        kind: "measure",
        measure: first.measure,
        op: "above",
        value: suggested(first, "above"),
        for_minutes: 5,
      }
    }
  }
}

const CONDITION_KINDS = (): Option[] => [
  ["measure", t("Une mesure")],
  ["state", t("L'état d'un appareil")],
  ["months", t("Une période de l'année")],
  ["hours", t("Une plage horaire")],
  ["days", t("Des jours de la semaine")],
  ["daylight", t("Le jour ou la nuit")],
  ["facade", t("Le soleil sur une façade")],
]

function newCondition(kind: string, home: Home): AutomationCondition | null {
  switch (kind) {
    case "measure": {
      const options = measureOptions(home)
      const temp = options.find(
        (o) => o.measure.kind === "device" && o.measure.field === "room_temp"
      )
      const chosen = temp ?? options[0]
      return {
        kind: "measure",
        measure: chosen.measure,
        op: "below",
        value: suggested(chosen, "below"),
      }
    }
    case "state": {
      const first = stateOptions(home)[0]
      if (!first) return null
      return {
        kind: "state",
        device_id: first.device_id,
        field: first.field.name,
        value: firstValue(first.field),
      }
    }
    case "months":
      // Winter.
      return { kind: "months", from: 11, to: 3 }
    case "hours":
      return { kind: "hours", from: "08:00", to: "20:00" }
    case "days":
      return { kind: "days", days: [1, 2, 3, 4, 5] }
    case "facade":
      return { kind: "facade", facing: 180 }
    default:
      return { kind: "daylight", day: true }
  }
}

type Leaf = Exclude<AutomationCondition, { kind: "group" }>

function ConditionEditor({
  home,
  condition: c,
  onChange,
}: {
  home: Home
  condition: Leaf
  onChange: (c: AutomationCondition) => void
}) {
  switch (c.kind) {
    case "measure":
      return (
        <Comparison
          home={home}
          measure={c.measure}
          op={c.op}
          value={c.value}
          onChange={(p) => onChange({ ...c, ...p })}
        />
      )
    case "state":
      return (
        <StatePick
          home={home}
          device_id={c.device_id}
          field={c.field}
          value={c.value}
          onChange={(p) => onChange({ ...c, ...p })}
        />
      )
    case "months": {
      const months: Option[] = MONTHS.map((m, i) => [String(i + 1), m])
      return (
        <>
          <span className="text-sm text-muted-foreground">{t("de")}</span>
          <Pick
            value={String(c.from)}
            onChange={(v) => onChange({ ...c, from: Number(v) })}
            options={months}
            className="w-full lg:w-40"
          />
          <span className="text-sm text-muted-foreground">{t("à")}</span>
          <Pick
            value={String(c.to)}
            onChange={(v) => onChange({ ...c, to: Number(v) })}
            options={months}
            className="w-full lg:w-40"
          />
        </>
      )
    }
    case "hours":
      return (
        <>
          <span className="text-sm text-muted-foreground">{t("entre")}</span>
          <Input
            type="time"
            className="w-32"
            value={c.from}
            onChange={(e) => onChange({ ...c, from: e.target.value })}
          />
          <span className="text-sm text-muted-foreground">{t("et")}</span>
          <Input
            type="time"
            className="w-32"
            value={c.to}
            onChange={(e) => onChange({ ...c, to: e.target.value })}
          />
        </>
      )
    case "days":
      return (
        <DaysPicker
          days={c.days}
          onChange={(days) => onChange({ ...c, days })}
        />
      )
    case "daylight":
      return (
        <Pick
          value={String(c.day)}
          onChange={(v) => onChange({ kind: "daylight", day: v === "true" })}
          options={[
            ["true", t("Il fait jour")],
            ["false", t("Il fait nuit")],
          ]}
          className="w-full lg:w-44"
        />
      )
    case "facade":
      return (
        <>
          <span className="text-sm text-muted-foreground">
            {t("le soleil est sur la")}
          </span>
          <FacadePick
            facing={c.facing}
            onChange={(facing) => onChange({ ...c, facing })}
          />
        </>
      )
  }
}

function MatchToggle({
  value,
  onChange,
}: {
  value: AutomationMatch
  onChange: (m: AutomationMatch) => void
}) {
  return (
    <div className="inline-flex rounded-full border p-0.5" role="group">
      {(
        [
          ["all", t("Toutes (et)")],
          ["any", t("Au moins une (ou)")],
        ] as [AutomationMatch, string][]
      ).map(([m, label]) => (
        <Button
          key={m}
          type="button"
          size="sm"
          variant={value === m ? "default" : "ghost"}
          aria-pressed={value === m}
          className="rounded-full"
          onClick={() => onChange(m)}
        >
          {label}
        </Button>
      ))}
    </div>
  )
}

const opposite = (m: AutomationMatch): AutomationMatch =>
  m === "all" ? "any" : "all"
const word = (m: AutomationMatch) => (m === "all" ? t("et") : t("ou"))

/** The word between two conditions ("et", "ou"). */
function Connector({ text }: { text: string }) {
  return (
    <div className="pl-3 text-xs font-medium tracking-wide text-muted-foreground uppercase">
      {text}
    </div>
  )
}

function leavesOf(conditions: AutomationCondition[]) {
  return conditions.reduce(
    (n, c) => n + (c.kind === "group" ? c.conditions.length : 1),
    0
  )
}

/** The conditions of a rule, or of a group: rows joined by "et" or "ou", and groups. */
function ConditionList({
  home,
  conditions,
  match,
  onChange,
  room,
  inGroup = false,
}: {
  home: Home
  conditions: AutomationCondition[]
  match: AutomationMatch
  onChange: (c: AutomationCondition[]) => void
  /** Conditions that can still be added (the rule's limit). */
  room: number
  inGroup?: boolean
}) {
  const set = (i: number, next: AutomationCondition) =>
    onChange(conditions.map((x, j) => (j === i ? next : x)))
  const remove = (i: number) => onChange(conditions.filter((_, j) => j !== i))
  const kinds = CONDITION_KINDS().filter(
    ([k]) => k !== "state" || stateOptions(home).length > 0
  )
  const add: Option[] = inGroup
    ? kinds
    : [
        ...kinds,
        [
          "group",
          t("Un groupe « {word} » (2 conditions ou plus)", {
            word: word(opposite(match)),
          }),
        ],
      ]
  return (
    <div className="grid gap-2">
      {conditions.map((c, i) => (
        <div key={i} className="grid gap-2">
          {i > 0 && <Connector text={word(match)} />}
          {c.kind === "group" ? (
            <div className="grid gap-2 rounded-2xl border border-dashed p-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="text-sm text-muted-foreground">
                  {t("Groupe : ")}
                </span>
                <MatchToggle
                  value={c.match}
                  onChange={(m) => set(i, { ...c, match: m })}
                />
                <RemoveButton onClick={() => remove(i)} />
              </div>
              <ConditionList
                home={home}
                conditions={c.conditions}
                match={c.match}
                onChange={(inner) => set(i, { ...c, conditions: inner })}
                room={room}
                inGroup
              />
            </div>
          ) : (
            <div className={ROW}>
              <ConditionEditor
                home={home}
                condition={c}
                onChange={(next) => set(i, next)}
              />
              <RemoveButton onClick={() => remove(i)} />
            </div>
          )}
        </div>
      ))}
      {room > 0 && (
        <Pick
          value=""
          placeholder={
            inGroup ? t("Ajouter au groupe…") : t("Ajouter une condition…")
          }
          onChange={(k) => {
            if (k === "group") {
              const a = newCondition("measure", home)!
              const b = newCondition("months", home)!
              if (room >= 2)
                onChange([
                  ...conditions,
                  { kind: "group", match: opposite(match), conditions: [a, b] },
                ])
              return
            }
            const c = newCondition(k, home)
            if (c) onChange([...conditions, c])
          }}
          options={add}
          className="w-full lg:w-72"
        />
      )}
    </div>
  )
}

/** The actions a rule can run on a device: `trait.action`. */
function actionsOf(home: Home, d: DeviceView) {
  return traitsOf(home, d).flatMap((trait) =>
    trait.actions.map((a) => ({
      key: `${trait.trait}.${a.name}`,
      trait: trait.trait,
      action: a,
    }))
  )
}

/** The parameters shown: a zone keeps the three that matter in a rule. */
function shownParams(trait: string, params: TraitField[]) {
  return trait === "climate.zone"
    ? params.filter((p) => ["on", "setpoint", "mode"].includes(p.name))
    : params
}

function defaultParams(trait: string, params: TraitField[]) {
  if (trait === "climate.zone") return { on: true }
  const out: Record<string, unknown> = {}
  for (const p of params.filter((p) => !p.optional))
    out[p.name] =
      p.type === "bool"
        ? true
        : p.type === "enum"
          ? p.values![0]
          : p.name === "minutes"
            ? 30
            : (p.min ?? 0)
  return out
}

function deviceStep(home: Home, d: DeviceView): AutomationStep | null {
  const first = actionsOf(home, d)[0]
  if (!first) return null
  return {
    kind: "device",
    device_id: d.id,
    trait: first.trait,
    action: first.action.name,
    params: defaultParams(first.trait, first.action.params),
  }
}

function ParamInput({
  field,
  value,
  onChange,
}: {
  field: TraitField
  value: unknown
  onChange: (v: unknown) => void
}) {
  const keep: Option[] = field.optional
    ? [["keep", t("{label} : inchangé", { label: tl(field.label) })]]
    : []
  if (field.type === "bool")
    return (
      <Pick
        value={value === undefined ? "keep" : String(value)}
        onChange={(v) => onChange(v === "keep" ? undefined : v === "true")}
        options={[
          ...keep,
          ["true", t("{label} : oui", { label: tl(field.label) })],
          ["false", t("{label} : non", { label: tl(field.label) })],
        ]}
        className="w-full lg:w-52"
      />
    )
  if (field.type === "enum")
    return (
      <Pick
        value={value === undefined ? "keep" : String(value)}
        onChange={(v) => onChange(v === "keep" ? undefined : v)}
        options={[
          ...keep,
          ...field.values!.map((v): Option => [v, valueLabel(field, v)]),
        ]}
        className="w-full lg:w-52"
      />
    )
  return (
    <NumberInput
      value={typeof value === "number" ? value : undefined}
      onChange={onChange}
      unit={field.unit}
      placeholder={tl(field.label)}
      min={field.min}
      max={field.max}
    />
  )
}

/** The device of a step, or a scene. */
function TargetEditor({
  home,
  step,
  onChange,
}: {
  home: Home
  step: Extract<AutomationStep, { kind: "device" | "scene" }>
  onChange: (s: AutomationStep) => void
}) {
  const devices = home.devices.filter(
    (d) => home.drivable.has(d.id) && actionsOf(home, d).length > 0
  )
  const targets: Option[] = [
    ...byType(devices).map(({ d, type }): Option => [
      `device:${d.id}`,
      d.name,
      type,
    ]),
    ...home.scenes.map((s): Option => [`scene:${s.id}`, s.name, SCENE_TYPE]),
  ]
  const key =
    step.kind === "scene"
      ? `scene:${step.routine_id}`
      : `device:${step.device_id}`
  if (!targets.some(([k]) => k === key))
    targets.unshift([key, t("Appareil indisponible")])
  const device =
    step.kind === "device"
      ? home.devices.find((d) => d.id === step.device_id)
      : undefined
  const actions = device ? actionsOf(home, device) : []
  const chosen =
    step.kind === "device"
      ? actions.find((a) => a.key === `${step.trait}.${step.action}`)
      : undefined
  return (
    <>
      <Pick
        value={key}
        onChange={(k) => {
          const [kind, id] = k.split(":")
          if (kind === "scene")
            return onChange({ kind: "scene", routine_id: Number(id) })
          const d = home.devices.find((x) => x.id === Number(id))
          const next = d && deviceStep(home, d)
          if (next) onChange(next)
        }}
        options={targets}
        className="w-full lg:w-64"
      />
      {step.kind === "device" && actions.length > 1 && (
        <Pick
          value={`${step.trait}.${step.action}`}
          onChange={(k) => {
            const a = actions.find((x) => x.key === k)!
            onChange({
              ...step,
              trait: a.trait,
              action: a.action.name,
              params: defaultParams(a.trait, a.action.params),
            })
          }}
          options={actions.map((a) => [a.key, tl(a.action.label)])}
          className="w-full lg:w-56"
        />
      )}
      {step.kind === "device" &&
        chosen &&
        shownParams(chosen.trait, chosen.action.params).map((p) => (
          <ParamInput
            key={p.name}
            field={p}
            value={step.params[p.name]}
            onChange={(v) => {
              const params = { ...step.params }
              if (v === undefined) delete params[p.name]
              else params[p.name] = v
              onChange({ ...step, params })
            }}
          />
        ))}
    </>
  )
}

function StepEditor({
  home,
  step,
  onChange,
}: {
  home: Home
  step: AutomationStep
  onChange: (s: AutomationStep) => void
}) {
  switch (step.kind) {
    case "device":
    case "scene":
      return <TargetEditor home={home} step={step} onChange={onChange} />
    case "wait":
      return (
        <>
          <span className="text-sm">{t("Attendre")}</span>
          <Pick
            value={String(step.seconds / 60)}
            onChange={(v) => onChange({ ...step, seconds: Number(v) * 60 })}
            options={WAITS.map((m) => [String(m), minutes(m)])}
            className="w-full lg:w-32"
          />
        </>
      )
    case "wait_until": {
      const c = step.condition
      return (
        <>
          <span className="text-sm">{t("Attendre que")}</span>
          {c.kind !== "group" && (
            <ConditionEditor
              home={home}
              condition={c}
              onChange={(condition) => onChange({ ...step, condition })}
            />
          )}
          <Pick
            value={String(step.for_seconds / 60)}
            onChange={(v) => onChange({ ...step, for_seconds: Number(v) * 60 })}
            options={forOptions()}
            className="w-full lg:w-44"
          />
          <Pick
            value={String(step.timeout_seconds / 60)}
            onChange={(v) =>
              onChange({ ...step, timeout_seconds: Number(v) * 60 })
            }
            options={TIMEOUTS.map((m) => [
              String(m),
              t("au plus {d}", { d: minutes(m) }),
            ])}
            className="w-full lg:w-36"
          />
          <Pick
            value={step.on_timeout}
            onChange={(v) =>
              onChange({ ...step, on_timeout: v as "continue" | "stop" })
            }
            options={[
              ["continue", t("puis continuer")],
              ["stop", t("sinon s'arrêter")],
            ]}
            className="w-full lg:w-40"
          />
        </>
      )
    }
    case "restore":
      return (
        <span className="text-sm">
          {t(
            "Remettre comme avant les appareils pilotés plus haut dans cette liste"
          )}
        </span>
      )
  }
}

/** A list of steps (then, else), with the menu to add one. */
function StepList({
  home,
  steps,
  onChange,
}: {
  home: Home
  steps: AutomationStep[]
  onChange: (s: AutomationStep[]) => void
}) {
  const firstTarget = home.devices.find(
    (d) => home.drivable.has(d.id) && actionsOf(home, d).length > 0
  )
  const drivesDevice = steps.some((s) => s.kind === "device")
  const kinds: Option[] = [
    ...(firstTarget ? [["device", t("Agir sur un appareil")] as Option] : []),
    ...(home.scenes.length ? [["scene", t("Lancer une scène")] as Option] : []),
    ["wait", t("Attendre")],
    ["wait_until", t("Attendre que…")],
    ...(drivesDevice ? [["restore", t("Remettre comme avant")] as Option] : []),
  ]
  const add = (k: string) => {
    let step: AutomationStep | null = null
    if (k === "device" && firstTarget) step = deviceStep(home, firstTarget)
    else if (k === "scene" && home.scenes[0])
      step = { kind: "scene", routine_id: home.scenes[0].id }
    else if (k === "wait") step = { kind: "wait", seconds: 600 }
    else if (k === "restore") step = { kind: "restore" }
    else if (k === "wait_until") {
      const condition = newCondition("measure", home)!
      step = {
        kind: "wait_until",
        condition,
        for_seconds: 0,
        timeout_seconds: 3600,
        on_timeout: "continue",
      }
    }
    if (step) onChange([...steps, step])
  }
  return (
    <div className="grid gap-2">
      {steps.map((s, i) => (
        <div key={i} className={ROW}>
          <span className="grid size-6 shrink-0 place-items-center rounded-full bg-muted text-xs text-muted-foreground">
            {i + 1}
          </span>
          <StepEditor
            home={home}
            step={s}
            onChange={(next) =>
              onChange(steps.map((x, j) => (j === i ? next : x)))
            }
          />
          <RemoveButton
            onClick={() => onChange(steps.filter((_, j) => j !== i))}
          />
        </div>
      ))}
      {steps.length < MAX_ACTIONS && (
        <Pick
          value=""
          placeholder={t("Ajouter une action…")}
          onChange={add}
          options={kinds}
          className="w-full lg:w-64"
        />
      )}
    </div>
  )
}

function Section({
  badge,
  tone,
  hint,
  children,
}: {
  badge: string
  tone: string
  hint: string
  children: React.ReactNode
}) {
  return (
    <div className="grid gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <Badge variant="outline" className={tone}>
          {badge}
        </Badge>
        <span className="text-sm text-muted-foreground">{hint}</span>
      </div>
      {children}
    </div>
  )
}

const WHEN_TONE = "border-warning/40 text-warning"
const IF_TONE = "border-fan/40 text-fan"
const THEN_TONE = "border-primary/40 text-primary"
const ELSE_TONE = "border-muted-foreground/40 text-muted-foreground"

function RemoveButton({ onClick }: { onClick: () => void }) {
  return (
    <Button
      variant="ghost"
      size="icon"
      className="lg:ml-auto"
      onClick={onClick}
      aria-label={t("Retirer")}
    >
      <Trash2 />
    </Button>
  )
}

function conditionComplete(c: AutomationCondition): boolean {
  if (c.kind === "group")
    return c.conditions.length >= 2 && c.conditions.every(conditionComplete)
  if (c.kind === "measure") return Number.isFinite(c.value)
  if (c.kind === "days") return c.days.length > 0
  return true
}

/** Every number of the rule is filled in. */
function complete(d: AutomationInput) {
  const triggersOk = d.triggers.every(
    (w) => w.kind !== "measure" || Number.isFinite(w.value)
  )
  const stepsOk = [...d.actions, ...d.else].every(
    (s) => s.kind !== "wait_until" || conditionComplete(s.condition)
  )
  return (
    Boolean(d.name.trim()) &&
    d.triggers.length > 0 &&
    triggersOk &&
    d.conditions.every(conditionComplete) &&
    d.actions.length > 0 &&
    stepsOk
  )
}

function inputOf(a: Automation): AutomationInput {
  return {
    name: a.name,
    enabled: a.enabled,
    triggers: a.triggers,
    match: a.match ?? "all",
    conditions: a.conditions,
    actions: a.actions,
    else: a.else ?? [],
    mode: a.mode ?? "single",
  }
}

function AutomationEditor({
  home,
  id,
  initial,
  onClose,
}: {
  home: Home
  /** The rule changed; none for a new one. */
  id?: number
  initial: AutomationInput
  onClose: () => void
}) {
  const qc = useQueryClient()
  const [draft, setDraft] = useState<AutomationInput>(initial)
  const [withElse, setWithElse] = useState(initial.else.length > 0)
  const set = (patch: Partial<AutomationInput>) =>
    setDraft((d) => ({ ...d, ...patch }))
  const save = useMutation({
    mutationFn: () => {
      const input = { ...draft, else: withElse ? draft.else : [] }
      return id ? api.updateAutomation(id, input) : api.createAutomation(input)
    },
    onSuccess: () => {
      toast.success(id ? t("Automatisme modifié") : t("Automatisme créé"))
      qc.invalidateQueries({ queryKey: ["automations"] })
      onClose()
    },
    onError: (e) => toast.error(e instanceof Error ? e.message : String(e)),
  })

  const all = [...draft.actions, ...(withElse ? draft.else : [])]
  const waits = all.some((s) => s.kind === "wait" || s.kind === "wait_until")
  const drivesZone = all.some(
    (s) => s.kind === "device" && s.trait === "climate.zone"
  )
  const room = MAX_CONDITIONS - leavesOf(draft.conditions)

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90svh] overflow-x-hidden overflow-y-auto sm:max-w-[min(72rem,95vw)]">
        <DialogHeader>
          <DialogTitle>
            {id ? t("Modifier l'automatisme") : t("Nouvel automatisme")}
          </DialogTitle>
          <DialogDescription>
            {t(
              "Une règle se lit comme une phrase : quand quelque chose se produit, si certaines choses sont vraies, alors Breezone agit. Tout se passe sur votre serveur."
            )}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-5">
          <div className="grid gap-2">
            <Label htmlFor="an">{t("Nom")}</Label>
            <Input
              id="an"
              value={draft.name}
              placeholder={t("ex. Surplus solaire : chauffer le salon")}
              onChange={(e) => set({ name: e.target.value })}
            />
          </div>

          <Section
            badge={t("Quand")}
            tone={WHEN_TONE}
            hint={t("Ce qui déclenche la règle ; un seul suffit.")}
          >
            {draft.triggers.map((w, i) => (
              <div key={i} className="grid gap-2">
                {i > 0 && <Connector text={t("ou")} />}
                <div className={ROW}>
                  <TriggerEditor
                    home={home}
                    when={w}
                    onChange={(next) =>
                      set({
                        triggers: draft.triggers.map((x, j) =>
                          j === i ? next : x
                        ),
                      })
                    }
                  />
                  {draft.triggers.length > 1 && (
                    <RemoveButton
                      onClick={() =>
                        set({
                          triggers: draft.triggers.filter((_, j) => j !== i),
                        })
                      }
                    />
                  )}
                </div>
              </div>
            ))}
            {draft.triggers.length < MAX_TRIGGERS && (
              <div>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() =>
                    set({
                      triggers: [...draft.triggers, newTrigger("time", home)!],
                    })
                  }
                >
                  <Plus /> {t("Ajouter un déclencheur (ou)")}
                </Button>
              </div>
            )}
            {draft.triggers.some(
              (w) =>
                w.kind === "measure" ||
                w.kind === "state" ||
                w.kind === "facade"
            ) && (
              <p className="text-xs text-muted-foreground">
                {t(
                  "Une mesure, un état ou le soleil sur une façade déclenche une fois, puis attend que ce ne soit plus vrai avant de pouvoir déclencher de nouveau."
                )}
              </p>
            )}
          </Section>

          <Section
            badge={t("Si")}
            tone={IF_TONE}
            hint={t("Facultatif : ce qui doit être vrai à ce moment.")}
          >
            {draft.conditions.length > 1 && (
              <div>
                <MatchToggle
                  value={draft.match}
                  onChange={(match) => set({ match })}
                />
              </div>
            )}
            <ConditionList
              home={home}
              conditions={draft.conditions}
              match={draft.match}
              onChange={(conditions) => set({ conditions })}
              room={room}
            />
          </Section>

          <Section
            badge={t("Alors")}
            tone={THEN_TONE}
            hint={t("Ce que Breezone fait, dans l'ordre.")}
          >
            <StepList
              home={home}
              steps={draft.actions}
              onChange={(actions) => set({ actions })}
            />
          </Section>

          {withElse ? (
            <Section
              badge={t("Sinon")}
              tone={ELSE_TONE}
              hint={t(
                "Ce que Breezone fait quand les conditions sont fausses."
              )}
            >
              <StepList
                home={home}
                steps={draft.else}
                onChange={(steps) => set({ else: steps })}
              />
              <div>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => setWithElse(false)}
                >
                  <Trash2 /> {t("Retirer « Sinon »")}
                </Button>
              </div>
            </Section>
          ) : (
            draft.conditions.length > 0 && (
              <div>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => setWithElse(true)}
                >
                  <Plus /> {t("Ajouter « Sinon »")}
                </Button>
              </div>
            )
          )}

          {waits && (
            <div className="grid gap-2">
              <Label>
                {t("Si la règle se redéclenche pendant qu'elle attend")}
              </Label>
              <Pick
                value={draft.mode}
                onChange={(v) => set({ mode: v as "single" | "restart" })}
                options={[
                  ["single", t("Ignorer le nouveau déclenchement")],
                  ["restart", t("Tout recommencer")],
                ]}
                className="w-full lg:w-72"
              />
            </div>
          )}
          {drivesZone && (
            <p className="text-xs text-muted-foreground">
              {t(
                "Cette règle pilote une zone de climatisation : vérifiez qu'une programmation Airzone ne fait pas le contraire au même moment."
              )}
            </p>
          )}
        </div>
        <DialogFooter>
          <Button
            onClick={() => save.mutate()}
            disabled={save.isPending || !complete(draft)}
          >
            {t("Enregistrer")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ---------------------------------------------------------------------------------------
// Recipes: ready-made rules, built from the devices of the home
// ---------------------------------------------------------------------------------------

type Recipe = { title: string; description: string; rule: AutomationInput }

function blank(home: Home): AutomationInput {
  return {
    name: "",
    enabled: true,
    triggers: [newTrigger("measure", home)!],
    match: "all",
    conditions: [],
    actions: [],
    else: [],
    mode: "single",
  }
}

function recipes(home: Home): Recipe[] {
  const mine = home.devices.filter((d) => home.drivable.has(d.id))
  const has = (d: DeviceView, trait: string) =>
    d.traits.some((x) => x.startsWith(`${trait}@`))
  const zones = mine.filter((d) => has(d, "climate.zone"))
  const charger = mine.find((d) => has(d, "ev.charger"))
  const shutters = mine.filter(
    (d) => has(d, "cover.position") && d.kind !== "window" && d.kind !== "door"
  )
  const windows = mine.filter(
    (d) => has(d, "cover.position") && d.kind === "window"
  )
  const lights = mine.filter((d) => has(d, "light"))
  const humid = home.devices.find(
    (d) => has(d, "ventilation.room") && typeof d.state?.humidity === "number"
  )
  const vmc = humid ?? mine.find((d) => has(d, "ventilation.unit"))
  const step = (
    d: DeviceView,
    trait: string,
    action: string,
    params: Record<string, unknown> = {}
  ): AutomationStep => ({
    kind: "device",
    device_id: d.id,
    trait,
    action,
    params,
  })
  const exportAbove = (w: number): AutomationTrigger => ({
    kind: "measure",
    measure: { kind: "energy", flow: "export" },
    op: "above",
    value: w,
    for_minutes: 5,
  })
  const importAbove = (w: number): AutomationCondition => ({
    kind: "measure",
    measure: { kind: "energy", flow: "import" },
    op: "above",
    value: w,
  })
  const base = (name: string, triggers: AutomationTrigger[]) => ({
    ...blank(home),
    name,
    triggers,
  })
  const out: Recipe[] = []
  if (home.roles.grid && charger)
    out.push({
      title: t("Surplus solaire : charger la voiture"),
      description: t(
        "Quand plus de 500 W repartent au réseau pendant 5 min, lancer la charge, puis l'arrêter quand la maison tire plus de 200 W du réseau pendant 10 min."
      ),
      rule: {
        ...base(t("Surplus solaire : charger la voiture"), [exportAbove(500)]),
        conditions: [
          {
            kind: "state",
            device_id: charger.id,
            field: "status",
            value: "plugged",
          },
        ],
        actions: [
          step(charger, "ev.charger", "start"),
          {
            kind: "wait_until",
            condition: importAbove(200),
            for_seconds: 600,
            timeout_seconds: 12 * 3600,
            on_timeout: "continue",
          },
          step(charger, "ev.charger", "stop"),
        ],
      },
    })
  if (home.roles.grid && zones[0])
    out.push({
      title: t("Surplus solaire : chauffer une pièce en hiver"),
      description: t(
        "Quand plus de 500 W repartent au réseau, si la pièce est sous 18 °C de novembre à mars, chauffer à 21°, puis remettre comme avant quand le surplus s'arrête."
      ),
      rule: {
        ...base(t("Surplus solaire : chauffer le salon"), [exportAbove(500)]),
        conditions: [
          {
            kind: "measure",
            measure: {
              kind: "device",
              device_id: zones[0].id,
              field: "room_temp",
            },
            op: "below",
            value: 18,
          },
          { kind: "months", from: 11, to: 3 },
        ],
        actions: [
          step(zones[0], "climate.zone", "set", {
            on: true,
            mode: "heating",
            setpoint: 21,
          }),
          {
            kind: "wait_until",
            condition: importAbove(300),
            for_seconds: 600,
            timeout_seconds: 8 * 3600,
            on_timeout: "continue",
          },
          { kind: "restore" },
        ],
      },
    })
  if (shutters.length) {
    const some = shutters.slice(0, MAX_ACTIONS)
    out.push({
      title: t("Fermer les volets au coucher du soleil"),
      description: t(
        "Chaque soir, au coucher du soleil, fermer les volets (à ajuster)."
      ),
      rule: {
        ...base(t("Fermer les volets le soir"), [
          { kind: "sun", event: "sunset", offset_minutes: 0, days: [] },
        ]),
        actions: some.map((d) =>
          step(d, "cover.position", "set", { position: 0 })
        ),
      },
    })
    out.push({
      title: t("Ouvrir les volets le matin, pas avant 7 h"),
      description: t(
        "Au lever du soleil ou à 7 h, au plus tard des deux : ouvrir les volets."
      ),
      rule: {
        ...base(t("Ouvrir les volets le matin"), [
          { kind: "sun", event: "sunrise", offset_minutes: 0, days: [] },
          { kind: "time", time: "07:00", days: [] },
        ]),
        conditions: [
          { kind: "hours", from: "07:00", to: "12:00" },
          { kind: "daylight", day: true },
        ],
        actions: some.map((d) =>
          step(d, "cover.position", "set", { position: 100 })
        ),
      },
    })
    out.push({
      title: t("Protéger de la chaleur"),
      description: t(
        "Quand le soleil arrive sur la façade sud et qu'il fait plus de 26 °C dehors, fermer les volets à 20 %."
      ),
      rule: {
        ...base(t("Protéger de la chaleur"), [{ kind: "facade", facing: 180 }]),
        conditions: [
          {
            kind: "measure",
            measure: { kind: "outdoor" },
            op: "above",
            value: 26,
          },
        ],
        actions: some.map((d) =>
          step(d, "cover.position", "set", { position: 20 })
        ),
      },
    })
  }
  if (windows[0] && zones[0])
    out.push({
      title: t("Fenêtre ouverte : couper la clim, puis la remettre"),
      description: t(
        "Quand la fenêtre est ouverte depuis 2 min, éteindre la zone ; quand elle se referme, la remettre comme avant."
      ),
      rule: {
        ...base(t("Fenêtre ouverte : couper la clim"), [
          {
            kind: "measure",
            measure: {
              kind: "device",
              device_id: windows[0].id,
              field: "position",
            },
            op: "above",
            value: 0,
            for_minutes: 2,
          },
        ]),
        actions: [
          step(zones[0], "climate.zone", "set", { on: false }),
          {
            kind: "wait_until",
            condition: {
              kind: "measure",
              measure: {
                kind: "device",
                device_id: windows[0].id,
                field: "position",
              },
              op: "below",
              value: 1,
            },
            for_seconds: 0,
            timeout_seconds: 12 * 3600,
            on_timeout: "stop",
          },
          { kind: "restore" },
        ],
      },
    })
  if (windows.length)
    out.push({
      title: t("Pluie annoncée : fermer les fenêtres de toit"),
      description: t(
        "Quand le risque de pluie dans l'heure dépasse 70 %, fermer les fenêtres."
      ),
      rule: {
        ...base(t("Pluie annoncée : fermer les fenêtres"), [
          {
            kind: "measure",
            measure: { kind: "weather", field: "rain_next_hour" },
            op: "above",
            value: 70,
            for_minutes: 0,
          },
        ]),
        actions: windows
          .slice(0, MAX_ACTIONS)
          .map((d) => step(d, "cover.position", "set", { position: 0 })),
      },
    })
  if (lights[0])
    out.push({
      title: t("Allumer une lumière au crépuscule"),
      description: t("Chaque soir au crépuscule, allumer la lumière choisie."),
      rule: {
        ...base(t("Lumière au crépuscule"), [
          { kind: "sun", event: "dusk", offset_minutes: 0, days: [] },
        ]),
        actions: [step(lights[0], "light", "set", { on: true })],
      },
    })
  if (vmc && humid)
    out.push({
      title: t("Salle de bains humide : boost de la VMC"),
      description: t(
        "Quand l'humidité de la pièce dépasse 75 % pendant 2 min, lancer un boost de 30 min."
      ),
      rule: {
        ...base(t("Boost VMC à la douche"), [
          {
            kind: "measure",
            measure: { kind: "device", device_id: humid.id, field: "humidity" },
            op: "above",
            value: 75,
            for_minutes: 2,
          },
        ]),
        actions: [
          step(
            vmc,
            has(vmc, "ventilation.room")
              ? "ventilation.room"
              : "ventilation.unit",
            "boost",
            { minutes: 30 }
          ),
        ],
      },
    })
  if (zones.length)
    out.push({
      title: t("Gel annoncé : garder la maison hors gel"),
      description: t(
        "À 22 h, si la minimale de demain est sous 0 °C, chauffer les zones à 16°."
      ),
      rule: {
        ...base(t("Gel annoncé"), [{ kind: "time", time: "22:00", days: [] }]),
        conditions: [
          {
            kind: "measure",
            measure: { kind: "weather", field: "tomorrow_min" },
            op: "below",
            value: 0,
          },
        ],
        actions: zones.slice(0, MAX_ACTIONS).map((d) =>
          step(d, "climate.zone", "set", {
            on: true,
            mode: "heating",
            setpoint: 16,
          })
        ),
      },
    })
  return out
}

function RecipesDialog({
  home,
  onPick,
  onClose,
}: {
  home: Home
  onPick: (rule: AutomationInput) => void
  onClose: () => void
}) {
  const list = recipes(home)
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90svh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("Recettes")}</DialogTitle>
          <DialogDescription>
            {t(
              "Des règles toutes prêtes, faites avec vos appareils : choisissez-en une, ajustez 2 ou 3 valeurs, enregistrez."
            )}
          </DialogDescription>
        </DialogHeader>
        {list.length ? (
          <div className="grid gap-2">
            {list.map((r) => (
              <button
                key={r.title}
                type="button"
                className="grid gap-1 rounded-2xl border p-3 text-left transition-colors hover:bg-muted/50"
                onClick={() => onPick(r.rule)}
              >
                <span className="font-medium">{r.title}</span>
                <span className="text-sm text-muted-foreground">
                  {r.description}
                </span>
              </button>
            ))}
          </div>
        ) : (
          <p className="text-sm text-muted-foreground">
            {t(
              "Aucune recette ne correspond encore à vos appareils (borne, volets, lumières, VMC, mesure d'énergie)."
            )}
          </p>
        )}
      </DialogContent>
    </Dialog>
  )
}

// ---------------------------------------------------------------------------------------
// The list
// ---------------------------------------------------------------------------------------

function Line({
  badge,
  tone,
  children,
}: {
  badge: string
  tone: string
  children: React.ReactNode
}) {
  return (
    <div className="flex items-start gap-2 text-sm">
      <Badge variant="outline" className={`mt-0.5 ${tone}`}>
        {badge}
      </Badge>
      <div className="min-w-0">{children}</div>
    </div>
  )
}

const BRANCH: Record<AutomationRun["branch"], string> = {
  then: t("Exécuté"),
  else: t("Sinon"),
  skipped: t("Pas déclenché"),
  busy: t("Déjà en cours"),
}

const ENDED: Record<NonNullable<AutomationRun["ended"]>, string> = {
  done: "",
  stopped: t("arrêté : attente écoulée"),
  cancelled: t("recommencé"),
  interrupted: t("interrompu par un arrêt du serveur"),
}

/** The last runs of a rule, those that did not happen included. */
function RunsPanel({ id }: { id: number }) {
  const runs = useQuery({
    queryKey: ["automation-runs", id],
    queryFn: () => api.automationRuns(id),
    refetchInterval: 10_000,
  })
  if (runs.isLoading) return <Skeleton className="h-16 rounded-2xl" />
  if (!runs.data?.length)
    return (
      <p className="text-sm text-muted-foreground">
        {t("Rien encore : l'historique garde les 20 derniers passages.")}
      </p>
    )
  return (
    <ul className="grid gap-2">
      {runs.data.map((r, i) => (
        <li key={i} className="rounded-2xl border bg-muted/30 p-3 text-sm">
          <div className="flex flex-wrap items-center gap-2">
            <Badge
              variant={
                r.branch === "then" || r.branch === "else"
                  ? "secondary"
                  : "outline"
              }
            >
              {BRANCH[r.branch]}
            </Badge>
            <span className="text-xs text-muted-foreground">
              {dateTime(r.started_at)}
            </span>
            {r.ended === null && (
              <Badge variant="outline" className={THEN_TONE}>
                {t("En cours")}
              </Badge>
            )}
            {r.ended && ENDED[r.ended] && (
              <span className="text-xs text-muted-foreground">
                {ENDED[r.ended]}
              </span>
            )}
          </div>
          <p className="mt-1">{r.reason}</p>
          {r.steps.length > 0 && (
            <ul className="mt-1 grid gap-0.5 text-xs text-muted-foreground">
              {r.steps.map((s, j) => (
                <li key={j} className={s.ok ? "" : "text-destructive"}>
                  {s.ok ? "✓" : "✗"} {s.what}
                  {s.error ? ` — ${s.error}` : ""}
                </li>
              ))}
            </ul>
          )}
        </li>
      ))}
    </ul>
  )
}

function AutomationCard({
  automation: a,
  onEdit,
  onDuplicate,
}: {
  automation: Automation
  /** Absent when the user may not change this rule. */
  onEdit?: () => void
  onDuplicate?: () => void
}) {
  const qc = useQueryClient()
  const [history, setHistory] = useState(false)
  const refresh = () => {
    qc.invalidateQueries({ queryKey: ["automations"] })
    qc.invalidateQueries({ queryKey: ["automation-runs", a.id] })
  }
  const onError = (e: unknown) =>
    toast.error(e instanceof Error ? e.message : String(e))
  const enable = useMutation({
    mutationFn: (enabled: boolean) => api.enableAutomation(a.id, enabled),
    onSuccess: refresh,
    onError,
  })
  const run = useMutation({
    mutationFn: () => api.runAutomation(a.id),
    onSuccess: (r) => {
      const failed = r.results.filter((x) => !x.ok).length
      if (r.running)
        toast.success(
          t("« {name} » a démarré ; elle continue en attendant.", {
            name: a.name,
          })
        )
      else if (failed)
        toast.warning(
          t("« {name} » : {failed} action(s) en échec", {
            name: a.name,
            failed,
          })
        )
      else toast.success(t("« {name} » exécutée", { name: a.name }))
      refresh()
    },
    onError,
  })
  const remove = useMutation({
    mutationFn: () => api.deleteAutomation(a.id),
    onSuccess: refresh,
    onError,
  })
  const now =
    a.status.value === null
      ? null
      : `${Math.round(a.status.value * 10) / 10}${a.status.unit ? ` ${a.status.unit}` : ""}`

  return (
    <Card className="rounded-3xl">
      <CardContent className="flex flex-col gap-3">
        <div className="flex items-start justify-between gap-3">
          <div className="flex min-w-0 items-center gap-2">
            <Workflow className="size-4 shrink-0 text-fan" />
            <h3 className="truncate font-semibold">{a.name}</h3>
            {a.status.running && (
              <Badge variant="outline" className={THEN_TONE}>
                {t("En cours")}
              </Badge>
            )}
          </div>
          <Switch
            checked={a.enabled}
            disabled={!onEdit}
            loading={enable.isPending}
            onCheckedChange={(v) => enable.mutate(v)}
            aria-label={t("Activer « {name} »", { name: a.name })}
          />
        </div>
        <div className={`grid gap-1.5 ${a.enabled ? "" : "opacity-60"}`}>
          <Line badge={t("Quand")} tone={WHEN_TONE}>
            {(a.summary.triggers ?? [a.summary.when]).map((w, i) => (
              <div key={i}>
                {i > 0 && (
                  <span className="text-muted-foreground">{t("ou")} </span>
                )}
                {w}
              </div>
            ))}
            {now && (
              <span className="text-muted-foreground">
                {t("maintenant : {value}", { value: now })}
              </span>
            )}
          </Line>
          {a.summary.conditions.length > 0 && (
            <Line badge={t("Si")} tone={IF_TONE}>
              {a.summary.conditions.map((c, i) => (
                <div
                  key={i}
                  className={
                    a.status.conditions[i] ? "" : "text-muted-foreground"
                  }
                >
                  {i > 0 && (
                    <span className="text-muted-foreground">
                      {a.summary.joined}{" "}
                    </span>
                  )}
                  {c}
                  {a.status.conditions[i] === false &&
                    ` · ${t("pas maintenant")}`}
                  {a.status.conditions[i] === null && ` · ${t("inconnu")}`}
                </div>
              ))}
            </Line>
          )}
          <Line badge={t("Alors")} tone={THEN_TONE}>
            {a.summary.actions.map((s, i) => (
              <div key={i}>{s}</div>
            ))}
          </Line>
          {(a.summary.else ?? []).length > 0 && (
            <Line badge={t("Sinon")} tone={ELSE_TONE}>
              {a.summary.else.map((s, i) => (
                <div key={i}>{s}</div>
              ))}
            </Line>
          )}
        </div>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="grid gap-0.5 text-xs text-muted-foreground">
            {a.last_run_at ? (
              <span>
                {t("Dernier : {when}", { when: relativeTime(a.last_run_at) })}
                {a.last_reason ? ` — ${a.last_reason}` : ""}
              </span>
            ) : (
              <span>{t("Jamais déclenché")}</span>
            )}
            {a.enabled && a.status.next_at && (
              <span>
                {t("Prochain : {when}", { when: dateTime(a.status.next_at) })}
              </span>
            )}
          </div>
          <div className="flex flex-wrap gap-1">
            <Button
              size="sm"
              variant="ghost"
              onClick={() => run.mutate()}
              disabled={run.isPending}
            >
              <Play /> {t("Essayer")}
            </Button>
            <Button
              size="icon"
              variant={history ? "secondary" : "ghost"}
              onClick={() => setHistory(!history)}
              aria-label={t("Historique")}
              aria-pressed={history}
            >
              <History />
            </Button>
            {onDuplicate && (
              <Button
                size="icon"
                variant="ghost"
                onClick={onDuplicate}
                aria-label={t("Dupliquer")}
              >
                <Copy />
              </Button>
            )}
            {onEdit && (
              <Button
                size="icon"
                variant="ghost"
                onClick={onEdit}
                aria-label={t("Modifier")}
              >
                <Pencil />
              </Button>
            )}
            {onEdit && (
              <AlertDialog>
                <AlertDialogTrigger asChild>
                  <Button
                    size="icon"
                    variant="ghost"
                    aria-label={t("Supprimer")}
                  >
                    <Trash2 />
                  </Button>
                </AlertDialogTrigger>
                <AlertDialogContent>
                  <AlertDialogHeader>
                    <AlertDialogTitle>
                      {t("Supprimer « {name} » ?", { name: a.name })}
                    </AlertDialogTitle>
                    <AlertDialogDescription>
                      {t("Cet automatisme ne se déclenchera plus.")}
                    </AlertDialogDescription>
                  </AlertDialogHeader>
                  <AlertDialogFooter>
                    <AlertDialogCancel>{t("Annuler")}</AlertDialogCancel>
                    <AlertDialogAction onClick={() => remove.mutate()}>
                      {t("Supprimer")}
                    </AlertDialogAction>
                  </AlertDialogFooter>
                </AlertDialogContent>
              </AlertDialog>
            )}
          </div>
        </div>
        {history && <RunsPanel id={a.id} />}
      </CardContent>
    </Card>
  )
}

/** Rules "quand… si… alors… sinon…" run by the server. */
export function AutomationsSection() {
  const automations = useQuery({
    queryKey: ["automations"],
    queryFn: api.automations,
    // What the rules see now (the measure, the conditions) follows the home.
    refetchInterval: 30_000,
  })
  const devices = useQuery({ queryKey: ["devices"], queryFn: api.devices })
  const traits = useQuery({
    queryKey: ["traits"],
    queryFn: api.traits,
    staleTime: Infinity,
  })
  const routines = useQuery({ queryKey: ["routines"], queryFn: api.routines })
  const live = useQuery({ queryKey: ["energy-live"], queryFn: api.energyLive })
  const { installations, ids } = useChosenInstallations()
  const admin = currentUser()?.role === "admin"
  const [editing, setEditing] = useState<{
    id?: number
    initial: AutomationInput
    nonce: number
  } | null>(null)
  const [choosing, setChoosing] = useState(false)

  const editable = new Set(
    installations
      .filter((i) => i.controllable && i.rights.routines)
      .map((i) => i.id)
  )
  // The zones of the installation chosen, and the devices of the modules of this server.
  const shown = (devices.data ?? []).filter(
    (d) => d.installation_id === null || ids.has(d.installation_id)
  )
  const drivable = new Set(
    shown
      .filter((d) =>
        d.installation_id === null ? admin : editable.has(d.installation_id)
      )
      .map((d) => d.id)
  )
  const home: Home = {
    devices: shown,
    drivable,
    traits: traits.data ?? [],
    scenes: (routines.data ?? []).filter((r) =>
      r.actions.every((x) => editable.has(x.installation_id))
    ),
    roles: live.data?.roles ?? {
      grid: false,
      solar: false,
      consumption: false,
    },
  }
  const installationOf = new Map(
    (devices.data ?? []).map((d) => [d.id, d.installation_id])
  )
  const deviceIds = (c: AutomationCondition): number[] =>
    c.kind === "group"
      ? c.conditions.flatMap(deviceIds)
      : c.kind === "state"
        ? [c.device_id]
        : c.kind === "measure" && c.measure.kind === "device"
          ? [c.measure.device_id]
          : []
  const steps = (a: Automation) => [...a.actions, ...(a.else ?? [])]
  const used = (a: Automation) => [
    ...steps(a).flatMap((s) => (s.kind === "device" ? [s.device_id] : [])),
    ...a.conditions.flatMap(deviceIds),
    ...(a.triggers ?? []).flatMap((w) =>
      w.kind === "state"
        ? [w.device_id]
        : w.kind === "measure" && w.measure.kind === "device"
          ? [w.measure.device_id]
          : []
    ),
  ]
  // The rules of the installation chosen: those that touch it, and those of the modules alone.
  const list = (automations.data ?? []).filter((a) => {
    const zones = used(a).flatMap((id) => installationOf.get(id) ?? [])
    return zones.length === 0 || zones.some((i) => ids.has(i))
  })
  const canEdit = (a: Automation) =>
    steps(a).every((s) =>
      s.kind === "device"
        ? drivable.has(s.device_id)
        : s.kind === "scene"
          ? home.scenes.some((r) => r.id === s.routine_id)
          : true
    )
  const ready = devices.isSuccess && traits.isSuccess
  const canCreate = ready && (drivable.size > 0 || home.scenes.length > 0)
  const open = (initial: AutomationInput, id?: number) =>
    setEditing((e) => ({ id, initial, nonce: (e?.nonce ?? 0) + 1 }))

  return (
    <section className="grid gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-lg font-semibold">{t("Automatismes")}</h2>
          <p className="text-sm text-muted-foreground">
            {t(
              "Quand quelque chose se produit, Breezone agit tout seul : surplus solaire, température, heure, soleil."
            )}
          </p>
        </div>
        {canCreate && (
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" onClick={() => setChoosing(true)}>
              <BookOpen /> {t("Recettes")}
            </Button>
            <Button onClick={() => open(blank(home))}>
              <Plus /> {t("Nouvel automatisme")}
            </Button>
          </div>
        )}
      </div>
      {automations.isLoading ? (
        <div className="grid gap-4 md:grid-cols-2">
          <Skeleton className="h-40 rounded-3xl" />
          <Skeleton className="h-40 rounded-3xl" />
        </div>
      ) : list.length ? (
        <div className="grid gap-4 md:grid-cols-2">
          {list.map((a) => (
            <AutomationCard
              key={a.id}
              automation={a}
              onEdit={
                ready && canEdit(a) ? () => open(inputOf(a), a.id) : undefined
              }
              onDuplicate={
                ready && canEdit(a)
                  ? () =>
                      open({
                        ...inputOf(a),
                        name: t("{name} (copie)", { name: a.name }),
                      })
                  : undefined
              }
            />
          ))}
        </div>
      ) : (
        <p className="rounded-3xl border border-dashed p-6 text-center text-sm text-muted-foreground">
          {t(
            "Aucun automatisme. Partez d'une recette, ou par exemple : quand l'injection au réseau dépasse 500 W pendant 5 min, si le salon est sous 18 °C de novembre à mars, alors allumer le salon à 21°."
          )}
        </p>
      )}
      {choosing && (
        <RecipesDialog
          home={home}
          onClose={() => setChoosing(false)}
          onPick={(rule) => {
            setChoosing(false)
            open(rule)
          }}
        />
      )}
      {editing && (
        <AutomationEditor
          key={editing.nonce}
          home={home}
          id={editing.id}
          initial={editing.initial}
          onClose={() => setEditing(null)}
        />
      )}
    </section>
  )
}
