import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  CalendarClock,
  Cloud,
  Pencil,
  Plus,
  RefreshCw,
  Trash2,
} from "lucide-react"
import { toast } from "sonner"
import type {
  AirzoneSchedule,
  AirzoneScheduleInput,
  Installation,
  Mode,
  SchedulesState,
  ZoneCommand,
} from "@/lib/api"
import { api } from "@/lib/api"
import { DAY_LABEL, DAY_SHORT, MODE_LABEL, relativeTime } from "@/lib/format"
import { cn } from "@/lib/utils"
import { Confirm } from "@/components/confirm"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Switch } from "@/components/ui/switch"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
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
import { t } from "@/lib/i18n"

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e))

function daysText(days: number[], time: string | null) {
  const d = [...days].sort()
  let text: string
  if (d.length === 7) text = t("Tous les jours")
  else if (d.join() === "1,2,3,4,5") text = t("En semaine")
  else if (d.join() === "6,7") text = t("Le week-end")
  else text = d.map((x) => DAY_LABEL[x - 1].slice(0, 3)).join(", ")
  return time ? t("{days} à {time}", { days: text, time }) : text
}

function actionText(a: ZoneCommand) {
  if (a.on === false) return t("Éteindre")
  const parts: string[] = []
  if (a.mode) parts.push(MODE_LABEL[a.mode])
  else if (a.on) parts.push(t("Allumer"))
  if (a.setpoint !== undefined && a.setpoint !== null)
    parts.push(`${a.setpoint}°`)
  return parts.join(" · ") || "—"
}

function zoneName(
  inst: Installation | undefined,
  z: { system_id: number; zone_id: number }
) {
  return (
    inst?.zones.find(
      (x) => x.system_id === z.system_id && x.zone_id === z.zone_id
    )?.name ?? t("Zone {id}", { id: z.zone_id })
  )
}

// ---------------------------------------------------------------------------------------
// Editor (same model as the Airzone app: zones, days, time, ON / OFF / mode)
// ---------------------------------------------------------------------------------------

type Kind = "on" | "off" | "mode"

function ScheduleEditor({
  schedule,
  cloudInstallations,
  installations,
  onClose,
}: {
  schedule?: AirzoneSchedule
  cloudInstallations: {
    cloud_id: string
    name: string
    installation_id: number | null
  }[]
  installations: Installation[]
  onClose: () => void
}) {
  const qc = useQueryClient()
  const linked = cloudInstallations.filter((c) =>
    installations.some(
      (i) => i.id === c.installation_id && i.controllable && i.rights.schedules
    )
  )
  const [cloudId, setCloudId] = useState(
    schedule?.cloud_id ?? linked[0]?.cloud_id ?? ""
  )
  const inst = installations.find(
    (i) =>
      i.id ===
      cloudInstallations.find((c) => c.cloud_id === cloudId)?.installation_id
  )
  const [name, setName] = useState(schedule?.name ?? "")
  const [days, setDays] = useState<string[]>(
    (schedule?.days ?? [1, 2, 3, 4, 5]).map(String)
  )
  const [time, setTime] = useState(schedule?.time ?? "07:00")
  const [zones, setZones] = useState<Set<string>>(
    () =>
      new Set((schedule?.zones ?? []).map((z) => `${z.system_id}-${z.zone_id}`))
  )
  const a = schedule?.action
  const [kind, setKind] = useState<Kind>(
    a?.on === false ? "off" : a?.mode ? "mode" : "on"
  )
  const [mode, setMode] = useState<Mode>(a?.mode ?? "heating")
  const [setpoint, setSetpoint] = useState(
    a?.setpoint != null ? String(a.setpoint) : ""
  )
  const modes = Array.from(
    new Set(
      inst?.zones.flatMap((z) => z.modes) ?? (["heating", "cooling"] as Mode[])
    )
  ).filter((m) => m !== "stop")
  const zoneList = inst
    ? inst.zones.map((z) => ({
        system_id: z.system_id,
        zone_id: z.zone_id,
        label: `${z.group ? `${z.group} · ` : ""}${z.name}`,
      }))
    : (schedule?.zones ?? []).map((z) => ({
        ...z,
        label: t("Zone {id}", { id: z.zone_id }),
      }))

  const save = useMutation({
    mutationFn: () => {
      const action: ZoneCommand =
        kind === "off"
          ? { on: false }
          : kind === "mode"
            ? { mode, ...(setpoint ? { setpoint: Number(setpoint) } : {}) }
            : { on: true, ...(setpoint ? { setpoint: Number(setpoint) } : {}) }
      const input: AirzoneScheduleInput = {
        cloud_id: cloudId,
        name: name.trim(),
        enabled: schedule?.enabled ?? true,
        days: days.map(Number),
        time,
        zones: [...zones].map((k) => {
          const [s, z] = k.split("-").map(Number)
          return { system_id: s, zone_id: z }
        }),
        action,
      }
      return schedule
        ? api.updateAirzoneSchedule(schedule.cloud_id, schedule.id, input)
        : api.createAirzoneSchedule(input)
    },
    onSuccess: (s) => {
      qc.setQueryData(["airzone-schedules"], s)
      toast.success(t("Programmation enregistrée chez Airzone"))
      onClose()
    },
    onError: (e) => toast.error(errorText(e)),
  })

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
              "Enregistrée dans votre compte Airzone, qui l'exécute — comme si vous l'aviez créée dans l'application Airzone."
            )}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-5">
          {!schedule && (
            <div className="grid gap-2">
              <Label>{t("Installation")}</Label>
              <Select
                value={cloudId}
                onValueChange={(v) => {
                  setCloudId(v)
                  setZones(new Set())
                }}
              >
                <SelectTrigger>
                  <SelectValue placeholder={t("Choisir")} />
                </SelectTrigger>
                <SelectContent>
                  {linked.map((c) => (
                    <SelectItem key={c.cloud_id} value={c.cloud_id}>
                      {c.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}
          <div className="grid gap-2">
            <Label htmlFor="az-name">{t("Nom")}</Label>
            <Input
              id="az-name"
              value={name}
              placeholder={t("ex. Réveil")}
              maxLength={11}
              onChange={(e) => setName(e.target.value)}
            />
          </div>
          <div className="grid gap-2">
            <Label>{t("Zones")}</Label>
            <div className="grid gap-1 sm:grid-cols-2">
              {zoneList.map((z) => {
                const k = `${z.system_id}-${z.zone_id}`
                return (
                  <label
                    key={k}
                    className="flex items-center gap-2 rounded-xl border p-2 text-sm"
                  >
                    <Checkbox
                      checked={zones.has(k)}
                      onCheckedChange={(v) =>
                        setZones((s) => {
                          const n = new Set(s)
                          if (v === true) n.add(k)
                          else n.delete(k)
                          return n
                        })
                      }
                    />
                    {z.label}
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
                    className="w-9 data-[state=on]:border-primary data-[state=on]:bg-primary data-[state=on]:text-primary-foreground"
                  >
                    {d}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </div>
            <div className="grid gap-2">
              <Label htmlFor="az-time">{t("Heure")}</Label>
              <Input
                id="az-time"
                type="time"
                className="w-32"
                value={time}
                onChange={(e) => setTime(e.target.value)}
              />
            </div>
          </div>
          <div className="grid gap-2">
            <Label>{t("Action")}</Label>
            <ToggleGroup
              type="single"
              variant="outline"
              value={kind}
              onValueChange={(v) => v && setKind(v as Kind)}
              className="justify-start"
            >
              <ToggleGroupItem
                value="on"
                className="data-[state=on]:border-primary data-[state=on]:bg-primary data-[state=on]:text-primary-foreground"
              >
                ON
              </ToggleGroupItem>
              <ToggleGroupItem
                value="off"
                className="data-[state=on]:border-primary data-[state=on]:bg-primary data-[state=on]:text-primary-foreground"
              >
                OFF
              </ToggleGroupItem>
              <ToggleGroupItem
                value="mode"
                className="data-[state=on]:border-primary data-[state=on]:bg-primary data-[state=on]:text-primary-foreground"
              >
                {t("Mode")}
              </ToggleGroupItem>
            </ToggleGroup>
            {kind !== "off" && (
              <div className="flex flex-wrap gap-3">
                {kind === "mode" && (
                  <Select
                    value={mode}
                    onValueChange={(v) => setMode(v as Mode)}
                  >
                    <SelectTrigger className="w-48">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {modes.map((m) => (
                        <SelectItem key={m} value={m}>
                          {MODE_LABEL[m]}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}
                <Input
                  type="number"
                  step="0.5"
                  className="w-36"
                  placeholder={t("Consigne (°)")}
                  value={setpoint}
                  onChange={(e) => setSetpoint(e.target.value)}
                />
              </div>
            )}
          </div>
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
              !zones.size ||
              !cloudId
            }
          >
            {save.isPending ? t("Envoi à Airzone…") : t("Enregistrer")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ---------------------------------------------------------------------------------------
// Section
// ---------------------------------------------------------------------------------------

function ScheduleCard({
  s,
  inst,
  onEdit,
}: {
  s: AirzoneSchedule
  inst: Installation | undefined
  /** Absent when the user may not change schedules (switching on/off stays possible). */
  onEdit?: () => void
}) {
  const qc = useQueryClient()
  const done = (r: SchedulesState) => qc.setQueryData(["airzone-schedules"], r)
  const toggle = useMutation({
    mutationFn: (enabled: boolean) =>
      api.setAirzoneScheduleEnabled(s.cloud_id, s.id, enabled),
    onSuccess: (r, enabled) => {
      done(r)
      toast.success(
        enabled
          ? t("« {name} » activée chez Airzone", { name: s.name })
          : t("« {name} » désactivée chez Airzone", { name: s.name })
      )
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const remove = useMutation({
    mutationFn: () => api.deleteAirzoneSchedule(s.cloud_id, s.id),
    onSuccess: (r) => {
      done(r)
      toast.success(t("Programmation supprimée chez Airzone"))
    },
    onError: (e) => toast.error(errorText(e)),
  })
  return (
    <Card className={cn("rounded-3xl", !s.enabled && "opacity-70")}>
      <CardContent className="flex flex-col gap-3">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <CalendarClock className="size-4 shrink-0 text-primary" />
              <h3 className="truncate font-semibold">{s.name}</h3>
            </div>
            <p className="text-sm text-muted-foreground">
              {s.time ? daysText(s.days, s.time) : t("Par dates")}
              {s.end && ` → ${s.end.time}`}
            </p>
          </div>
          <Switch
            checked={s.enabled}
            disabled={toggle.isPending}
            onCheckedChange={(v) => toggle.mutate(v)}
            aria-label={t("Activer")}
          />
        </div>
        <div className="flex flex-wrap gap-1.5">
          <Badge>{actionText(s.action)}</Badge>
          {s.zones.map((z) => (
            <Badge key={`${z.system_id}-${z.zone_id}`} variant="outline">
              {zoneName(inst, z)}
            </Badge>
          ))}
        </div>
        {s.note && <p className="text-xs text-muted-foreground">{s.note}</p>}
        <div className="flex justify-end gap-1">
          {s.editable && onEdit && (
            <Button size="sm" variant="ghost" onClick={onEdit}>
              <Pencil /> {t("Modifier")}
            </Button>
          )}
          {onEdit && (
            <Confirm
              title={`Supprimer « ${s.name} » ?`}
              description={t(
                "La programmation est supprimée de votre compte Airzone."
              )}
              onConfirm={() => remove.mutate()}
            >
              <Button size="icon" variant="ghost" aria-label={t("Supprimer")}>
                <Trash2 />
              </Button>
            </Confirm>
          )}
        </div>
      </CardContent>
    </Card>
  )
}

/** The schedules of the Airzone app, per installation: read and edited on Airzone Cloud. */
export function AirzoneSchedulesSection() {
  const qc = useQueryClient()
  const data = useQuery({
    queryKey: ["airzone-schedules"],
    queryFn: api.airzoneSchedules,
  })
  const status = useQuery({ queryKey: ["status"], queryFn: api.status })
  const cloud = useQuery({ queryKey: ["cloud"], queryFn: api.cloud })
  const editable = (installationId: number | null) =>
    !!status.data?.some(
      (i) => i.id === installationId && i.controllable && i.rights.schedules
    )
  const [editing, setEditing] = useState<{
    s?: AirzoneSchedule
    nonce: number
  } | null>(null)
  const refresh = useMutation({
    mutationFn: api.syncAirzoneSchedules,
    onSuccess: (r) => qc.setQueryData(["airzone-schedules"], r),
    onError: (e) => toast.error(errorText(e)),
  })
  const st = data.data
  const cloudInstallations = cloud.data?.installations ?? []
  if (!st || (!st.account && st.schedules.length === 0)) return null

  const groups = new Map<string, AirzoneSchedule[]>()
  for (const s of st.schedules) {
    groups.set(s.installation, [...(groups.get(s.installation) ?? []), s])
  }

  return (
    <section className="grid gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="flex items-center gap-2 text-lg font-semibold">
            <Cloud className="size-5" /> {t("Programmations Airzone")}
          </h2>
          <p className="text-sm text-muted-foreground">
            {st.last_sync
              ? t(
                  "Celles de l'application Airzone, exécutées par Airzone. Synchronisées toutes les heures · dernière {when}.",
                  {
                    when: relativeTime(st.last_sync),
                  }
                )
              : t(
                  "Celles de l'application Airzone, exécutées par Airzone. Synchronisées toutes les heures."
                )}
          </p>
          {st.last_error && (
            <p className="text-sm text-destructive">{st.last_error}</p>
          )}
        </div>
        <div className="flex gap-2">
          <Button
            variant="outline"
            onClick={() => refresh.mutate()}
            disabled={refresh.isPending}
          >
            <RefreshCw className={refresh.isPending ? "animate-spin" : ""} />{" "}
            {t("Actualiser")}
          </Button>
          {cloudInstallations.some((c) => editable(c.installation_id)) && (
            <Button onClick={() => setEditing({ nonce: Date.now() })}>
              <Plus /> {t("Nouvelle")}
            </Button>
          )}
        </div>
      </div>
      {[...groups.entries()].map(([installation, list]) => {
        const inst = status.data?.find((i) => i.id === list[0].installation_id)
        return (
          <div key={installation} className="grid gap-3">
            <h3 className="text-sm font-medium text-muted-foreground">
              {installation}
            </h3>
            <div className="grid gap-4 md:grid-cols-2">
              {list.map((s) => (
                <ScheduleCard
                  key={s.id}
                  s={s}
                  inst={inst}
                  onEdit={
                    editable(s.installation_id)
                      ? () => setEditing({ s, nonce: Date.now() })
                      : undefined
                  }
                />
              ))}
            </div>
          </div>
        )
      })}
      {st.schedules.length === 0 && (
        <p className="rounded-3xl border border-dashed p-6 text-center text-sm text-muted-foreground">
          {t("Aucune programmation dans l'application Airzone.")}
        </p>
      )}
      {editing && (
        <ScheduleEditor
          key={editing.nonce}
          schedule={editing.s}
          cloudInstallations={cloudInstallations}
          installations={status.data ?? []}
          onClose={() => setEditing(null)}
        />
      )}
    </section>
  )
}
