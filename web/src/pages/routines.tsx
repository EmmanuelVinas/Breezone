import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { ListPlus, Pencil, Play, Plus, Sparkles, Trash2 } from "lucide-react"
import { toast } from "sonner"
import type {
  Action,
  Installation,
  Mode,
  Routine,
  RoutineInput,
} from "@/lib/api"
import { api } from "@/lib/api"
import { useRights } from "@/hooks/use-rights"
import { MODE_LABEL, relativeTime } from "@/lib/format"
import { PageHeader } from "@/components/app-shell"
import { AirzoneSchedulesSection } from "@/components/airzone-schedules"
import { AirzoneRoutinesSection, TimersSection } from "@/components/timers"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Skeleton } from "@/components/ui/skeleton"
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
import { t, tn } from "@/lib/i18n"

type ZoneOption = {
  key: string
  label: string
  installation_id: number
  system_id: number
  zone_id: number
}

function zoneOptions(installations: Installation[]): ZoneOption[] {
  return installations.flatMap((i) =>
    i.zones.map((z) => ({
      key: `${i.id}-${z.system_id}-${z.zone_id}`,
      label: `${installations.length > 1 ? `${i.name} · ` : ""}${z.group ? `${z.group} · ` : ""}${z.name}`,
      installation_id: i.id,
      system_id: z.system_id,
      zone_id: z.zone_id,
    }))
  )
}

function describeAction(a: Action, options: ZoneOption[]) {
  const zone =
    options.find(
      (o) => o.key === `${a.installation_id}-${a.system_id}-${a.zone_id}`
    )?.label ?? t("Zone {id}", { id: a.zone_id })
  const parts = []
  if (a.on !== undefined) parts.push(a.on ? t("allumer") : t("éteindre"))
  if (a.setpoint !== undefined) parts.push(`${a.setpoint}°`)
  if (a.mode) parts.push(MODE_LABEL[a.mode].toLowerCase())
  const delay = a.delay_minutes
    ? ` (${DELAYS.find(([m]) => m === a.delay_minutes)?.[1].toLowerCase() ?? t("après {n} min", { n: a.delay_minutes })})`
    : ""
  return t("{zone} : {actions}", { zone, actions: parts.join(", ") }) + delay
}

type Draft = {
  name: string
  actions: Action[]
}

function toDraft(r?: Routine): Draft {
  return { name: r?.name ?? "", actions: r?.actions ?? [] }
}

/** When an action of a routine runs, after the routine starts. */
export const DELAYS: [number, string][] = [
  [0, t("Tout de suite")],
  [15, t("Après {n} min", { n: 15 })],
  [30, t("Après {n} min", { n: 30 })],
  [60, t("Après {h} h", { h: 1 })],
  [120, t("Après {h} h", { h: 2 })],
  [240, t("Après {h} h", { h: 4 })],
  [480, t("Après {h} h", { h: 8 })],
]

function ActionRow({
  action,
  options,
  onChange,
  onRemove,
}: {
  action: Action
  options: ZoneOption[]
  onChange: (a: Action) => void
  onRemove: () => void
}) {
  const key = `${action.installation_id}-${action.system_id}-${action.zone_id}`
  return (
    <div className="grid gap-2 rounded-2xl border bg-muted/30 p-3 lg:grid-cols-[minmax(12rem,1fr)_auto_auto_auto_auto_auto] lg:items-center">
      <Select
        value={key}
        onValueChange={(v) => {
          const o = options.find((x) => x.key === v)!
          onChange({
            ...action,
            installation_id: o.installation_id,
            system_id: o.system_id,
            zone_id: o.zone_id,
          })
        }}
      >
        <SelectTrigger className="w-full">
          <SelectValue placeholder={t("Zone")} />
        </SelectTrigger>
        <SelectContent>
          {options.map((o) => (
            <SelectItem key={o.key} value={o.key}>
              {o.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Select
        value={action.on === undefined ? "keep" : action.on ? "on" : "off"}
        onValueChange={(v) =>
          onChange({ ...action, on: v === "keep" ? undefined : v === "on" })
        }
      >
        <SelectTrigger className="w-full lg:w-32">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="keep">{t("Allumage inchangé")}</SelectItem>
          <SelectItem value="on">{t("Allumer")}</SelectItem>
          <SelectItem value="off">{t("Éteindre")}</SelectItem>
        </SelectContent>
      </Select>
      <Input
        type="number"
        step="0.5"
        min={15}
        max={30}
        placeholder={t("Consigne °")}
        className="sm:w-28"
        value={action.setpoint ?? ""}
        onChange={(e) =>
          onChange({
            ...action,
            setpoint:
              e.target.value === "" ? undefined : Number(e.target.value),
          })
        }
      />
      <Select
        value={action.mode ?? "keep"}
        onValueChange={(v) =>
          onChange({ ...action, mode: v === "keep" ? undefined : (v as Mode) })
        }
      >
        <SelectTrigger className="w-full lg:w-32">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="keep">{t("Mode inchangé")}</SelectItem>
          {(Object.keys(MODE_LABEL) as Mode[]).map((m) => (
            <SelectItem key={m} value={m}>
              {MODE_LABEL[m]}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Select
        value={String(action.delay_minutes ?? 0)}
        onValueChange={(v) =>
          onChange({ ...action, delay_minutes: Number(v) || null })
        }
      >
        <SelectTrigger className="w-full lg:w-36" aria-label={t("Quand")}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {DELAYS.map(([m, label]) => (
            <SelectItem key={m} value={String(m)}>
              {label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Button
        variant="ghost"
        size="icon"
        onClick={onRemove}
        aria-label={t("Retirer")}
      >
        <Trash2 />
      </Button>
    </div>
  )
}

function RoutineEditor({
  routine,
  open,
  onOpenChange,
}: {
  routine?: Routine
  open: boolean
  onOpenChange: (o: boolean) => void
}) {
  const qc = useQueryClient()
  const status = useQuery({ queryKey: ["status"], queryFn: api.status })
  const options = zoneOptions(status.data ?? [])
  const [draft, setDraft] = useState<Draft>(() => toDraft(routine))
  const set = (patch: Partial<Draft>) => setDraft((d) => ({ ...d, ...patch }))

  const save = useMutation({
    mutationFn: () => {
      const input: RoutineInput = {
        name: draft.name,
        enabled: true,
        actions: draft.actions,
      }
      return routine
        ? api.updateRoutine(routine.id, input)
        : api.createRoutine(input)
    },
    onSuccess: () => {
      toast.success(routine ? t("Scène modifiée") : t("Scène créée"))
      qc.invalidateQueries({ queryKey: ["routines"] })
      onOpenChange(false)
    },
    onError: (e) => toast.error(e instanceof Error ? e.message : String(e)),
  })

  const addZone = (o?: ZoneOption) => {
    const target = o ?? options[0]
    if (!target) return
    set({
      actions: [
        ...draft.actions,
        {
          installation_id: target.installation_id,
          system_id: target.system_id,
          zone_id: target.zone_id,
          on: true,
        },
      ],
    })
  }
  const addAll = () =>
    set({
      actions: [
        ...draft.actions,
        ...options
          .filter(
            (o) =>
              !draft.actions.some(
                (a) =>
                  `${a.installation_id}-${a.system_id}-${a.zone_id}` === o.key
              )
          )
          .map((o) => ({
            installation_id: o.installation_id,
            system_id: o.system_id,
            zone_id: o.zone_id,
            on: true,
          })),
      ],
    })

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90svh] overflow-x-hidden overflow-y-auto sm:max-w-[min(68rem,95vw)]">
        <DialogHeader>
          <DialogTitle>
            {routine ? t("Modifier la scène") : t("Nouvelle scène")}
          </DialogTitle>
          <DialogDescription>
            {t(
              "Lancée à la demande et exécutée par le serveur Breezone. Chaque action peut attendre (« après 2 h »). Pour une heure fixe, créez une programmation."
            )}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-5">
          <div className="grid gap-2">
            <Label htmlFor="rn">{t("Nom")}</Label>
            <Input
              id="rn"
              value={draft.name}
              placeholder={t("ex. Je pars")}
              onChange={(e) => set({ name: e.target.value })}
            />
          </div>
          <div className="grid gap-2">
            <div className="flex items-center justify-between">
              <Label>{t("Actions")}</Label>
              <div className="flex gap-1">
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={addAll}
                  disabled={!options.length}
                >
                  <ListPlus /> {t("Toutes les zones")}
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => addZone()}
                  disabled={!options.length}
                >
                  <Plus /> {t("Zone")}
                </Button>
              </div>
            </div>
            {draft.actions.length === 0 && (
              <p className="text-sm text-muted-foreground">
                {t("Ajoutez au moins une zone à piloter.")}
              </p>
            )}
            {draft.actions.map((a, i) => (
              <ActionRow
                key={i}
                action={a}
                options={options}
                onChange={(next) =>
                  set({
                    actions: draft.actions.map((x, j) => (j === i ? next : x)),
                  })
                }
                onRemove={() =>
                  set({ actions: draft.actions.filter((_, j) => j !== i) })
                }
              />
            ))}
          </div>
        </div>
        <DialogFooter>
          <Button
            onClick={() => save.mutate()}
            disabled={
              save.isPending || !draft.name.trim() || draft.actions.length === 0
            }
          >
            {t("Enregistrer")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function RoutineCard({
  routine,
  options,
  onEdit,
}: {
  routine: Routine
  options: ZoneOption[]
  /** Absent when the user may not change routines. */
  onEdit?: () => void
}) {
  const qc = useQueryClient()
  const run = useMutation({
    mutationFn: () => api.runRoutine(routine.id),
    onSuccess: (r) => {
      const failed = r.results.filter((x) => !x.ok).length
      if (failed)
        toast.warning(
          t("« {name} » : {failed} action(s) en échec", {
            name: routine.name,
            failed,
          })
        )
      else toast.success(t("« {name} » exécutée", { name: routine.name }))
    },
    onError: (e) => toast.error(e instanceof Error ? e.message : String(e)),
  })
  const remove = useMutation({
    mutationFn: () => api.deleteRoutine(routine.id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["routines"] }),
  })

  return (
    <Card className="rounded-3xl">
      <CardContent className="flex flex-col gap-4">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <Sparkles className="size-4 text-fan" />
              <h3 className="truncate font-semibold">{routine.name}</h3>
            </div>
            <p className="mt-1 text-sm text-muted-foreground">
              {tn(routine.actions.length, "{n} action", "{n} actions")}
              {routine.actions.some((a) => a.delay_minutes)
                ? t(" · dont certaines différées")
                : ""}
            </p>
          </div>
        </div>
        <ul className="space-y-1 text-sm">
          {routine.actions.slice(0, 4).map((a, i) => (
            <li key={i} className="truncate text-muted-foreground">
              · {describeAction(a, options)}
            </li>
          ))}
          {routine.actions.length > 4 && (
            <li className="text-muted-foreground">
              ·{" "}
              {tn(routine.actions.length - 4, "et {n} autre", "et {n} autres")}
            </li>
          )}
        </ul>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex flex-wrap gap-2 text-xs text-muted-foreground">
            {routine.last_run_at && (
              <span>
                {t("Dernière : {when}", {
                  when: relativeTime(routine.last_run_at),
                })}
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
              <Play /> {t("Lancer")}
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
                      {t("Supprimer « {name} » ?", { name: routine.name })}
                    </AlertDialogTitle>
                    <AlertDialogDescription>
                      {t("Cette scène ne pourra plus être lancée.")}
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

function BreezoneRoutines({
  title,
  description,
  empty,
}: {
  title: string
  description: string
  empty: string
}) {
  const routines = useQuery({ queryKey: ["routines"], queryFn: api.routines })
  const status = useQuery({ queryKey: ["status"], queryFn: api.status })
  const options = zoneOptions(status.data ?? [])
  const canEdit = useRights().routines
  const [editing, setEditing] = useState<{
    routine?: Routine
    nonce: number
  } | null>(null)
  const list = routines.data ?? []
  return (
    <section className="grid gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-lg font-semibold">{title}</h2>
          <p className="text-sm text-muted-foreground">{description}</p>
        </div>
        {canEdit && (
          <Button
            onClick={() => setEditing({ nonce: Date.now() })}
            disabled={!options.length}
          >
            <Plus /> {t("Nouvelle")}
          </Button>
        )}
      </div>
      {routines.isLoading ? (
        <div className="grid gap-4 md:grid-cols-2">
          <Skeleton className="h-40 rounded-3xl" />
          <Skeleton className="h-40 rounded-3xl" />
        </div>
      ) : list.length ? (
        <div className="grid gap-4 md:grid-cols-2">
          {list.map((r) => (
            <RoutineCard
              key={r.id}
              routine={r}
              options={options}
              onEdit={
                canEdit
                  ? () => setEditing({ routine: r, nonce: Date.now() })
                  : undefined
              }
            />
          ))}
        </div>
      ) : (
        <p className="rounded-3xl border border-dashed p-6 text-center text-sm text-muted-foreground">
          {empty}
        </p>
      )}
      {editing && (
        <RoutineEditor
          key={editing.nonce}
          routine={editing.routine}
          open
          onOpenChange={(o) => !o && setEditing(null)}
        />
      )}
    </section>
  )
}

/** Fixed-time schedules: those of the Airzone app, and Breezone's own. */
export function SchedulesPage() {
  return (
    <>
      <PageHeader
        title={t("Programmations")}
        description={t(
          "Ce qui se déclenche à heure fixe. Enregistrées dans votre compte Airzone et exécutées par Airzone ; modifiables ici comme dans l'application Airzone."
        )}
      />
      <div className="grid gap-10">
        <AirzoneSchedulesSection />
      </div>
    </>
  )
}

/** Things started on demand: timers, scenes with delayed steps, Airzone routines. */
export function RoutinesPage() {
  return (
    <>
      <PageHeader
        title={t("Routines")}
        description={t(
          "Ce que vous lancez quand vous voulez : une minuterie (« dans 2 h, éteindre l'étage ») ou une scène de plusieurs actions."
        )}
      />
      <div className="grid gap-10">
        <TimersSection />
        <BreezoneRoutines
          title={t("Scènes")}
          description={t(
            "Plusieurs actions lancées d'un geste ; chacune peut attendre (ex. éteindre le salon tout de suite, l'étage après 2 h)."
          )}
          empty={t(
            "Aucune scène. Exemple : « Je pars » — tout éteindre sauf la chambre, qui s'éteint après 1 h."
          )}
        />
        <AirzoneRoutinesSection />
      </div>
    </>
  )
}
