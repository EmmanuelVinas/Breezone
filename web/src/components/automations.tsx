import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Pencil, Play, Plus, Trash2, Workflow } from "lucide-react"
import { toast } from "sonner"
import type {
  Automation,
  AutomationCondition,
  AutomationInput,
  AutomationMeasure,
  AutomationOp,
  AutomationStep,
  AutomationTrigger,
  DeviceView,
  Mode,
  Routine,
  TraitField,
  TraitInfo,
  TraitLabel,
} from "@/lib/api"
import { api, currentUser } from "@/lib/api"
import { MODE_LABEL, relativeTime, dateTime } from "@/lib/format"
import { lang, locale, t } from "@/lib/i18n"
import { useChosenInstallations } from "@/lib/selected-installation"
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
  SelectItem,
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

const MAX_CONDITIONS = 3
const MAX_ACTIONS = 5

const tl = (l: TraitLabel) => l[lang()]

const MONTHS = Array.from({ length: 12 }, (_, i) =>
  new Date(2024, i, 1).toLocaleString(locale(), { month: "long" })
)
// 1 January 2024 was a Monday.
const DAYS = Array.from({ length: 7 }, (_, i) =>
  new Date(2024, 0, i + 1).toLocaleString(locale(), { weekday: "short" })
)

const FOR_MINUTES = [0, 1, 5, 10, 15, 30, 60]
const SUN_OFFSETS = [-120, -60, -30, -15, 0, 15, 30, 60, 120]

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
  measure: AutomationMeasure
}

function measureKey(m: AutomationMeasure) {
  if (m.kind === "energy") return `energy:${m.flow}`
  if (m.kind === "device") return `device:${m.device_id}:${m.field}`
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
    measure: { kind: "energy", flow },
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
    measure: { kind: "outdoor" },
  })
  for (const d of home.devices)
    for (const trait of traitsOf(home, d))
      for (const f of trait.state)
        if (isMeasure(trait.trait, f))
          out.push({
            key: `device:${d.id}:${f.name}`,
            label: `${d.name} · ${tl(f.label)}`,
            unit: f.unit,
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
  device_id: number
  field: TraitField
}

function stateOptions(home: Home): StateOption[] {
  return home.devices.flatMap((d) =>
    traitsOf(home, d).flatMap((trait) =>
      trait.state
        .filter((f) => f.type === "bool" || f.type === "enum")
        .map((f) => ({
          key: `${d.id}:${f.name}`,
          label: `${d.name} · ${tl(f.label)}`,
          device_id: d.id,
          field: f,
        }))
    )
  )
}

function valueLabel(field: TraitField | undefined, v: string) {
  return field?.name === "mode" && v in MODE_LABEL ? MODE_LABEL[v as Mode] : v
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
  options: [string, string][]
  placeholder?: string
  className?: string
}) {
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger className={className}>
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent>
        {options.map(([k, label]) => (
          <SelectItem key={k} value={k}>
            {label}
          </SelectItem>
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
        onChange={(k) =>
          onChange({
            measure: options.find((o) => o.key === k)!.measure,
            op,
            value,
          })
        }
        options={options.map((o) => [o.key, o.label])}
        className="w-full lg:w-72"
      />
      <Pick
        value={op}
        onChange={(v) => onChange({ measure, op: v as AutomationOp, value })}
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

const ROW =
  "flex flex-col gap-2 rounded-2xl border bg-muted/30 p-3 lg:flex-row lg:flex-wrap lg:items-center"

function TriggerEditor({
  home,
  when,
  onChange,
}: {
  home: Home
  when: AutomationTrigger
  onChange: (w: AutomationTrigger) => void
}) {
  const kinds: [string, string][] = [
    ["measure", t("Une mesure passe un seuil")],
    ["time", t("À heure fixe")],
    ["sun", t("Lever ou coucher du soleil")],
  ]
  const setKind = (k: string) => {
    if (k === when.kind) return
    if (k === "time") onChange({ kind: "time", time: "07:00", days: [] })
    else if (k === "sun")
      onChange({ kind: "sun", event: "sunset", offset_minutes: 0, days: [] })
    else onChange(defaultTrigger(home))
  }
  return (
    <div className={ROW}>
      <Pick
        value={when.kind}
        onChange={setKind}
        options={kinds}
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
            options={FOR_MINUTES.map((m) => [
              String(m),
              m ? t("pendant {n} min", { n: m }) : t("tout de suite"),
            ])}
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
            onChange={(v) =>
              onChange({ ...when, event: v as "sunrise" | "sunset" })
            }
            options={[
              ["sunrise", t("Lever du soleil")],
              ["sunset", t("Coucher du soleil")],
            ]}
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
      {when.kind !== "measure" && (
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
    </div>
  )
}

function defaultTrigger(home: Home): AutomationTrigger {
  const first = measureOptions(home)[0]
  return {
    kind: "measure",
    measure: first.measure,
    op: "above",
    value: first.key === "energy:export" ? 500 : NaN,
    for_minutes: 5,
  }
}

function newCondition(kind: string, home: Home): AutomationCondition | null {
  switch (kind) {
    case "measure": {
      const options = measureOptions(home)
      const temp = options.find(
        (o) => o.measure.kind === "device" && o.measure.field === "room_temp"
      )
      return {
        kind: "measure",
        measure: (temp ?? options[0]).measure,
        op: "below",
        value: NaN,
      }
    }
    case "state": {
      const first = stateOptions(home)[0]
      if (!first) return null
      return {
        kind: "state",
        device_id: first.device_id,
        field: first.field.name,
        value: first.field.type === "bool" ? true : first.field.values![0],
      }
    }
    case "months":
      // Winter.
      return { kind: "months", from: 11, to: 3 }
    case "hours":
      return { kind: "hours", from: "08:00", to: "20:00" }
    case "days":
      return { kind: "days", days: [1, 2, 3, 4, 5] }
    default:
      return { kind: "daylight", day: true }
  }
}

function ConditionEditor({
  home,
  condition: c,
  onChange,
}: {
  home: Home
  condition: AutomationCondition
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
    case "state": {
      const options = stateOptions(home)
      const chosen = options.find((o) => o.key === `${c.device_id}:${c.field}`)
      const values: [string, string][] =
        chosen?.field.type === "enum"
          ? chosen.field.values!.map((v) => [v, valueLabel(chosen.field, v)])
          : [
              ["true", t("Oui")],
              ["false", t("Non")],
            ]
      return (
        <>
          <Pick
            value={`${c.device_id}:${c.field}`}
            onChange={(k) => {
              const o = options.find((x) => x.key === k)!
              onChange({
                kind: "state",
                device_id: o.device_id,
                field: o.field.name,
                value: o.field.type === "bool" ? true : o.field.values![0],
              })
            }}
            options={options.map((o) => [o.key, o.label])}
            className="w-full lg:w-72"
          />
          <Pick
            value={String(c.value)}
            onChange={(v) =>
              onChange({
                ...c,
                value: chosen?.field.type === "enum" ? v : v === "true",
              })
            }
            options={values}
            className="w-full lg:w-40"
          />
        </>
      )
    }
    case "months": {
      const months: [string, string][] = MONTHS.map((m, i) => [
        String(i + 1),
        m,
      ])
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
  }
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
      p.type === "bool" ? true : p.type === "enum" ? p.values![0] : (p.min ?? 0)
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
  const keep: [string, string][] = field.optional
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
          ...field.values!.map((v): [string, string] => [
            v,
            valueLabel(field, v),
          ]),
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

function StepEditor({
  home,
  step,
  onChange,
}: {
  home: Home
  step: AutomationStep
  onChange: (s: AutomationStep) => void
}) {
  const devices = home.devices.filter(
    (d) => home.drivable.has(d.id) && actionsOf(home, d).length > 0
  )
  const targets: [string, string][] = [
    ...devices.map((d): [string, string] => [`device:${d.id}`, d.name]),
    ...home.scenes.map((s): [string, string] => [
      `scene:${s.id}`,
      t("Scène « {name} »", { name: s.name }),
    ]),
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

/** Every number of the rule is filled in. */
function complete(d: AutomationInput) {
  const whenOk = d.when.kind !== "measure" || Number.isFinite(d.when.value)
  const conditionsOk = d.conditions.every(
    (c) =>
      (c.kind !== "measure" || Number.isFinite(c.value)) &&
      (c.kind !== "days" || c.days.length > 0)
  )
  return (
    Boolean(d.name.trim()) && whenOk && conditionsOk && d.actions.length > 0
  )
}

function AutomationEditor({
  home,
  automation,
  onClose,
}: {
  home: Home
  automation?: Automation
  onClose: () => void
}) {
  const qc = useQueryClient()
  const [draft, setDraft] = useState<AutomationInput>(() => ({
    name: automation?.name ?? "",
    enabled: automation?.enabled ?? true,
    when: automation?.when ?? defaultTrigger(home),
    conditions: automation?.conditions ?? [],
    actions: automation?.actions ?? [],
  }))
  const set = (patch: Partial<AutomationInput>) =>
    setDraft((d) => ({ ...d, ...patch }))
  const save = useMutation({
    mutationFn: () =>
      automation
        ? api.updateAutomation(automation.id, draft)
        : api.createAutomation(draft),
    onSuccess: () => {
      toast.success(
        automation ? t("Automatisme modifié") : t("Automatisme créé")
      )
      qc.invalidateQueries({ queryKey: ["automations"] })
      onClose()
    },
    onError: (e) => toast.error(e instanceof Error ? e.message : String(e)),
  })

  const conditionKinds: [string, string][] = [
    ["measure", t("Une mesure")],
    ["state", t("L'état d'un appareil")],
    ["months", t("Une période de l'année")],
    ["hours", t("Une plage horaire")],
    ["days", t("Des jours de la semaine")],
    ["daylight", t("Le jour ou la nuit")],
  ]
  const firstTarget = home.devices.find(
    (d) => home.drivable.has(d.id) && actionsOf(home, d).length > 0
  )
  const addStep = () => {
    const step: AutomationStep | null = firstTarget
      ? deviceStep(home, firstTarget)
      : home.scenes[0]
        ? { kind: "scene", routine_id: home.scenes[0].id }
        : null
    if (step) set({ actions: [...draft.actions, step] })
  }
  const drivesZone = draft.actions.some(
    (s) => s.kind === "device" && s.trait === "climate.zone"
  )

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90svh] overflow-x-hidden overflow-y-auto sm:max-w-[min(68rem,95vw)]">
        <DialogHeader>
          <DialogTitle>
            {automation ? t("Modifier l'automatisme") : t("Nouvel automatisme")}
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
            hint={t("Ce qui déclenche la règle.")}
          >
            <TriggerEditor
              home={home}
              when={draft.when}
              onChange={(when) => set({ when })}
            />
            {draft.when.kind === "measure" && (
              <p className="text-xs text-muted-foreground">
                {t(
                  "La règle agit une fois, puis attend que la mesure soit repassée nettement de l'autre côté du seuil avant de pouvoir agir de nouveau."
                )}
              </p>
            )}
          </Section>

          <Section
            badge={t("Si")}
            tone={IF_TONE}
            hint={t("Facultatif : toutes les conditions doivent être vraies.")}
          >
            {draft.conditions.map((c, i) => (
              <div key={i} className={ROW}>
                <ConditionEditor
                  home={home}
                  condition={c}
                  onChange={(next) =>
                    set({
                      conditions: draft.conditions.map((x, j) =>
                        j === i ? next : x
                      ),
                    })
                  }
                />
                <RemoveButton
                  onClick={() =>
                    set({
                      conditions: draft.conditions.filter((_, j) => j !== i),
                    })
                  }
                />
              </div>
            ))}
            {draft.conditions.length < MAX_CONDITIONS && (
              <Pick
                value=""
                placeholder={t("Ajouter une condition…")}
                onChange={(k) => {
                  const c = newCondition(k, home)
                  if (c) set({ conditions: [...draft.conditions, c] })
                }}
                options={conditionKinds}
                className="w-full lg:w-64"
              />
            )}
          </Section>

          <Section
            badge={t("Alors")}
            tone={THEN_TONE}
            hint={t("Ce que Breezone fait, dans l'ordre.")}
          >
            {draft.actions.map((s, i) => (
              <div key={i} className={ROW}>
                <StepEditor
                  home={home}
                  step={s}
                  onChange={(next) =>
                    set({
                      actions: draft.actions.map((x, j) =>
                        j === i ? next : x
                      ),
                    })
                  }
                />
                <RemoveButton
                  onClick={() =>
                    set({ actions: draft.actions.filter((_, j) => j !== i) })
                  }
                />
              </div>
            ))}
            {draft.actions.length < MAX_ACTIONS && (
              <div>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={addStep}
                  disabled={!firstTarget && !home.scenes.length}
                >
                  <Plus /> {t("Ajouter une action")}
                </Button>
              </div>
            )}
            {drivesZone && (
              <p className="text-xs text-muted-foreground">
                {t(
                  "Cette règle pilote une zone de climatisation : vérifiez qu'une programmation Airzone ne fait pas le contraire au même moment."
                )}
              </p>
            )}
          </Section>
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

function AutomationCard({
  automation: a,
  onEdit,
}: {
  automation: Automation
  /** Absent when the user may not change this rule. */
  onEdit?: () => void
}) {
  const qc = useQueryClient()
  const refresh = () => qc.invalidateQueries({ queryKey: ["automations"] })
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
      if (failed)
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
            {a.summary.when}
            {now && (
              <span className="text-muted-foreground">
                {" "}
                · {t("maintenant : {value}", { value: now })}
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
          <div className="flex gap-1">
            <Button
              size="sm"
              variant="ghost"
              onClick={() => run.mutate()}
              disabled={run.isPending}
            >
              <Play /> {t("Essayer")}
            </Button>
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
      </CardContent>
    </Card>
  )
}

/** Rules "quand… si… alors…" run by the server. */
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
    automation?: Automation
    nonce: number
  } | null>(null)

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
  const used = (a: Automation) => [
    ...a.actions.flatMap((s) => (s.kind === "device" ? [s.device_id] : [])),
    ...a.conditions.flatMap((c) =>
      c.kind === "state"
        ? [c.device_id]
        : c.kind === "measure" && c.measure.kind === "device"
          ? [c.measure.device_id]
          : []
    ),
    ...(a.when.kind === "measure" && a.when.measure.kind === "device"
      ? [a.when.measure.device_id]
      : []),
  ]
  // The rules of the installation chosen: those that touch it, and those of the modules alone.
  const list = (automations.data ?? []).filter((a) => {
    const zones = used(a).flatMap((id) => installationOf.get(id) ?? [])
    return zones.length === 0 || zones.some((i) => ids.has(i))
  })
  const canEdit = (a: Automation) =>
    a.actions.every((s) =>
      s.kind === "device"
        ? drivable.has(s.device_id)
        : home.scenes.some((r) => r.id === s.routine_id)
    )
  const ready = devices.isSuccess && traits.isSuccess
  const canCreate = ready && (drivable.size > 0 || home.scenes.length > 0)

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
          <Button onClick={() => setEditing({ nonce: Date.now() })}>
            <Plus /> {t("Nouvel automatisme")}
          </Button>
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
                ready && canEdit(a)
                  ? () => setEditing({ automation: a, nonce: Date.now() })
                  : undefined
              }
            />
          ))}
        </div>
      ) : (
        <p className="rounded-3xl border border-dashed p-6 text-center text-sm text-muted-foreground">
          {t(
            "Aucun automatisme. Exemple : quand l'injection au réseau dépasse 500 W pendant 5 min, si le salon est sous 18 °C de novembre à mars, alors allumer le salon à 21°."
          )}
        </p>
      )}
      {editing && (
        <AutomationEditor
          key={editing.nonce}
          home={home}
          automation={editing.automation}
          onClose={() => setEditing(null)}
        />
      )}
    </section>
  )
}
