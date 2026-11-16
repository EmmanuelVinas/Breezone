import type { AutomationCondition, AutomationMatch } from "@/lib/api"
import { t } from "@/lib/i18n"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import type { Home, Option } from "./fields"
import {
  Comparison,
  DaysPicker,
  MONTHS,
  Pick,
  StatePick,
  firstValue,
  measureOptions,
  stateOptions,
  suggested,
} from "./fields"
import { RemoveButton } from "./steps"
import { FacadePick, ROW } from "./triggers"

export const CONDITION_KINDS = (): Option[] => [
  ["measure", t("Une mesure")],
  ["state", t("L'état d'un appareil")],
  ["months", t("Une période de l'année")],
  ["hours", t("Une plage horaire")],
  ["days", t("Des jours de la semaine")],
  ["daylight", t("Le jour ou la nuit")],
  ["facade", t("Le soleil sur une façade")],
  ["presence", t("Quelqu'un à la maison, ou personne")],
]

export function newCondition(
  kind: string,
  home: Home
): AutomationCondition | null {
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
    case "presence":
      return { kind: "presence", home: false }
    default:
      return { kind: "daylight", day: true }
  }
}

export type Leaf = Exclude<AutomationCondition, { kind: "group" }>

export function ConditionEditor({
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
    case "presence":
      return (
        <Pick
          value={String(c.home)}
          onChange={(v) => onChange({ kind: "presence", home: v === "true" })}
          options={[
            ["true", t("Quelqu'un est à la maison")],
            ["false", t("Personne n'est à la maison")],
          ]}
          className="w-full lg:w-64"
        />
      )
  }
}

export function MatchToggle({
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

export const opposite = (m: AutomationMatch): AutomationMatch =>
  m === "all" ? "any" : "all"
export const word = (m: AutomationMatch) => (m === "all" ? t("et") : t("ou"))

/** The word between two conditions ("et", "ou"). */
export function Connector({ text }: { text: string }) {
  return (
    <div className="pl-3 text-xs font-medium tracking-wide text-muted-foreground uppercase">
      {text}
    </div>
  )
}

export function leavesOf(conditions: AutomationCondition[]) {
  return conditions.reduce(
    (n, c) => n + (c.kind === "group" ? c.conditions.length : 1),
    0
  )
}

/** The conditions of a rule, or of a group: rows joined by "et" or "ou", and groups. */
export function ConditionList({
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
