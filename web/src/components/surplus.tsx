import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Link } from "react-router"
import {
  AirVent,
  ArrowDown,
  ArrowUp,
  Pencil,
  PlugZap,
  Power,
  Sun,
  Trash2,
} from "lucide-react"
import { toast } from "sonner"
import type {
  SurplusLoad,
  SurplusLoadInput,
  SurplusStatus,
} from "@/lib/api"
import { api, currentUser } from "@/lib/api"
import { locale, t } from "@/lib/i18n"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
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

const kw = (w: number) =>
  `${(Math.round(w / 100) / 10).toLocaleString(locale(), { maximumFractionDigits: 1 })} kW`

const TONE: Record<SurplusStatus["state"], string> = {
  solar: "var(--sun, var(--warning))",
  guarantee: "var(--primary)",
  waiting: "var(--muted-foreground)",
  no_car: "var(--muted-foreground)",
  unavailable: "var(--destructive)",
  no_meter: "var(--destructive)",
  idle: "var(--muted-foreground)",
  manual: "var(--primary)",
}

const ICON = { charger: PlugZap, relay: Power, climate: AirVent }

/** What a zone is given when it follows the surplus, until changed. */
const CLIMATE_DEFAULTS = {
  heat_setpoint: 20,
  cool_setpoint: 24,
  heat_below: 14,
  cool_above: 25,
}

export function useSurplus() {
  return useQuery({
    queryKey: ["surplus"],
    queryFn: api.surplus,
    refetchInterval: 10_000,
  })
}

function useSave() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, input }: { id: number; input: SurplusLoadInput }) =>
      api.saveSurplusLoad(id, input),
    onSuccess: (v) => qc.setQueryData(["surplus"], v),
    onError: (e) => toast.error(e instanceof Error ? e.message : String(e)),
  })
}

/** What is set, as a load's settings are sent back. */
function inputOf(l: SurplusLoad): SurplusLoadInput {
  return {
    enabled: l.enabled,
    min_amps: l.min_amps,
    max_amps: l.max_amps,
    power: l.power,
    guarantee_kwh: l.guarantee_kwh,
    guarantee_by: l.guarantee_by,
    min_on_minutes: l.min_on_minutes,
    tolerance: l.tolerance,
    heat_setpoint: l.heat_setpoint,
    cool_setpoint: l.cool_setpoint,
    heat_below: l.heat_below,
    cool_above: l.cool_above,
  }
}

/** "6 à 16 A · +20 kWh garantis avant 07:00", "2 kW · 10 min au moins". */
function summary(l: SurplusLoad) {
  if (l.kind === "climate")
    return [
      l.power ? kw(l.power) : null,
      t("chauffe à {sp}° sous {t}° dehors", {
        sp: l.heat_setpoint ?? CLIMATE_DEFAULTS.heat_setpoint,
        t: l.heat_below ?? CLIMATE_DEFAULTS.heat_below,
      }),
      t("rafraîchit à {sp}° au-dessus de {t}°", {
        sp: l.cool_setpoint ?? CLIMATE_DEFAULTS.cool_setpoint,
        t: l.cool_above ?? CLIMATE_DEFAULTS.cool_above,
      }),
    ]
      .filter(Boolean)
      .join(" · ")
  if (l.kind === "relay")
    return [
      l.power ? kw(l.power) : null,
      l.min_on_minutes ? t("{n} min au moins", { n: l.min_on_minutes }) : null,
    ]
      .filter(Boolean)
      .join(" · ")
  return [
    t("{min} à {max} A", {
      min: l.min_amps ?? l.limit_min ?? 6,
      max: l.max_amps ?? Math.min(16, l.limit_max ?? 32),
    }),
    l.guarantee_kwh && l.guarantee_by
      ? t("+{kwh} kWh garantis avant {time}", {
          kwh: l.guarantee_kwh,
          time: l.guarantee_by,
        })
      : null,
  ]
    .filter(Boolean)
    .join(" · ")
}

function LoadDialog({
  load,
  onClose,
}: {
  load: SurplusLoad
  onClose: () => void
}) {
  const save = useSave()
  const [draft, setDraft] = useState<SurplusLoadInput>(inputOf(load))
  const set = (patch: Partial<SurplusLoadInput>) =>
    setDraft((d) => ({ ...d, ...patch }))
  const num = (v: string) => (v === "" ? null : Number(v))
  const lo = load.limit_min ?? 6
  const hi = load.limit_max ?? 32
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{load.name}</DialogTitle>
          <DialogDescription>
            {load.kind === "charger"
              ? t(
                  "Breezone règle l'intensité selon le surplus, entre un minimum et un maximum. Sous le minimum, la charge est suspendue."
                )
              : load.kind === "climate"
                ? t(
                    "Quand le surplus couvre sa puissance, Breezone allume la zone : en chauffage s'il fait froid dehors, en froid s'il fait chaud, selon le mode du système. Il la remet comme avant quand le surplus manque. Une zone déjà allumée ou réglée à la main n'est pas touchée."
                  )
                : t(
                    "Breezone allume l'appareil quand le surplus couvre sa puissance, et l'éteint quand il manque."
                  )}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-4">
          {load.kind === "charger" ? (
            <>
              <div className="grid grid-cols-2 gap-3">
                <div className="grid gap-1.5">
                  <Label htmlFor="min">{t("Intensité minimale (A)")}</Label>
                  <Input
                    id="min"
                    type="number"
                    min={lo}
                    max={hi}
                    value={draft.min_amps ?? lo}
                    onChange={(e) => set({ min_amps: num(e.target.value) })}
                  />
                </div>
                <div className="grid gap-1.5">
                  <Label htmlFor="max">{t("Intensité maximale (A)")}</Label>
                  <Input
                    id="max"
                    type="number"
                    min={lo}
                    max={hi}
                    value={draft.max_amps ?? Math.min(16, hi)}
                    onChange={(e) => set({ max_amps: num(e.target.value) })}
                  />
                </div>
              </div>
              <div className="grid gap-1.5">
                <Label>{t("Garantie")}</Label>
                <div className="flex flex-wrap items-center gap-2 text-sm">
                  <span>+</span>
                  <Input
                    type="number"
                    className="w-24"
                    min={0}
                    step="any"
                    placeholder="20"
                    value={draft.guarantee_kwh ?? ""}
                    onChange={(e) =>
                      set({ guarantee_kwh: num(e.target.value) })
                    }
                  />
                  <span>{t("kWh avant")}</span>
                  <Input
                    type="time"
                    className="w-32"
                    value={draft.guarantee_by ?? ""}
                    onChange={(e) =>
                      set({ guarantee_by: e.target.value || null })
                    }
                  />
                </div>
                <p className="text-xs text-muted-foreground">
                  {t(
                    "Facultatif. Si le soleil ne suffit pas, Breezone charge au maximum depuis le réseau, juste à temps."
                  )}
                </p>
              </div>
            </>
          ) : (
            <>
              {load.kind === "climate" && (
                <div className="grid grid-cols-2 gap-3">
                  <div className="grid gap-1.5">
                    <Label htmlFor="hs">{t("Chauffer à (°C)")}</Label>
                    <Input
                      id="hs"
                      type="number"
                      step={0.5}
                      value={
                        draft.heat_setpoint ?? CLIMATE_DEFAULTS.heat_setpoint
                      }
                      onChange={(e) =>
                        set({ heat_setpoint: num(e.target.value) })
                      }
                    />
                  </div>
                  <div className="grid gap-1.5">
                    <Label htmlFor="hb">
                      {t("quand il fait moins de (°C dehors)")}
                    </Label>
                    <Input
                      id="hb"
                      type="number"
                      step={0.5}
                      value={draft.heat_below ?? CLIMATE_DEFAULTS.heat_below}
                      onChange={(e) => set({ heat_below: num(e.target.value) })}
                    />
                  </div>
                  <div className="grid gap-1.5">
                    <Label htmlFor="cs">{t("Rafraîchir à (°C)")}</Label>
                    <Input
                      id="cs"
                      type="number"
                      step={0.5}
                      value={
                        draft.cool_setpoint ?? CLIMATE_DEFAULTS.cool_setpoint
                      }
                      onChange={(e) =>
                        set({ cool_setpoint: num(e.target.value) })
                      }
                    />
                  </div>
                  <div className="grid gap-1.5">
                    <Label htmlFor="ca">
                      {t("quand il fait plus de (°C dehors)")}
                    </Label>
                    <Input
                      id="ca"
                      type="number"
                      step={0.5}
                      value={draft.cool_above ?? CLIMATE_DEFAULTS.cool_above}
                      onChange={(e) => set({ cool_above: num(e.target.value) })}
                    />
                  </div>
                </div>
              )}
              <div className="grid grid-cols-2 gap-3">
                <div className="grid gap-1.5">
                  <Label htmlFor="power">
                    {load.kind === "climate"
                      ? t("Puissance estimée (W)")
                      : t("Puissance (W)")}
                  </Label>
                  <Input
                    id="power"
                    type="number"
                    min={1}
                    placeholder={load.kind === "climate" ? "1500" : "2000"}
                    value={draft.power ?? ""}
                    onChange={(e) => set({ power: num(e.target.value) })}
                  />
                </div>
                <div className="grid gap-1.5">
                  <Label htmlFor="minon">{t("Marche minimale (min)")}</Label>
                  <Input
                    id="minon"
                    type="number"
                    min={0}
                    value={draft.min_on_minutes ?? 0}
                    onChange={(e) =>
                      set({ min_on_minutes: Number(e.target.value) || 0 })
                    }
                  />
                </div>
              </div>
            </>
          )}
          <div className="grid gap-1.5">
            <Label htmlFor="tol">{t("Tolérance réseau (W)")}</Label>
            <Input
              id="tol"
              type="number"
              min={0}
              value={draft.tolerance ?? 0}
              onChange={(e) => set({ tolerance: Number(e.target.value) || 0 })}
            />
            <p className="text-xs text-muted-foreground">
              {t(
                "Ce que l'appareil peut tirer du réseau avant que Breezone ne réduise ou n'arrête."
              )}
            </p>
          </div>
        </div>
        <DialogFooter>
          <Button
            disabled={save.isPending}
            onClick={() =>
              save.mutate(
                { id: load.device_id, input: draft },
                { onSuccess: onClose }
              )
            }
          >
            {t("Enregistrer")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function StatusLine({ status }: { status: SurplusStatus | null }) {
  if (!status) return null
  return (
    <span className="text-xs" style={{ color: TONE[status.state] }}>
      {status.text}
    </span>
  )
}

/** The loads run on the solar surplus, in order of priority (energy page). */
export function SurplusCard() {
  const surplus = useSurplus()
  const qc = useQueryClient()
  const save = useSave()
  const admin = currentUser()?.role === "admin"
  const [editing, setEditing] = useState<SurplusLoad | null>(null)
  const onError = (e: unknown) =>
    toast.error(e instanceof Error ? e.message : String(e))
  const order = useMutation({
    mutationFn: api.orderSurplus,
    onSuccess: (v) => qc.setQueryData(["surplus"], v),
    onError,
  })
  const remove = useMutation({
    mutationFn: api.removeSurplusLoad,
    onSuccess: () => qc.invalidateQueries({ queryKey: ["surplus"] }),
    onError,
  })
  const v = surplus.data
  if (!v || (!v.loads.length && !v.candidates.length)) return null
  const move = (i: number, by: number) => {
    const ids = v.loads.map((l) => l.device_id)
    const [id] = ids.splice(i, 1)
    ids.splice(i + by, 0, id)
    order.mutate(ids)
  }
  return (
    <Card className="mt-6 rounded-3xl">
      <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-3">
        <div className="grid gap-1.5">
          <CardTitle className="flex items-center gap-2">
            <Sun className="size-5 text-warning" />
            {t("Pilotage sur surplus solaire")}
          </CardTitle>
          <CardDescription>
            {v.meter === false
              ? t(
                  "Il faut un compteur au tableau (une voie « réseau ») pour connaître le surplus."
                )
              : v.export !== null && v.export > 0
                ? t(
                    "Par ordre de priorité · {kw} repartent au réseau en ce moment",
                    { kw: kw(v.export) }
                  )
                : t(
                    "Par ordre de priorité : le surplus va d'abord au premier appareil."
                  )}
          </CardDescription>
        </div>
        {admin && v.candidates.length > 0 && (
          <Select
            value=""
            onValueChange={(id) => {
              const c = v.candidates.find((x) => x.device_id === Number(id))!
              // A relay or a zone needs its power: asked in the settings.
              if (c.kind === "relay" || c.kind === "climate")
                setEditing({
                  device_id: c.device_id,
                  name: c.name,
                  kind: c.kind,
                  automations: [],
                  heat_setpoint: null,
                  cool_setpoint: null,
                  heat_below: null,
                  cool_above: null,
                  position: v.loads.length,
                  enabled: true,
                  min_amps: null,
                  max_amps: null,
                  power: null,
                  guarantee_kwh: null,
                  guarantee_by: null,
                  min_on_minutes: c.kind === "climate" ? 30 : 10,
                  tolerance: 0,
                  limit_min: null,
                  limit_max: null,
                  status: null,
                })
              else save.mutate({ id: c.device_id, input: { enabled: true } })
            }}
          >
            <SelectTrigger className="w-auto">
              <SelectValue placeholder={t("Ajouter un appareil…")} />
            </SelectTrigger>
            <SelectContent>
              {v.candidates.map((c) => (
                <SelectItem key={c.device_id} value={String(c.device_id)}>
                  {c.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        )}
      </CardHeader>
      <CardContent>
        {v.loads.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            {t(
              "Aucun appareil ne suit le surplus. Ajoutez la borne, un chauffe-eau sur une prise pilotée, ou une zone de climatisation."
            )}
          </p>
        ) : (
          <ol className="grid">
            {v.loads.map((l, i) => {
              const Icon = ICON[l.kind]
              return (
                <li
                  key={l.device_id}
                  className="flex flex-wrap items-center gap-3 border-b py-3 last:border-b-0"
                >
                  <span className="grid size-7 place-items-center rounded-lg bg-muted text-xs text-muted-foreground">
                    {i + 1}
                  </span>
                  <Icon
                    className="size-5"
                    style={{
                      color: l.status ? TONE[l.status.state] : undefined,
                    }}
                  />
                  <div className="grid min-w-0 flex-1">
                    <span className="font-medium">{l.name}</span>
                    <span className="text-xs text-muted-foreground">
                      {summary(l)}
                    </span>
                    <StatusLine status={l.enabled ? l.status : null} />
                    {l.enabled && l.automations.length > 0 && (
                      <span className="text-xs text-warning">
                        {t(
                          "Aussi piloté par {names} : les deux se contrediraient.",
                          {
                            names: l.automations
                              .map((n) => t("« {name} »", { name: n }))
                              .join(", "),
                          }
                        )}
                      </span>
                    )}
                  </div>
                  {admin && (
                    <div className="flex items-center gap-1">
                      <Button
                        size="icon"
                        variant="ghost"
                        disabled={i === 0 || order.isPending}
                        onClick={() => move(i, -1)}
                        aria-label={t("Monter")}
                      >
                        <ArrowUp />
                      </Button>
                      <Button
                        size="icon"
                        variant="ghost"
                        disabled={i === v.loads.length - 1 || order.isPending}
                        onClick={() => move(i, 1)}
                        aria-label={t("Descendre")}
                      >
                        <ArrowDown />
                      </Button>
                      <Button
                        size="icon"
                        variant="ghost"
                        onClick={() => setEditing(l)}
                        aria-label={t("Modifier")}
                      >
                        <Pencil />
                      </Button>
                      <Button
                        size="icon"
                        variant="ghost"
                        onClick={() => remove.mutate(l.device_id)}
                        aria-label={t("Retirer")}
                      >
                        <Trash2 />
                      </Button>
                      <Switch
                        checked={l.enabled}
                        onCheckedChange={(enabled) =>
                          save.mutate({
                            id: l.device_id,
                            input: { ...inputOf(l), enabled },
                          })
                        }
                        aria-label={t("Suivre le surplus")}
                      />
                    </div>
                  )}
                </li>
              )
            })}
          </ol>
        )}
      </CardContent>
      {editing && (
        <LoadDialog
          key={editing.device_id}
          load={editing}
          onClose={() => setEditing(null)}
        />
      )}
    </Card>
  )
}

/**
 * The surplus steering as the energy page shows it: what it runs, in order, and what each does
 * now. Changed on the automations page only: one place sets what follows the surplus.
 */
export function SurplusProgramme() {
  const surplus = useSurplus()
  const loads = (surplus.data?.loads ?? []).filter((l) => l.enabled)
  if (loads.length === 0) return null
  return (
    <Card className="rounded-3xl border-dashed">
      <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-3">
        <div className="grid gap-1.5">
          <CardTitle className="flex items-center gap-2 text-base">
            <Sun className="size-4 text-warning" />
            {t("Pilotage sur surplus solaire")}
          </CardTitle>
          <CardDescription>
            {t(
              "Tourne tout seul, par ordre de priorité. Il se règle dans Automatismes ; un automatisme ne doit pas piloter les mêmes appareils."
            )}
          </CardDescription>
        </div>
        <Button asChild variant="outline" size="sm">
          <Link to="/routines">{t("Régler")}</Link>
        </Button>
      </CardHeader>
      <CardContent>
        <ol className="grid">
          {loads.map((l, i) => {
            const Icon = ICON[l.kind]
            return (
              <li
                key={l.device_id}
                className="flex items-center gap-3 border-b py-2 last:border-b-0"
              >
                <span className="grid size-6 place-items-center rounded-md bg-muted text-xs text-muted-foreground">
                  {i + 1}
                </span>
                <Icon
                  className="size-4"
                  style={{
                    color: l.status ? TONE[l.status.state] : undefined,
                  }}
                />
                <div className="grid min-w-0 flex-1">
                  <span className="text-sm font-medium">{l.name}</span>
                  <span className="text-xs text-muted-foreground">
                    {summary(l)}
                  </span>
                </div>
                <StatusLine status={l.status} />
              </li>
            )
          })}
        </ol>
      </CardContent>
    </Card>
  )
}
