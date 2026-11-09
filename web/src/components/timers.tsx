import { useEffect, useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Cloud, Hourglass, Plus, X } from "lucide-react"
import { toast } from "sonner"
import type { Installation, Mode, TimerTarget, ZoneCommand } from "@/lib/api"
import { api } from "@/lib/api"
import { MODE_LABEL } from "@/lib/format"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { locale, t } from "@/lib/i18n"

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e))

const DELAYS: [number, string][] = [
  [15, t("{n} min", { n: 15 })],
  [30, t("{n} min", { n: 30 })],
  [60, t("{h} h", { h: 1 })],
  [90, t("{h} h {m}", { h: 1, m: 30 })],
  [120, t("{h} h", { h: 2 })],
  [180, t("{h} h", { h: 3 })],
  [240, t("{h} h", { h: 4 })],
  [480, t("{h} h", { h: 8 })],
  [720, t("{h} h", { h: 12 })],
]

/** Targets: everything, then per installation the installation, its groups and its zones. */
function targets(installations: Installation[]) {
  const list: {
    key: string
    label: string
    target: TimerTarget
    group: string
  }[] = []
  if (installations.length > 1)
    list.push({
      key: "all",
      label: t("Toutes les installations"),
      target: { kind: "all" },
      group: "",
    })
  for (const i of installations.filter((i) => i.controllable)) {
    list.push({
      key: `i-${i.id}`,
      label: t("{name} (toute l'installation)", { name: i.name }),
      target: { kind: "installation", installation_id: i.id },
      group: i.name,
    })
    const groups = Array.from(
      new Set(i.zones.map((z) => z.group).filter((g): g is string => !!g))
    )
    for (const g of groups)
      list.push({
        key: `g-${i.id}-${g}`,
        label: t("Groupe {name}", { name: g }),
        target: { kind: "group", installation_id: i.id, group: g },
        group: i.name,
      })
    for (const z of i.zones)
      list.push({
        key: `z-${i.id}-${z.system_id}-${z.zone_id}`,
        label: z.name,
        target: {
          kind: "zone",
          installation_id: i.id,
          system_id: z.system_id,
          zone_id: z.zone_id,
        },
        group: i.name,
      })
  }
  return list
}

/** "dans 1 h 58", refreshed every 30 s. */
function useCountdown(ts: number) {
  const [now, setNow] = useState(() => Date.now() / 1000)
  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now() / 1000), 30_000)
    return () => clearInterval(tick)
  }, [])
  const left = Math.max(0, Math.round((ts - now) / 60))
  if (left < 1) return t("dans un instant")
  const h = Math.floor(left / 60)
  const m = left % 60
  return h
    ? t("dans {h} h {m}", { h, m: String(m).padStart(2, "0") })
    : t("dans {m} min", { m })
}

function TimerRow({
  id,
  label,
  runAt,
  by,
}: {
  id: number
  label: string
  runAt: number
  by: string | null
}) {
  const qc = useQueryClient()
  const countdown = useCountdown(runAt)
  const cancel = useMutation({
    mutationFn: () => api.cancelTimer(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["timers"] })
      toast.success(t("Minuterie annulée"))
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const at = new Date(runAt * 1000).toLocaleTimeString(locale(), {
    hour: "2-digit",
    minute: "2-digit",
  })
  return (
    <div className="flex items-center gap-3 rounded-2xl border p-3">
      <Hourglass className="size-4 shrink-0 text-primary" />
      <div className="min-w-0 flex-1">
        <div className="truncate font-medium">{label}</div>
        <div className="text-xs text-muted-foreground">
          {countdown} · {t("à {time}", { time: at })}
          {by ? ` · ${by}` : ""}
        </div>
      </div>
      <Button
        size="sm"
        variant="ghost"
        onClick={() => cancel.mutate()}
        disabled={cancel.isPending}
      >
        <X /> {t("Annuler")}
      </Button>
    </div>
  )
}

/** "Dans 2 h, éteindre l'étage": quick timers on a zone, a group, an installation or everything. */
export function TimersSection() {
  const qc = useQueryClient()
  const timers = useQuery({
    queryKey: ["timers"],
    queryFn: api.timers,
    refetchInterval: 30_000,
  })
  const status = useQuery({ queryKey: ["status"], queryFn: api.status })
  const options = targets(status.data ?? [])
  const [delay, setDelay] = useState("120")
  const [targetKey, setTargetKey] = useState("")
  const [action, setAction] = useState<"off" | "on" | "setpoint" | "mode">(
    "off"
  )
  const [setpoint, setSetpoint] = useState("19")
  const [mode, setMode] = useState<Mode>("heating")
  const target = options.find((o) => o.key === targetKey) ?? options[0]

  const create = useMutation({
    mutationFn: () => {
      const command: ZoneCommand =
        action === "off"
          ? { on: false }
          : action === "on"
            ? { on: true }
            : action === "setpoint"
              ? { setpoint: Number(setpoint) }
              : { mode }
      return api.createTimer({
        delay_minutes: Number(delay),
        target: target!.target,
        command,
      })
    },
    onSuccess: (timer) => {
      qc.invalidateQueries({ queryKey: ["timers"] })
      toast.success(t("Minuterie : {label}", { label: timer.label }))
    },
    onError: (e) => toast.error(errorText(e)),
  })

  const byGroup = new Map<string, typeof options>()
  for (const o of options)
    byGroup.set(o.group, [...(byGroup.get(o.group) ?? []), o])

  return (
    <section className="grid gap-4">
      <div>
        <h2 className="flex items-center gap-2 text-lg font-semibold">
          <Hourglass className="size-5" /> {t("Minuteries")}
        </h2>
        <p className="text-sm text-muted-foreground">
          {t(
            "Une action plus tard sur une zone, un groupe ou toute une installation."
          )}
        </p>
      </div>
      <Card className="rounded-3xl">
        <CardContent className="flex flex-wrap items-end gap-3">
          <div className="grid gap-1.5">
            <Label>{t("Dans")}</Label>
            <Select value={delay} onValueChange={setDelay}>
              <SelectTrigger className="w-28">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {DELAYS.map(([m, l]) => (
                  <SelectItem key={m} value={String(m)}>
                    {l}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-1.5">
            <Label>{t("Action")}</Label>
            <Select
              value={action}
              onValueChange={(v) => setAction(v as typeof action)}
            >
              <SelectTrigger className="w-36">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="off">{t("Éteindre")}</SelectItem>
                <SelectItem value="on">{t("Allumer")}</SelectItem>
                <SelectItem value="setpoint">{t("Consigne")}</SelectItem>
                <SelectItem value="mode">{t("Mode")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          {action === "setpoint" && (
            <div className="grid gap-1.5">
              <Label htmlFor="tm-sp">°C</Label>
              <Input
                id="tm-sp"
                type="number"
                step="0.5"
                className="w-24"
                value={setpoint}
                onChange={(e) => setSetpoint(e.target.value)}
              />
            </div>
          )}
          {action === "mode" && (
            <div className="grid gap-1.5">
              <Label>{t("Mode")}</Label>
              <Select value={mode} onValueChange={(v) => setMode(v as Mode)}>
                <SelectTrigger className="w-44">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {(["heating", "cooling", "fan", "dry", "auto"] as Mode[]).map(
                    (m) => (
                      <SelectItem key={m} value={m}>
                        {MODE_LABEL[m]}
                      </SelectItem>
                    )
                  )}
                </SelectContent>
              </Select>
            </div>
          )}
          <div className="grid min-w-56 flex-1 gap-1.5">
            <Label>{t("Sur")}</Label>
            <Select value={target?.key ?? ""} onValueChange={setTargetKey}>
              <SelectTrigger>
                <SelectValue placeholder={t("Choisir")} />
              </SelectTrigger>
              <SelectContent>
                {[...byGroup.entries()].map(([g, list]) => (
                  <SelectGroup key={g || "_"}>
                    {g && <SelectLabel>{g}</SelectLabel>}
                    {list.map((o) => (
                      <SelectItem key={o.key} value={o.key}>
                        {o.label}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                ))}
              </SelectContent>
            </Select>
          </div>
          <Button
            onClick={() => create.mutate()}
            disabled={!target || create.isPending}
          >
            <Plus /> {t("Programmer")}
          </Button>
        </CardContent>
      </Card>
      {(timers.data ?? []).map((timer) => (
        <TimerRow
          key={timer.id}
          id={timer.id}
          label={timer.label}
          runAt={timer.run_at}
          by={timer.created_by}
        />
      ))}
    </section>
  )
}

/** Routines of the Airzone app (read at the hourly synchronisation; import to come). */
export function AirzoneRoutinesSection() {
  const routines = useQuery({
    queryKey: ["airzone-routines"],
    queryFn: api.airzoneRoutines,
  })
  const list = routines.data ?? []
  if (!list.length) return null
  return (
    <section className="grid gap-3">
      <div>
        <h2 className="flex items-center gap-2 text-lg font-semibold">
          <Cloud className="size-5" /> {t("Routines Airzone")}
        </h2>
        <p className="text-sm text-muted-foreground">
          {t(
            "Celles de votre compte Airzone. Leur import en scènes Breezone arrive bientôt."
          )}
        </p>
      </div>
      <div className="flex flex-wrap gap-2">
        {list.map((r, i) => (
          <Badge key={r.id ?? i} variant="outline" className="px-3 py-1">
            {r.name} · {r.installation}
          </Badge>
        ))}
      </div>
    </section>
  )
}
