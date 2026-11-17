import type {
  AutomationMeasure,
  AutomationOp,
  DeviceView,
  Mode,
  Routine,
  SunEvent,
  TraitField,
  TraitInfo,
  TraitLabel,
} from "@/lib/api"
import { MODE_LABEL } from "@/lib/format"
import { lang, locale, t } from "@/lib/i18n"
import type { DeviceType } from "@/lib/device-icons"
import {
  DeviceTypeIcon,
  HOME_TYPE,
  WEATHER_TYPE,
  deviceType,
  typeOrder,
} from "@/lib/device-icons"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"

export const MAX_TRIGGERS = 3
/** Conditions of a rule, those of its groups included. */
export const MAX_CONDITIONS = 8
/** Steps of each branch. */
export const MAX_ACTIONS = 8

export const tl = (l: TraitLabel) => l[lang()]

export const MONTHS = Array.from({ length: 12 }, (_, i) =>
  new Date(2024, i, 1).toLocaleString(locale(), { month: "long" })
)
// 1 January 2024 was a Monday.
export const DAYS = Array.from({ length: 7 }, (_, i) =>
  new Date(2024, 0, i + 1).toLocaleString(locale(), { weekday: "short" })
)

export const FOR_MINUTES = [0, 1, 2, 5, 10, 15, 30, 60]
export const SUN_OFFSETS = [-120, -60, -30, -15, 0, 15, 30, 60, 120]
/** Waits offered (minutes). */
export const WAITS = [1, 2, 5, 10, 15, 30, 60, 120, 240, 480]
/** "At most" offered for a wait until (minutes). */
export const TIMEOUTS = [5, 15, 30, 60, 120, 240, 480, 720, 1440]

/** The eight directions a façade faces, from the north. */
export const FACINGS: [number, string][] = [
  [0, t("Nord")],
  [45, t("Nord-est")],
  [90, t("Est")],
  [135, t("Sud-est")],
  [180, t("Sud")],
  [225, t("Sud-ouest")],
  [270, t("Ouest")],
  [315, t("Nord-ouest")],
]

/** The colours of the weather warnings, 0 to 3. */
export const VIGILANCE = [t("verte"), t("jaune"), t("orange"), t("rouge")]

/** A Tempo colour as a comparison of its code (1 blue, 2 white, 3 red). */
const TEMPO_CHOICES: [string, string][] = [
  ["above:2", t("est rouge")],
  ["above:1", t("est blanc ou rouge")],
  ["below:2", t("est bleu")],
  ["below:3", t("est bleu ou blanc")],
]

export const PRESSES: [string, string][] = [
  ["any", t("tout appui")],
  ["single", t("appui")],
  ["double", t("double appui")],
  ["long", t("appui long")],
]

/** The remotes of the home (`button@1`). */
export function remotes(home: Home) {
  return home.devices.filter((d) =>
    d.traits.some((x) => x.startsWith("button@"))
  )
}

export const SUN_EVENTS: [SunEvent, string][] = [
  ["dawn", t("Aube")],
  ["sunrise", t("Lever du soleil")],
  ["sunset", t("Coucher du soleil")],
  ["dusk", t("Crépuscule")],
]

/** "5 min", "2 h", "1 h 30". */
export function minutes(m: number) {
  if (m < 60) return t("{n} min", { n: m })
  const h = Math.floor(m / 60)
  return m % 60 ? `${h} h ${String(m % 60).padStart(2, "0")}` : `${h} h`
}

/** What the editor offers: the devices, their traits, the scenes and the energy measures. */
export type Home = {
  devices: DeviceView[]
  /** Devices this user may drive from a rule. */
  drivable: Set<number>
  traits: TraitInfo[]
  scenes: Routine[]
  roles: { grid: boolean; solar: boolean; consumption: boolean }
  /** The home has a tariff of electricity, and one that follows the Tempo colours. */
  prices?: { tariff: boolean; tempo: boolean }
}

export function traitsOf(home: Home, d: DeviceView): TraitInfo[] {
  return d.traits.flatMap((id) => {
    const found = home.traits.find((x) => `${x.trait}@${x.version}` === id)
    return found ? [found] : []
  })
}

/** A number worth watching: not a setting, and for a zone what its thermostat measures. */
export function isMeasure(trait: string, f: TraitField) {
  if (f.type !== "number" && f.type !== "integer") return false
  if (trait === "climate.zone")
    return ["room_temp", "humidity"].includes(f.name)
  return f.history !== false
}

export type MeasureOption = {
  key: string
  label: string
  unit?: string
  type?: DeviceType
  measure: AutomationMeasure
}

/** The devices by type (climate, charger, lights…), in their order within a type. */
export function byType(devices: DeviceView[]) {
  return devices
    .map((d, i) => ({ d, i, type: deviceType(d) }))
    .sort((a, b) => typeOrder(a.type) - typeOrder(b.type) || a.i - b.i)
}

export function measureKey(m: AutomationMeasure) {
  if (m.kind === "energy") return `energy:${m.flow}`
  if (m.kind === "device") return `device:${m.device_id}:${m.field}`
  if (m.kind === "weather") return `weather:${m.field}`
  if (m.kind === "price") return "price"
  if (m.kind === "tempo") return `tempo:${m.day}`
  return "outdoor"
}

export function measureOptions(home: Home, current?: AutomationMeasure) {
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
  out.push(weather("humidity", t("Humidité extérieure"), "%"))
  out.push(weather("vigilance", t("Vigilance météo"), ""))
  if (home.prices?.tariff)
    out.push({
      key: "price",
      label: t("Prix de l'électricité"),
      unit: "€/kWh",
      type: HOME_TYPE,
      measure: { kind: "price" },
    })
  if (home.prices?.tempo)
    for (const [day, label] of [
      ["today", t("Couleur Tempo du jour")],
      ["tomorrow", t("Couleur Tempo de demain")],
    ] as const)
      out.push({
        key: `tempo:${day}`,
        label,
        type: HOME_TYPE,
        measure: { kind: "tempo", day },
      })
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

export type StateOption = {
  key: string
  label: string
  type: DeviceType
  device_id: number
  field: TraitField
}

export function stateOptions(home: Home): StateOption[] {
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

export function firstValue(f: TraitField): boolean | string {
  return f.type === "bool" ? true : f.values![0]
}

export function valueLabel(field: TraitField | undefined, v: string) {
  const named = field?.value_labels?.[v]
  if (named) return tl(named)
  return field?.name === "mode" && v in MODE_LABEL ? MODE_LABEL[v as Mode] : v
}

/** A choice of a list: its key, its label, and the type it is grouped under (with its icon). */
export type Option = [string, string, DeviceType?]

export function OptionLabel({
  label,
  type,
}: {
  label: string
  type?: DeviceType
}) {
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

export function Pick({
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

export function NumberInput({
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

export function DaysPicker({
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
export function suggested(
  o: MeasureOption | undefined,
  op: AutomationOp
): number {
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
  if (m.kind === "price") return pick(0.2, 0.15)
  // Tomorrow red (above white), or blue (below white).
  if (m.kind === "tempo") return 2
  if (m.kind === "outdoor") return pick(28, 5)
  if (m.kind === "weather")
    return {
      precipitation: pick(0, 0.1),
      wind: pick(50, 10),
      rain_next_hour: pick(70, 20),
      tomorrow_min: pick(20, 0),
      tomorrow_max: pick(33, 10),
      humidity: pick(85, 40),
      // Above yellow: orange or red.
      vigilance: pick(1, 1),
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
export function nextValue(
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
export function Comparison({
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
  if (measure.kind === "tempo")
    return (
      <>
        <Pick
          value={measureKey(measure)}
          onChange={(k) => {
            const next = options.find((o) => o.key === k)!
            onChange({
              measure: next.measure,
              op,
              value:
                next.measure.kind === "tempo" ? value : suggested(next, op),
            })
          }}
          options={options.map((o) => [o.key, o.label, o.type])}
          className="w-full lg:w-72"
        />
        <Pick
          value={`${op}:${value}`}
          onChange={(v) => {
            const [o, n] = v.split(":")
            onChange({ measure, op: o as AutomationOp, value: Number(n) })
          }}
          options={TEMPO_CHOICES}
          className="w-full lg:w-48"
        />
      </>
    )
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
      {measure.kind === "weather" && measure.field === "vigilance" ? (
        <Pick
          value={String(value)}
          onChange={(v) => onChange({ measure, op, value: Number(v) })}
          options={VIGILANCE.map((name, i): Option => [String(i), name])}
          className="w-full lg:w-32"
        />
      ) : (
        <NumberInput
          value={value}
          unit={chosen?.unit || undefined}
          onChange={(v) => onChange({ measure, op, value: v ?? NaN })}
        />
      )}
    </>
  )
}

/** A device's state equal to a value (a yes/no or a choice). */
export function StatePick({
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
