import { useMemo, useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { CalendarClock, Pencil, Plus, Trash2 } from "lucide-react"
import { toast } from "sonner"
import type { DeviceView, HardwareSchedule, TraitField } from "@/lib/api"
import { api } from "@/lib/api"
import { DAY_LABEL, DAY_SHORT, errorText } from "@/lib/format"
import { t } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import { daysText } from "@/components/airzone-schedules"
import { Pick, tl, valueLabel } from "@/components/automations/fields"
import {
  actionsOf,
  defaultParams,
  homeOf,
  ParamInput,
} from "@/components/automations/steps"
import { Confirm } from "@/components/confirm"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Switch } from "@/components/ui/switch"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

type ScheduleInput = Omit<HardwareSchedule, "id" | "editable" | "note">

const ON =
  "data-[state=on]:border-primary data-[state=on]:bg-primary data-[state=on]:text-primary-foreground"

/** The schedule as the hardware gets it (without what only the hardware says). */
function inputOf(s: HardwareSchedule): ScheduleInput {
  return {
    name: s.name,
    enabled: s.enabled,
    days: s.days,
    time: s.time,
    references: s.references,
    trait: s.trait,
    action: s.action,
    params: s.params,
    end: s.end,
  }
}

/** The parameters given, in words ("Allumé : oui · Luminosité 40 %"). */
function paramsText(fields: TraitField[], params: Record<string, unknown>) {
  return Object.entries(params)
    .map(([k, v]) => {
      const f = fields.find((x) => x.name === k)
      const label = f ? tl(f.label) : k
      const value =
        typeof v === "boolean"
          ? v
            ? t("oui")
            : t("non")
          : typeof v === "string" && f
            ? valueLabel(f, v)
            : `${String(v)}${f?.unit ? ` ${f.unit}` : ""}`
      return t("{label} : {value}", { label, value })
    })
    .join(" · ")
}

/**
 * Editor of a schedule of the hardware: the same model as the Airzone app (devices, days, time,
 * an action, its end), with any action of the devices' traits.
 */
function ScheduleEditor({
  sourceId,
  devices,
  schedule,
  onClose,
}: {
  sourceId: number
  devices: DeviceView[]
  schedule?: HardwareSchedule
  onClose: () => void
}) {
  const qc = useQueryClient()
  const traits = useQuery({ queryKey: ["traits"], queryFn: api.traits })
  const home = homeOf({ devices, traits: traits.data ?? [] })
  const [name, setName] = useState(schedule?.name ?? "")
  const [days, setDays] = useState<string[]>(
    (schedule?.days ?? [1, 2, 3, 4, 5]).map(String)
  )
  const [time, setTime] = useState(schedule?.time ?? "07:00")
  const [chosen, setChosen] = useState<Set<string>>(
    () => new Set(schedule?.references ?? [])
  )
  const [key, setKey] = useState(
    schedule ? `${schedule.trait}.${schedule.action}` : ""
  )
  const [params, setParams] = useState<Record<string, unknown>>(
    schedule?.params ?? {}
  )
  const [withEnd, setWithEnd] = useState(!!schedule?.end)
  const [endTime, setEndTime] = useState(schedule?.end?.time ?? "08:00")
  const [endParams, setEndParams] = useState<Record<string, unknown>>(
    schedule?.end?.params ?? {}
  )
  // The actions every chosen device has.
  const selected = devices.filter(
    (d) => typeof d.reference === "string" && chosen.has(d.reference)
  )
  const actions = useMemo(() => {
    const lists = selected.map((d) => actionsOf(home, d))
    if (!lists.length) return []
    return lists[0].filter((a) =>
      lists.every((l) => l.some((b) => b.key === a.key))
    )
  }, [selected, home])
  const current = actions.find((a) => a.key === key)
  const pickAction = (k: string) => {
    const a = actions.find((x) => x.key === k)
    setKey(k)
    if (a) {
      setParams(defaultParams(a.trait, a.action.params))
      setEndParams(defaultParams(a.trait, a.action.params))
    }
  }
  const save = useMutation({
    mutationFn: () => {
      const [trait, ...rest] = key.split(".")
      // A trait name has dots too ("switch.relay"): the action is the last part.
      const action = rest.pop() ?? ""
      const input: ScheduleInput = {
        name: name.trim(),
        enabled: schedule?.enabled ?? true,
        days: days.map(Number),
        time,
        references: [...chosen],
        trait: [trait, ...rest].join("."),
        action,
        params,
        end: withEnd ? { time: endTime, params: endParams } : null,
      }
      return schedule
        ? api.updateSourceSchedule(sourceId, schedule.id, input)
        : api.createSourceSchedule(sourceId, input)
    },
    onSuccess: (list) => {
      qc.setQueryData(["source-schedules", sourceId], list)
      toast.success(t("Programmation enregistrée dans l'appareil"))
      onClose()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const drivable = devices.filter(
    (d) => typeof d.reference === "string" && actionsOf(home, d).length > 0
  )
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {schedule
              ? t("Modifier la programmation")
              : t("Nouvelle programmation")}
          </DialogTitle>
          <DialogDescription>
            {t(
              "Enregistrée dans l'appareil, qui l'exécute lui-même, même sans Breezone."
            )}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-5">
          <div className="grid gap-2">
            <Label htmlFor="hw-name">{t("Nom")}</Label>
            <Input
              id="hw-name"
              value={name}
              placeholder={t("ex. Réveil")}
              maxLength={40}
              onChange={(e) => setName(e.target.value)}
            />
          </div>
          <div className="grid gap-2">
            <Label>{t("Appareils")}</Label>
            <div className="grid gap-1 sm:grid-cols-2">
              {drivable.map((d) => {
                const r = d.reference as string
                return (
                  <label
                    key={r}
                    className="flex items-center gap-2 rounded-xl border p-2 text-sm"
                  >
                    <Checkbox
                      checked={chosen.has(r)}
                      onCheckedChange={(v) =>
                        setChosen((s) => {
                          const n = new Set(s)
                          if (v === true) n.add(r)
                          else n.delete(r)
                          return n
                        })
                      }
                    />
                    {d.name}
                  </label>
                )
              })}
            </div>
          </div>
          <div className="flex flex-wrap items-end gap-4">
            <div className="grid gap-2">
              <Label>{t("Jours")}</Label>
              <ToggleGroup
                type="multiple"
                variant="outline"
                value={days}
                onValueChange={setDays}
              >
                {DAY_SHORT.map((d, i) => (
                  <ToggleGroupItem
                    key={i}
                    value={String(i + 1)}
                    aria-label={DAY_LABEL[i]}
                    className={cn("w-9", ON)}
                  >
                    {d}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </div>
            <div className="grid gap-2">
              <Label htmlFor="hw-time">{t("Heure")}</Label>
              <Input
                id="hw-time"
                type="time"
                className="w-32"
                value={time}
                onChange={(e) => setTime(e.target.value)}
              />
            </div>
          </div>
          <div className="grid gap-2">
            <Label>{t("Action")}</Label>
            <Pick
              value={current ? key : ""}
              placeholder={
                selected.length
                  ? t("Choisir une action")
                  : t("Choisissez d'abord les appareils")
              }
              onChange={pickAction}
              options={actions.map((a) => [a.key, tl(a.action.label)])}
              className="w-full sm:w-72"
            />
            {current && (
              <div className="flex flex-wrap gap-3">
                {current.action.params.map((f) => (
                  <ParamInput
                    key={f.name}
                    field={f}
                    value={params[f.name]}
                    onChange={(v) => setParams({ ...params, [f.name]: v })}
                  />
                ))}
              </div>
            )}
          </div>
          {current && current.action.params.length > 0 && (
            <div className="grid gap-2">
              <label className="flex items-center gap-2 text-sm font-medium">
                <Switch
                  checked={withEnd}
                  onCheckedChange={(on) => {
                    setWithEnd(on)
                    // At its end, what was switched on goes off (and the other way round).
                    if (on && !schedule?.end)
                      setEndParams(
                        Object.fromEntries(
                          Object.entries(params).map(([k, v]) => [
                            k,
                            typeof v === "boolean" ? !v : v,
                          ])
                        )
                      )
                  }}
                />
                {t("Et à une heure de fin")}
              </label>
              {withEnd && (
                <div className="flex flex-wrap items-center gap-3">
                  <Input
                    type="time"
                    className="w-32"
                    value={endTime}
                    aria-label={t("Heure de fin")}
                    onChange={(e) => setEndTime(e.target.value)}
                  />
                  {current.action.params.map((f) => (
                    <ParamInput
                      key={f.name}
                      field={f}
                      value={endParams[f.name]}
                      onChange={(v) =>
                        setEndParams({ ...endParams, [f.name]: v })
                      }
                    />
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            {t("Annuler")}
          </Button>
          <Button
            onClick={() => save.mutate()}
            disabled={
              save.isPending ||
              !name.trim() ||
              !days.length ||
              !chosen.size ||
              !current
            }
          >
            {save.isPending ? t("Envoi à l'appareil…") : t("Enregistrer")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function ScheduleRow({
  sourceId,
  s,
  devices,
  onEdit,
}: {
  sourceId: number
  s: HardwareSchedule
  devices: DeviceView[]
  onEdit: () => void
}) {
  const qc = useQueryClient()
  const traits = useQuery({ queryKey: ["traits"], queryFn: api.traits })
  const done = (list: HardwareSchedule[]) =>
    qc.setQueryData(["source-schedules", sourceId], list)
  const toggle = useMutation({
    mutationFn: (enabled: boolean) =>
      api.updateSourceSchedule(sourceId, s.id, { ...inputOf(s), enabled }),
    onSuccess: done,
    onError: (e) => toast.error(errorText(e)),
  })
  const remove = useMutation({
    mutationFn: () => api.deleteSourceSchedule(sourceId, s.id),
    onSuccess: (list) => {
      done(list)
      toast.success(t("Programmation supprimée de l'appareil"))
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const action = traits.data
    ?.find((x) => x.trait === s.trait)
    ?.actions.find((a) => a.name === s.action)
  const names = s.references.map(
    (r) => devices.find((d) => d.reference === r)?.name ?? r
  )
  return (
    <div
      className={cn(
        "flex flex-col gap-2 rounded-2xl border p-3",
        !s.enabled && "opacity-70"
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <CalendarClock className="size-4 shrink-0 text-primary" />
            <span className="truncate font-medium">{s.name}</span>
          </div>
          <p className="text-sm text-muted-foreground">
            {daysText(s.days, s.time)}
            {s.end && ` → ${s.end.time}`}
          </p>
        </div>
        <Switch
          checked={s.enabled}
          loading={toggle.isPending}
          disabled={!s.editable}
          onCheckedChange={(v) => toggle.mutate(v)}
          aria-label={t("Activer")}
        />
      </div>
      <div className="flex flex-wrap gap-1.5">
        <Badge>
          {action ? tl(action.label) : `${s.trait}.${s.action}`}
          {action && Object.keys(s.params).length > 0
            ? ` · ${paramsText(action.params, s.params)}`
            : ""}
        </Badge>
        {names.map((n) => (
          <Badge key={n} variant="outline">
            {n}
          </Badge>
        ))}
      </div>
      {s.note && <p className="text-xs text-muted-foreground">{s.note}</p>}
      {s.editable && (
        <div className="flex justify-end gap-1">
          <Button size="sm" variant="ghost" onClick={onEdit}>
            <Pencil /> {t("Modifier")}
          </Button>
          <Confirm
            title={t("Supprimer « {name} » ?", { name: s.name })}
            description={t("La programmation est supprimée de l'appareil.")}
            onConfirm={() => remove.mutate()}
          >
            <Button size="icon" variant="ghost" aria-label={t("Supprimer")}>
              <Trash2 />
            </Button>
          </Confirm>
        </div>
      )}
    </div>
  )
}

/**
 * The schedules a piece of hardware keeps and runs itself, for a source whose module offers them
 * (`schedules` in its manifest's `optional_calls`).
 */
export function HardwareSchedulesCard({
  sourceId,
  devices,
}: {
  sourceId: number
  devices: DeviceView[]
}) {
  const list = useQuery({
    queryKey: ["source-schedules", sourceId],
    queryFn: () => api.sourceSchedules(sourceId),
  })
  const [editing, setEditing] = useState<HardwareSchedule | "new" | null>(null)
  return (
    <Card className="rounded-3xl">
      <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-3">
        <div className="grid gap-1.5">
          <CardTitle>{t("Programmations de l'appareil")}</CardTitle>
          <CardDescription>
            {t(
              "Gardées et exécutées par l'appareil lui-même : elles continuent même quand Breezone est arrêté."
            )}
          </CardDescription>
        </div>
        <Button size="sm" variant="outline" onClick={() => setEditing("new")}>
          <Plus /> {t("Ajouter")}
        </Button>
      </CardHeader>
      <CardContent className="grid gap-2">
        {list.isError && (
          <p className="text-sm text-destructive">{errorText(list.error)}</p>
        )}
        {list.data?.length === 0 && (
          <p className="text-sm text-muted-foreground">
            {t("Aucune programmation dans l'appareil.")}
          </p>
        )}
        {list.data?.map((s) => (
          <ScheduleRow
            key={s.id}
            sourceId={sourceId}
            s={s}
            devices={devices}
            onEdit={() => setEditing(s)}
          />
        ))}
      </CardContent>
      {editing && (
        <ScheduleEditor
          sourceId={sourceId}
          devices={devices}
          schedule={editing === "new" ? undefined : editing}
          onClose={() => setEditing(null)}
        />
      )}
    </Card>
  )
}
