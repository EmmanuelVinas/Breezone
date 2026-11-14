import { useState } from "react"
import { Link } from "react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Area, AreaChart, CartesianGrid, XAxis, YAxis } from "recharts"
import { toast } from "sonner"
import {
  Car,
  Lock,
  LockOpen,
  Play,
  Plug,
  PlugZap,
  Square,
  Unplug,
} from "lucide-react"
import type { DeviceView } from "@/lib/api"
import { api, currentUser } from "@/lib/api"
import { CHARGER_STATUS, CHARGER_TONE } from "@/lib/charger"
import { duration, relativeTime, time } from "@/lib/format"
import { cn } from "@/lib/utils"
import { locale, t } from "@/lib/i18n"
import { PageHeader } from "@/components/app-shell"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
} from "@/components/ui/chart"
import type { ChartConfig } from "@/components/ui/chart"
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
import { Skeleton } from "@/components/ui/skeleton"
import { Slider } from "@/components/ui/slider"
import { Switch } from "@/components/ui/switch"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

/** The state of a charger, as `ev.charger@1` and `energy.power@1` describe it. */
type ChargerState = {
  status?: string
  vehicle?: boolean
  paused?: boolean
  locked?: boolean
  session_energy?: number
  session_secs?: number
  limit?: number
  limit_min?: number
  limit_max?: number
  auto_limit?: boolean
  energy_goal?: number
  power?: number
  current?: number
}

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e))

function number(v: number, digits: number) {
  return v.toLocaleString(locale(), { maximumFractionDigits: digits })
}

function kw(w: number | undefined) {
  if (w === undefined) return "–"
  return w >= 1000 ? `${number(w / 1000, 2)} kW` : `${Math.round(w)} W`
}

function kwh(v: number | undefined) {
  return v === undefined ? "–" : `${number(v, v < 10 ? 2 : 1)} kWh`
}

/** Paused only matters with a car plugged in. */
function shownStatus(s: ChargerState) {
  const plugged = s.status === "plugged" || s.status === "charging"
  return s.paused && plugged ? "paused" : (s.status ?? "fault")
}

const DRIVER_LABEL: Record<string, string> = { trydan: "V2C Trydan" }

const STATUS_TITLE: Record<string, string> = {
  ...CHARGER_STATUS,
  paused: t("En pause"),
}

function Tile({
  label,
  value,
  sub,
  color,
}: {
  label: string
  value: string
  sub?: string
  color?: string
}) {
  return (
    <div className="rounded-2xl bg-muted/60 px-3 py-2.5">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="tabular text-xl font-semibold" style={{ color }}>
        {value}
      </div>
      {sub && <div className="text-xs text-muted-foreground">{sub}</div>}
    </div>
  )
}

function useAct(device: DeviceView) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({
      action,
      params,
    }: {
      action: string
      params: Record<string, unknown>
    }) => api.act(device.id, "ev.charger", action, params),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["devices"] }),
    onError: (e) => toast.error(errorText(e)),
  })
}

function StartDialog({
  device,
  open,
  onClose,
}: {
  device: DeviceView
  open: boolean
  onClose: () => void
}) {
  const s = (device.state ?? {}) as ChargerState
  const min = s.limit_min ?? 6
  const max = s.limit_max ?? 32
  const [byBreezone, setByBreezone] = useState(!s.auto_limit)
  const [limit, setLimit] = useState(Math.min(max, s.limit ?? max))
  const [withGoal, setWithGoal] = useState(false)
  const [goal, setGoal] = useState("10")
  const act = useAct(device)
  const energy = Number(goal.replace(",", "."))
  const goalValid = !withGoal || (energy >= 0.1 && energy <= 200)
  const start = () =>
    act.mutate(
      {
        action: "start",
        params: {
          ...(byBreezone ? { limit } : {}),
          ...(withGoal ? { energy } : {}),
        },
      },
      { onSuccess: onClose }
    )
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t("Lancer la charge")}</DialogTitle>
          <DialogDescription>
            {t("La charge démarre dès qu'une voiture est branchée.")}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-5">
          <div className="grid gap-2">
            <Label>{t("Intensité")}</Label>
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              value={byBreezone ? "breezone" : "charger"}
              onValueChange={(v) => v && setByBreezone(v === "breezone")}
              className="justify-start"
            >
              <ToggleGroupItem value="breezone">
                {t("Maximum choisi")}
              </ToggleGroupItem>
              <ToggleGroupItem value="charger">
                {t("Réglée par la borne")}
              </ToggleGroupItem>
            </ToggleGroup>
            {byBreezone ? (
              <div className="flex items-center gap-3">
                <Slider
                  min={min}
                  max={max}
                  step={1}
                  value={[limit]}
                  onValueChange={([v]) => setLimit(v)}
                  aria-label={t("Intensité maximale")}
                />
                <span className="tabular w-24 shrink-0 text-right text-sm font-medium">
                  {limit} A · {number((limit * 230) / 1000, 1)} kW
                </span>
              </div>
            ) : (
              <p className="text-xs text-muted-foreground">
                {t(
                  "La borne adapte l'intensité selon sa propre pince (mode dynamique)."
                )}
              </p>
            )}
          </div>
          <div className="grid gap-2">
            <label className="flex items-center gap-2 text-sm font-medium">
              <Checkbox
                checked={withGoal}
                onCheckedChange={(v) => setWithGoal(v === true)}
              />
              {t("S'arrêter après")}
            </label>
            {withGoal && (
              <div className="flex items-center gap-2">
                <Input
                  value={goal}
                  inputMode="decimal"
                  className="w-28"
                  aria-label={t("Énergie à ajouter")}
                  onChange={(e) => setGoal(e.target.value)}
                />
                <span className="text-sm text-muted-foreground">
                  {t("kWh ajoutés")}
                </span>
              </div>
            )}
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("Annuler")}
          </Button>
          <Button onClick={start} disabled={!goalValid || act.isPending}>
            <Play /> {t("Lancer la charge")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

const curveConfig = {
  surplus: { label: t("Surplus solaire"), color: "var(--warning)" },
  power: { label: t("Puissance de charge"), color: "var(--primary)" },
} satisfies ChartConfig

/**
 * The charging power over the last 24 hours, and the solar surplus of today (what the home
 * returns to the grid, plus what the charger takes): it needs a main meter and a solar measure.
 */
function PowerCurve({ device }: { device: DeviceView }) {
  const history = useQuery({
    queryKey: ["device-history", device.id, 24],
    queryFn: () => api.deviceHistory(device.id, 24, 96),
    refetchInterval: 60_000,
  })
  const live = useQuery({ queryKey: ["energy-live"], queryFn: api.energyLive })
  const withSurplus = !!live.data?.roles.grid && !!live.data?.roles.solar
  const day = useQuery({
    queryKey: ["energy-day", "today"],
    queryFn: () => api.energyDay(),
    enabled: withSurplus,
    refetchInterval: 60_000,
  })
  const grid = new Map(
    (day.data?.points ?? [])
      .filter((p) => p.grid !== null)
      .map((p) => [p.ts, p.grid!])
  )
  const points = (history.data?.series.power ?? []).map(([ts, w]) => {
    const g = grid.get(ts)
    return {
      ts,
      power: w / 1000,
      surplus: g === undefined ? null : Math.max(0, -g + w) / 1000,
    }
  })
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{t("Dernières 24 heures")}</CardTitle>
        <CardDescription>
          {withSurplus
            ? t("Puissance de charge et surplus solaire disponible")
            : t("Puissance de charge")}
        </CardDescription>
      </CardHeader>
      <CardContent>
        {history.isLoading ? (
          <Skeleton className="h-56 w-full" />
        ) : points.length < 2 ? (
          <div className="flex h-56 items-center justify-center text-center text-sm text-muted-foreground">
            {t(
              "Pas encore assez de relevés : ils s'accumulent toutes les 5 minutes."
            )}
          </div>
        ) : (
          <ChartContainer
            config={curveConfig}
            className="aspect-auto h-56 w-full"
          >
            <AreaChart data={points} margin={{ left: 0, right: 8 }}>
              <CartesianGrid vertical={false} />
              <XAxis
                dataKey="ts"
                type="number"
                domain={["dataMin", "dataMax"]}
                scale="time"
                tickFormatter={(ts: number) => time(ts)}
                tickLine={false}
                axisLine={false}
                minTickGap={48}
              />
              <YAxis
                tickLine={false}
                axisLine={false}
                width={56}
                tickFormatter={(v: number) => kw(v * 1000)}
              />
              <ChartTooltip
                content={
                  <ChartTooltipContent
                    labelFormatter={(_, p) =>
                      p?.[0] ? time(Number(p[0].payload.ts)) : ""
                    }
                    formatter={(value, name, item) => (
                      <div className="flex w-full items-center justify-between gap-4">
                        <span className="flex items-center gap-1.5 text-muted-foreground">
                          <span
                            className="size-2 rounded-[2px]"
                            style={{ background: item.color }}
                          />
                          {curveConfig[name as keyof typeof curveConfig]?.label}
                        </span>
                        <span className="tabular font-medium">
                          {kw(Number(value) * 1000)}
                        </span>
                      </div>
                    )}
                  />
                }
              />
              {withSurplus && (
                <Area
                  dataKey="surplus"
                  type="monotone"
                  stroke="var(--color-surplus)"
                  fill="var(--color-surplus)"
                  fillOpacity={0.14}
                  strokeWidth={1.5}
                  connectNulls={false}
                  isAnimationActive={false}
                />
              )}
              <Area
                dataKey="power"
                type="stepAfter"
                stroke="var(--color-power)"
                fill="var(--color-power)"
                fillOpacity={0.25}
                strokeWidth={2}
                isAnimationActive={false}
              />
            </AreaChart>
          </ChartContainer>
        )}
      </CardContent>
    </Card>
  )
}

/** The current Breezone asks for, and a link to the module (who sets the current, address). */
function SettingsCard({
  device,
  admin,
}: {
  device: DeviceView
  admin: boolean
}) {
  const s = (device.state ?? {}) as ChargerState
  const act = useAct(device)
  const min = s.limit_min ?? 6
  const max = s.limit_max ?? 32
  // Draft while sliding; otherwise what the charger says.
  const [draft, setDraft] = useState<number | null>(null)
  const limit = draft ?? s.limit ?? min
  const disabled = !admin || !device.available || act.isPending
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{t("Réglages de la borne")}</CardTitle>
        <CardDescription>
          {s.auto_limit
            ? t(
                "La borne adapte l'intensité selon sa propre pince (mode dynamique)."
              )
            : t("Intensité maximale demandée par Breezone pendant la charge")}
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-5">
        {!s.auto_limit && (
          <div className="grid gap-2">
            <div className="flex items-center justify-between text-sm">
              <span>{t("Intensité maximale")}</span>
              <span className="tabular font-medium">
                {limit} A · {number((limit * 230) / 1000, 1)} kW
              </span>
            </div>
            <Slider
              min={min}
              max={max}
              step={1}
              value={[limit]}
              disabled={disabled}
              onValueChange={([v]) => setDraft(v)}
              onValueCommit={([v]) =>
                act.mutate(
                  { action: "set_limit", params: { limit: v } },
                  { onSettled: () => setDraft(null) }
                )
              }
              aria-label={t("Intensité maximale")}
            />
            <div className="flex justify-between text-xs text-muted-foreground">
              <span>{min} A</span>
              <span>{max} A</span>
            </div>
          </div>
        )}
        <div className="flex flex-wrap items-center justify-between gap-2 rounded-2xl bg-muted/60 px-3 py-2.5 text-sm">
          <span className="text-muted-foreground">
            {t("Plage de la borne : {min}–{max} A", { min, max })}
          </span>
          {admin && device.source_id !== null && (
            <Link
              to={`/settings?tab=modules&module=${device.source_id}`}
              className="font-medium text-primary hover:underline"
            >
              {t("Réglages du module ›")}
            </Link>
          )}
        </div>
      </CardContent>
    </Card>
  )
}

/** The states every charger shares, whatever its brand. */
function StatesCard() {
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{t("États de la borne")}</CardTitle>
        <CardDescription>
          {t(
            "Vocabulaire commun à toutes les bornes, quel que soit le protocole"
          )}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-wrap gap-2">
        {Object.entries(STATUS_TITLE).map(([k, label]) => (
          <Badge key={k} variant="outline" className="gap-1.5">
            <span
              className="size-2 rounded-full"
              style={{ background: CHARGER_TONE[k] }}
            />
            {label}
          </Badge>
        ))}
      </CardContent>
    </Card>
  )
}

/** `readAt`: when the state was read (seconds). */
function ChargerCard({
  device,
  readAt,
}: {
  device: DeviceView
  readAt: number
}) {
  const admin = currentUser()?.role === "admin"
  const s = (device.state ?? {}) as ChargerState
  const status = device.available ? shownStatus(s) : "fault"
  const tone = CHARGER_TONE[status] ?? "var(--muted-foreground)"
  const act = useAct(device)
  const [starting, setStarting] = useState(false)
  const charging = s.status === "charging" && !s.paused
  const progress =
    s.energy_goal && s.session_energy !== undefined
      ? Math.min(1, s.session_energy / s.energy_goal)
      : null
  const disabled = !admin || !device.available || act.isPending
  // With an energy goal and a car charging: when it is reached at the current power.
  const left = s.energy_goal ? s.energy_goal - (s.session_energy ?? 0) : null
  const finish =
    charging && left !== null && left > 0 && (s.power ?? 0) > 100
      ? time(Math.round(readAt + (left / (s.power! / 1000)) * 3600))
      : null

  return (
    <div className="grid gap-4">
      <Card
        className="rounded-3xl"
        style={{
          background: `radial-gradient(70% 90% at 0% 0%, color-mix(in oklch, ${tone} 14%, transparent), transparent 70%), var(--card)`,
        }}
      >
        <CardContent className="grid gap-5">
          <div className="flex flex-wrap items-center justify-between gap-4">
            <div className="flex items-center gap-3.5">
              <span
                className="grid size-14 place-items-center rounded-2xl"
                style={{
                  background: `color-mix(in oklch, ${tone} 20%, transparent)`,
                  color: tone,
                }}
              >
                {charging ? (
                  <PlugZap className="size-7" />
                ) : (
                  <Car className="size-7" />
                )}
              </span>
              <div>
                <div className="flex flex-wrap items-center gap-2">
                  <h2 className="font-heading text-xl font-semibold">
                    {device.name}
                  </h2>
                  <Badge variant="outline" className="gap-1.5">
                    <span
                      className="size-2 rounded-full"
                      style={{ background: tone }}
                    />
                    {device.available ? STATUS_TITLE[status] : t("Injoignable")}
                  </Badge>
                </div>
                <div className="flex flex-wrap items-center gap-x-2 text-sm text-muted-foreground">
                  {s.vehicle !== undefined && (
                    <span
                      className="inline-flex items-center gap-1.5"
                      style={{ color: s.vehicle ? tone : undefined }}
                    >
                      {s.vehicle ? (
                        <Plug className="size-3.5" />
                      ) : (
                        <Unplug className="size-3.5" />
                      )}
                      {s.vehicle
                        ? t("Véhicule branché")
                        : t("Aucun véhicule branché")}
                    </span>
                  )}
                  {s.vehicle !== undefined && <span aria-hidden>·</span>}
                  <span>
                    {s.session_secs
                      ? t("Session : {duration}", {
                          duration: duration(s.session_secs),
                        })
                      : t("Aucune session en cours")}
                  </span>
                </div>
              </div>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <label className="flex items-center gap-2 text-sm text-muted-foreground">
                {s.locked ? (
                  <Lock className="size-4" />
                ) : (
                  <LockOpen className="size-4" />
                )}
                {t("Verrouillée")}
                <Switch
                  checked={!!s.locked}
                  disabled={disabled}
                  onCheckedChange={(v) =>
                    act.mutate({ action: "lock", params: { locked: v } })
                  }
                />
              </label>
              {charging ? (
                <Button
                  variant="outline"
                  disabled={disabled}
                  onClick={() => act.mutate({ action: "stop", params: {} })}
                >
                  <Square /> {t("Arrêter")}
                </Button>
              ) : (
                <Button disabled={disabled} onClick={() => setStarting(true)}>
                  <Play /> {t("Lancer la charge")}
                </Button>
              )}
            </div>
          </div>
          <div className="grid grid-cols-2 gap-2 md:grid-cols-5">
            <Tile
              label={t("Puissance")}
              value={kw(s.power)}
              sub={
                typeof s.current === "number" && s.current > 0
                  ? `${number(s.current, 1)} A`
                  : undefined
              }
              color={charging ? "var(--primary)" : undefined}
            />
            <Tile
              label={t("Énergie ajoutée")}
              value={kwh(s.session_energy)}
              sub={
                s.energy_goal
                  ? t("arrêt à {kwh}", { kwh: kwh(s.energy_goal) })
                  : undefined
              }
            />
            <Tile
              label={t("Intensité")}
              value={s.auto_limit ? t("Auto") : s.limit ? `${s.limit} A` : "–"}
              sub={
                s.auto_limit
                  ? t("réglée par la borne")
                  : t("maximum fixé par Breezone")
              }
            />
            <Tile
              label={t("Objectif")}
              value={
                s.energy_goal
                  ? `+${kwh(s.energy_goal - (s.session_energy ?? 0) > 0 ? s.energy_goal - (s.session_energy ?? 0) : 0)}`
                  : "–"
              }
              sub={s.energy_goal ? t("restants") : t("aucun")}
            />
            <Tile
              label={t("Fin estimée")}
              value={finish ?? "–"}
              sub={finish ? t("au rythme actuel") : undefined}
            />
          </div>
          {progress !== null && (
            <div>
              <div className="mb-1.5 flex justify-between text-xs text-muted-foreground">
                <span>
                  {kwh(s.session_energy)} / {kwh(s.energy_goal)}
                </span>
                <span>{Math.round(progress * 100)} %</span>
              </div>
              <div className="h-2.5 overflow-hidden rounded-full bg-muted">
                <div
                  className="h-full rounded-full bg-primary transition-all"
                  style={{ width: `${progress * 100}%` }}
                />
              </div>
            </div>
          )}
        </CardContent>
      </Card>
      <div className="grid gap-4 lg:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)]">
        <PowerCurve device={device} />
        <SettingsCard
          key={`${s.limit}-${s.auto_limit}`}
          device={device}
          admin={admin}
        />
      </div>
      <StatesCard />
      {starting && (
        <StartDialog
          device={device}
          open={starting}
          onClose={() => setStarting(false)}
        />
      )}
    </div>
  )
}

export function ChargingPage() {
  const devices = useQuery({
    queryKey: ["devices"],
    queryFn: api.devices,
    refetchInterval: 5_000,
  })
  const chargers = (devices.data ?? []).filter((d) =>
    d.traits.includes("ev.charger@1")
  )
  const admin = currentUser()?.role === "admin"
  return (
    <>
      <PageHeader
        title={t("Recharge")}
        description={
          chargers.length === 1
            ? [
                chargers[0].name,
                DRIVER_LABEL[chargers[0].source] ?? chargers[0].source,
                typeof chargers[0].state?.limit_max === "number"
                  ? t("{kw} kW max", {
                      kw: number(
                        ((chargers[0].state.limit_max as number) * 230) / 1000,
                        1
                      ),
                    })
                  : null,
              ]
                .filter(Boolean)
                .join(" · ")
            : undefined
        }
        actions={
          chargers.length > 0 && (
            <Badge variant="outline" className="gap-1.5">
              <span
                className={cn(
                  "size-2 rounded-full",
                  chargers.every((c) => c.available)
                    ? "bg-success"
                    : "bg-destructive"
                )}
              />
              {chargers.every((c) => c.available)
                ? t("Connectée")
                : t("Injoignable")}{" "}
              ·{" "}
              {t("relevé {when}", {
                when: relativeTime(devices.dataUpdatedAt / 1000),
              })}
            </Badge>
          )
        }
      />
      {devices.isLoading ? (
        <Skeleton className="h-72 w-full rounded-3xl" />
      ) : chargers.length === 0 ? (
        <Card className="rounded-3xl">
          <CardContent className="flex flex-col items-center gap-3 py-12 text-center">
            <span className="grid size-12 place-items-center rounded-2xl bg-primary/15 text-primary">
              <PlugZap className="size-6" />
            </span>
            <div className="font-heading text-lg font-semibold">
              {t("Aucune borne")}
            </div>
            <p className="max-w-md text-sm text-muted-foreground">
              {t(
                "Ajoutez votre borne (V2C Trydan) dans Réglages › Modules : Breezone la trouve sur le réseau."
              )}
            </p>
            {admin && (
              <Button asChild>
                <Link to="/settings?tab=modules">
                  {t("Ouvrir Réglages › Modules")}
                </Link>
              </Button>
            )}
          </CardContent>
        </Card>
      ) : (
        <div className="grid gap-8">
          {chargers.map((d) => (
            <ChargerCard
              key={d.id}
              device={d}
              readAt={devices.dataUpdatedAt / 1000}
            />
          ))}
        </div>
      )}
    </>
  )
}
