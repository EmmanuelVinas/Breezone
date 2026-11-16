import { Trash2 } from "lucide-react"
import type {
  AutomationStep,
  DeviceView,
  TraitField,
  TraitInfo,
} from "@/lib/api"
import type { DeviceAction } from "@/lib/api"
import { t } from "@/lib/i18n"
import { SCENE_TYPE } from "@/lib/device-icons"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { ConditionEditor, newCondition } from "./conditions"
import type { Home, Option } from "./fields"
import {
  byType,
  MAX_ACTIONS,
  NumberInput,
  Pick,
  TIMEOUTS,
  WAITS,
  minutes,
  tl,
  traitsOf,
  valueLabel,
} from "./fields"
import { ROW, forOptions } from "./triggers"

/** The actions a rule can run on a device: `trait.action`. */
export function actionsOf(home: Home, d: DeviceView) {
  return traitsOf(home, d).flatMap((trait) =>
    trait.actions.map((a) => ({
      key: `${trait.trait}.${a.name}`,
      trait: trait.trait,
      action: a,
    }))
  )
}

/** The parameters shown: a zone keeps the three that matter in a rule. */
export function shownParams(trait: string, params: TraitField[]) {
  return trait === "climate.zone"
    ? params.filter((p) => ["on", "setpoint", "mode"].includes(p.name))
    : params
}

export function defaultParams(trait: string, params: TraitField[]) {
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

export function deviceStep(home: Home, d: DeviceView): AutomationStep | null {
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

export function ParamInput({
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
    <label className="flex items-center gap-2">
      <span className="text-sm text-muted-foreground">{tl(field.label)}</span>
      <NumberInput
        value={typeof value === "number" ? value : undefined}
        onChange={onChange}
        unit={field.unit}
        placeholder={field.optional ? t("facultatif") : undefined}
        min={field.min}
        max={field.max}
      />
    </label>
  )
}

/** The device of a step, or a scene. */
export function TargetEditor({
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

/** What the scenes need to drive the devices: the devices and the catalog of their traits. */
export type DeviceHome = { devices: DeviceView[]; traits: TraitInfo[] }

export function homeOf({ devices, traits }: DeviceHome): Home {
  return {
    devices,
    drivable: new Set(devices.map((d) => d.id)),
    traits,
    scenes: [],
    roles: { grid: false, solar: false, consumption: false },
  }
}

/** The devices a scene can drive (those with an action). */
export function drivableDevices(home: DeviceHome) {
  return home.devices.filter((d) => actionsOf(homeOf(home), d).length > 0)
}

/** A first action on this device (its first action, default parameters). */
export function newDeviceAction(
  home: DeviceHome,
  d: DeviceView
): DeviceAction | null {
  const step = deviceStep(homeOf(home), d)
  return step?.kind === "device"
    ? {
        device_id: step.device_id,
        trait: step.trait,
        action: step.action,
        params: step.params,
      }
    : null
}

/** "Salon : allumer · Luminosité 60 %". */
export function describeDeviceAction(home: DeviceHome, a: DeviceAction) {
  const device = home.devices.find((d) => d.id === a.device_id)
  if (!device) return t("Appareil indisponible")
  const found = actionsOf(homeOf(home), device).find(
    (x) => x.key === `${a.trait}.${a.action}`
  )
  if (!found) return device.name
  const given = shownParams(found.trait, found.action.params).flatMap((p) => {
    const v = a.params[p.name]
    if (v === undefined || v === null) return []
    if (typeof v === "boolean")
      return [
        v
          ? t("{label} : oui", { label: tl(p.label) })
          : t("{label} : non", { label: tl(p.label) }),
      ]
    if (typeof v === "string") return [valueLabel(p, v)]
    return [`${tl(p.label)} ${v}${p.unit ? ` ${p.unit}` : ""}`]
  })
  return t("{zone} : {actions}", {
    zone: device.name,
    actions: [tl(found.action.label), ...given].join(" · "),
  })
}

/** The device, action and parameters of an action of a scene. */
export function DeviceActionEditor({
  home,
  action,
  onChange,
}: {
  home: DeviceHome
  action: DeviceAction
  onChange: (a: DeviceAction) => void
}) {
  return (
    <TargetEditor
      home={homeOf(home)}
      step={{
        kind: "device",
        device_id: action.device_id,
        trait: action.trait,
        action: action.action,
        params: action.params,
      }}
      onChange={(s) =>
        s.kind === "device" &&
        onChange({
          ...action,
          device_id: s.device_id,
          trait: s.trait,
          action: s.action,
          params: s.params,
        })
      }
    />
  )
}

export function StepEditor({
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
    case "notify":
      return (
        <>
          <span className="text-sm">{t("Prévenir")}</span>
          <Input
            className="w-full lg:w-96"
            maxLength={200}
            value={step.message}
            placeholder={t("ex. Le surplus solaire charge la voiture")}
            onChange={(e) => onChange({ ...step, message: e.target.value })}
          />
        </>
      )
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
export function StepList({
  home,
  steps,
  onChange,
  before = [],
}: {
  home: Home
  steps: AutomationStep[]
  onChange: (s: AutomationStep[]) => void
  /** The steps run before these in the same run (what "put back" can undo). */
  before?: AutomationStep[]
}) {
  const firstTarget = home.devices.find(
    (d) => home.drivable.has(d.id) && actionsOf(home, d).length > 0
  )
  const drivesDevice = [...before, ...steps].some((s) => s.kind === "device")
  const kinds: Option[] = [
    ...(firstTarget ? [["device", t("Agir sur un appareil")] as Option] : []),
    ...(home.scenes.length ? [["scene", t("Lancer une scène")] as Option] : []),
    ["wait", t("Attendre")],
    ["wait_until", t("Attendre que…")],
    ["notify", t("Prévenir")],
    ...(drivesDevice ? [["restore", t("Remettre comme avant")] as Option] : []),
  ]
  const add = (k: string) => {
    let step: AutomationStep | null = null
    if (k === "device" && firstTarget) step = deviceStep(home, firstTarget)
    else if (k === "scene" && home.scenes[0])
      step = { kind: "scene", routine_id: home.scenes[0].id }
    else if (k === "wait") step = { kind: "wait", seconds: 600 }
    else if (k === "restore") step = { kind: "restore" }
    else if (k === "notify") step = { kind: "notify", message: "" }
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

export function Section({
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

export const WHEN_TONE = "border-warning/40 text-warning"
export const IF_TONE = "border-fan/40 text-fan"
export const THEN_TONE = "border-primary/40 text-primary"
export const ELSE_TONE = "border-muted-foreground/40 text-muted-foreground"

export function RemoveButton({ onClick }: { onClick: () => void }) {
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
