import type { AutomationTrigger, SunEvent } from "@/lib/api"
import { t } from "@/lib/i18n"
import { deviceType } from "@/lib/device-icons"
import { Input } from "@/components/ui/input"
import type { Home, Option } from "./fields"
import {
  Comparison,
  DaysPicker,
  FACINGS,
  FOR_MINUTES,
  PRESSES,
  Pick,
  SUN_EVENTS,
  SUN_OFFSETS,
  StatePick,
  firstValue,
  moduleStateOptions,
  ModuleStatePick,
  measureOptions,
  remotes,
  stateOptions,
  suggested,
} from "./fields"
import { TariffPick, firstStretch } from "./tariff"

export const ROW =
  "flex flex-col gap-2 rounded-2xl border bg-muted/30 p-3 lg:flex-row lg:flex-wrap lg:items-center"

export const forOptions = (): Option[] =>
  FOR_MINUTES.map((m) => [
    String(m),
    m ? t("pendant {n} min", { n: m }) : t("tout de suite"),
  ])

export function TriggerEditor({
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
    ["presence", t("Quelqu'un arrive ou part")],
    ...(home.tariff
      ? [["tariff", t("Le tarif change de plage")] as Option]
      : []),
    ...(moduleStateOptions(home).length
      ? [["module", t("Un module change d'état")] as Option]
      : []),
    ...(remotes(home).length
      ? [["button", t("Un bouton de télécommande")] as Option]
      : []),
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
      {when.kind === "presence" && (
        <Pick
          value={String(when.home)}
          onChange={(v) => onChange({ ...when, home: v === "true" })}
          options={[
            ["true", t("Le premier arrive à la maison")],
            ["false", t("Le dernier part de la maison")],
          ]}
          className="w-full lg:w-64"
        />
      )}
      {when.kind === "module" && (
        <>
          <ModuleStatePick
            home={home}
            module={when.module}
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
      {when.kind === "tariff" && (
        <>
          <span className="text-sm">{t("passe en")}</span>
          <TariffPick
            home={home}
            profile={when.profile}
            slot={when.slot}
            onChange={(p) => onChange({ ...when, ...p })}
          />
        </>
      )}
      {when.kind === "button" && (
        <ButtonPick
          home={home}
          trigger={when}
          onChange={(next) => onChange(next)}
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

export function FacadePick({
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

export type ButtonTrigger = Extract<AutomationTrigger, { kind: "button" }>

/** Which remote, which button, which press. */
export function ButtonPick({
  home,
  trigger,
  onChange,
}: {
  home: Home
  trigger: ButtonTrigger
  onChange: (t: ButtonTrigger) => void
}) {
  const list = remotes(home)
  const remote = list.find((r) => r.id === trigger.device_id)
  const count = Number(remote?.state?.buttons ?? 4) || 4
  return (
    <>
      <Pick
        value={String(trigger.device_id)}
        onChange={(v) =>
          onChange({ ...trigger, device_id: Number(v), button: 1 })
        }
        options={list.map((r): Option => [String(r.id), r.name, deviceType(r)])}
        className="w-full lg:w-56"
      />
      <Pick
        value={String(trigger.button ?? "any")}
        onChange={(v) =>
          onChange({ ...trigger, button: v === "any" ? null : Number(v) })
        }
        options={[
          ["any", t("n'importe quel bouton")],
          ...Array.from({ length: count }, (_, i): Option => [
            String(i + 1),
            t("bouton {n}", { n: i + 1 }),
          ]),
        ]}
        className="w-full lg:w-48"
      />
      <Pick
        value={trigger.press ?? "any"}
        onChange={(v) =>
          onChange({
            ...trigger,
            press: v === "any" ? null : (v as "single" | "double" | "long"),
          })
        }
        options={PRESSES}
        className="w-full lg:w-40"
      />
    </>
  )
}

export function newTrigger(kind: string, home: Home): AutomationTrigger | null {
  switch (kind) {
    case "time":
      return { kind: "time", time: "07:00", days: [] }
    case "sun":
      return { kind: "sun", event: "sunset", offset_minutes: 0, days: [] }
    case "facade":
      return { kind: "facade", facing: 180 }
    case "presence":
      return { kind: "presence", home: false }
    case "tariff":
      return home.tariff ? { kind: "tariff", ...firstStretch(home) } : null
    case "module": {
      const first = moduleStateOptions(home)[0]
      return first
        ? {
            kind: "module",
            module: first.module,
            field: first.field.name,
            value: firstValue(first.field),
            for_minutes: 0,
          }
        : null
    }
    case "button": {
      const r = remotes(home)[0]
      return r ? { kind: "button", device_id: r.id, button: 1 } : null
    }
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
