import { useMemo, useState } from "react"
import { Link } from "react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { toast } from "sonner"
import { Droplets, Fan, Loader2, Square } from "lucide-react"
import type { DeviceView, Room } from "@/lib/api"
import { api, currentUser } from "@/lib/api"
import type {
  VentilationRoomState,
  VentilationUnitState,
} from "@/lib/traits.generated"
import { t, tn } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import { PageHeader } from "@/components/app-shell"
import { Board } from "@/components/overview/board"
import { useOverview } from "@/components/overview/context"
import { useHomeData } from "@/components/overview/home-data"
import { ventilationLayout } from "@/components/overview/model"
import { SelectionContext, useSelection } from "@/components/overview/selection"
import { RoomSelect } from "@/components/room-select"
import { Sparkline, fieldFormat } from "@/components/sparkline"
import { PeriodPicker } from "@/components/period-picker"
import type { HistoryPeriod } from "@/lib/periods"
import {
  bucketFor,
  PERIOD_HOURS,
  PERIOD_LABEL,
  periodsFor,
} from "@/lib/periods"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { errorText, number } from "@/lib/format"

export const isUnit = (d: DeviceView) => d.traits.includes("ventilation.unit@1")
export const isRoom = (d: DeviceView) => d.traits.includes("ventilation.room@1")
const roomState = (d: DeviceView) =>
  (d.state ?? {}) as Partial<VentilationRoomState>
const unitState = (d: DeviceView) =>
  (d.state ?? {}) as Partial<VentilationUnitState>

const PROFILE: Record<string, string> = {
  eco: t("Eco"),
  health: t("Health"),
  intense: t("Intense"),
}

const CAUSE: Record<string, string> = {
  humidity: t("l'humidité"),
  co2: t("le CO₂"),
  voc: t("les COV"),
  other: t("la qualité de l'air"),
}

type Quality = "ok" | "warn" | "bad"

/**
 * How the unit judges the room (only with the Renson API key), with the thresholds of its app: up
 * to 40 good, above 100 bad. Without its index, what the room's own measures say.
 */
function quality(s: Partial<VentilationRoomState>): Quality | null {
  if (typeof s.air_quality_index === "number")
    return s.air_quality_index > 100
      ? "bad"
      : s.air_quality_index > 40
        ? "warn"
        : "ok"
  const levels: Quality[] = []
  if (typeof s.humidity === "number")
    levels.push(s.humidity > 75 ? "bad" : s.humidity > 60 ? "warn" : "ok")
  if (typeof s.co2 === "number")
    levels.push(s.co2 > 1200 ? "bad" : s.co2 > 800 ? "warn" : "ok")
  if (levels.length === 0) return null
  return levels.includes("bad")
    ? "bad"
    : levels.includes("warn")
      ? "warn"
      : "ok"
}

const QUALITY: Record<
  Quality,
  { label: string; text: string; bg: string; tile: string }
> = {
  ok: {
    label: t("Air bon"),
    text: "text-success",
    bg: "bg-success",
    tile: "bg-success/15 text-success",
  },
  warn: {
    label: t("Air moyen"),
    text: "text-warning",
    bg: "bg-warning",
    tile: "bg-warning/15 text-warning",
  },
  bad: {
    label: t("Air mauvais"),
    text: "text-destructive",
    bg: "bg-destructive",
    tile: "bg-destructive/15 text-destructive",
  },
}

const minutesLeft = (s: Partial<VentilationRoomState>) =>
  typeof s.boost_remaining === "number"
    ? Math.max(1, Math.round(s.boost_remaining / 60))
    : null

type Command = {
  id: number
  trait: "ventilation.room" | "ventilation.unit"
  action: "boost" | "stop_boost" | "set_profile"
  params?: Record<string, unknown>
}

function useCommand() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (c: Command) =>
      api.act(c.id, c.trait, c.action, c.params ?? {}),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["devices"] }),
    onError: (e) => toast.error(errorText(e)),
  })
}

function useHistory(id: number, period: HistoryPeriod = "24h") {
  return useQuery({
    queryKey: ["device-history", id, period],
    queryFn: () =>
      api.deviceHistoryOver(id, PERIOD_HOURS[period], bucketFor(period)),
    staleTime: 60_000,
  })
}

function Bar({ value, className }: { value: number; className?: string }) {
  return (
    <div className="h-2 overflow-hidden rounded-full bg-muted">
      <div
        className={cn(
          "h-full rounded-full bg-air transition-[width]",
          className
        )}
        style={{ width: `${Math.min(100, Math.max(0, value))}%` }}
      />
    </div>
  )
}

function Measures({ s }: { s: Partial<VentilationRoomState> }) {
  const rows = [
    typeof s.humidity === "number" && [
      t("Humidité"),
      `${Math.round(s.humidity)} %`,
    ],
    typeof s.co2 === "number" && [t("CO₂"), `${Math.round(s.co2)} ppm`],
    typeof s.voc === "number" && [
      t("COV"),
      `${number(s.voc, s.voc < 10 ? 1 : 0)} ppm`,
    ],
  ].filter(Boolean) as [string, string][]
  if (rows.length === 0) return null
  return (
    <div className="grid gap-1 text-sm">
      {rows.map(([k, v]) => (
        <div key={k} className="flex justify-between">
          <span className="text-muted-foreground">{k}</span>
          <b className="tabular font-medium">{v}</b>
        </div>
      ))}
    </div>
  )
}

export function RoomCard({
  device,
  demanding,
  selected,
  onSelect,
  canDrive,
}: {
  device: DeviceView
  demanding: boolean
  selected: boolean
  onSelect: () => void
  canDrive: boolean
}) {
  const command = useCommand()
  const s = roomState(device)
  const q = quality(s)
  const left = minutesLeft(s)
  const flow = useHistory(device.id).data?.series.flow ?? []
  return (
    <Card
      className={cn(
        "cursor-pointer gap-3 rounded-3xl py-4 transition-shadow",
        selected && "ring-2 ring-primary"
      )}
      onClick={onSelect}
    >
      <CardContent className="grid gap-3 px-4">
        <div className="flex items-start justify-between gap-2">
          <div className="flex min-w-0 items-center gap-2">
            <span
              className={cn(
                "grid size-8 shrink-0 place-items-center rounded-xl bg-air/15 text-air",
                q && QUALITY[q].tile
              )}
            >
              <Fan className="size-4" />
            </span>
            <b className="truncate">{device.name}</b>
          </div>
          {q ? (
            <Badge variant="outline" className={QUALITY[q].text}>
              <span className={cn("size-1.5 rounded-full", QUALITY[q].bg)} />
              {QUALITY[q].label}
            </Badge>
          ) : (
            demanding && (
              <Badge variant="outline" className="text-warning">
                {t("Demande de l'air")}
              </Badge>
            )
          )}
        </div>
        <div className="grid gap-1.5">
          <div className="flex justify-between text-sm">
            <span className="text-muted-foreground">{t("Débit d'air")}</span>
            <b className="tabular font-medium">
              {typeof s.level === "number" ? `${s.level} %` : "–"}
              <span className="ml-1.5 text-xs font-normal text-muted-foreground">
                {typeof s.flow === "number" &&
                  t("{n} m³/h", { n: Math.round(s.flow) })}
              </span>
            </b>
          </div>
          <Bar value={s.level ?? 0} />
        </div>
        <Measures s={s} />
        <Sparkline
          points={flow}
          format={fieldFormat("flow")}
          className="h-10 text-air"
          label={t("Débit des dernières 24 h")}
        />
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs text-muted-foreground">
            {s.profile ? t("Profil {p}", { p: PROFILE[s.profile] }) : ""}
          </span>
          {s.boost ? (
            <Button
              variant="secondary"
              size="sm"
              disabled={!canDrive || command.isPending}
              onClick={(e) => {
                e.stopPropagation()
                command.mutate({
                  id: device.id,
                  trait: "ventilation.room",
                  action: "stop_boost",
                })
              }}
            >
              <Square className="fill-current" />
              {left ? t("Boost · {n} min", { n: left }) : t("Boost")}
            </Button>
          ) : (
            <Button
              variant="ghost"
              size="sm"
              disabled={!canDrive || !device.available || command.isPending}
              onClick={(e) => {
                e.stopPropagation()
                command.mutate({
                  id: device.id,
                  trait: "ventilation.room",
                  action: "boost",
                  params: { minutes: 30 },
                })
              }}
            >
              {command.isPending ? (
                <Loader2 className="animate-spin" />
              ) : (
                <Fan />
              )}
              {t("Boost 30 min")}
            </Button>
          )}
        </div>
      </CardContent>
    </Card>
  )
}

function Detail({
  device,
  demanding,
  cause,
  rooms,
  canDrive,
  admin,
}: {
  device: DeviceView
  demanding: boolean
  cause: string | null
  rooms: Room[]
  canDrive: boolean
  admin: boolean
}) {
  const command = useCommand()
  const s = roomState(device)
  const q = quality(s)
  const left = minutesLeft(s)
  const [period, setPeriod] = useState<HistoryPeriod>("24h")
  const read = useHistory(device.id, period)
  const history = read.data?.series
  const periods = periodsFor(read.data?.oldest, read.dataUpdatedAt / 1000)
  const curve = t("Courbe sur {period}", { period: PERIOD_LABEL[period] })
  const room = rooms.find((r) => r.id === device.room_id)
  const disabled = !canDrive || !device.available || command.isPending
  const sentence = s.boost
    ? t("Boost en cours, encore {n} min.", { n: left ?? 0 })
    : q === "bad"
      ? t("La qualité de l'air est mauvaise. La ventilation est active.")
      : demanding
        ? t("C'est la pièce qui demande le plus d'air, à cause de {cause}.", {
            cause: cause ?? t("la qualité de l'air"),
          })
        : (s.level ?? 0) > 50
          ? t("La ventilation est active.")
          : t("La ventilation est au repos.")
  const boost = (minutes: number) =>
    command.mutate({
      id: device.id,
      trait: "ventilation.room",
      action: "boost",
      params: { minutes },
    })
  return (
    <Card className="rounded-3xl lg:sticky lg:top-4">
      <CardHeader>
        <CardTitle className="truncate">{device.name}</CardTitle>
        <CardDescription>
          {room
            ? t("Pièce Breezone : {name}", { name: room.name })
            : t("Sans pièce")}
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4">
        <p className="text-lg leading-snug font-semibold">{sentence}</p>
        <PeriodPicker value={period} periods={periods} onChange={setPeriod} />
        <div className="grid gap-1.5 rounded-2xl bg-muted/50 p-3">
          <div className="flex justify-between text-sm">
            <span className="flex items-center gap-1.5 font-medium">
              <Fan className="size-4 text-air" /> {t("Débit d'air")}
            </span>
            <b className="tabular">
              {typeof s.level === "number" ? `${s.level} %` : "–"}
            </b>
          </div>
          <span className="text-xs text-muted-foreground">
            {typeof s.flow === "number" && typeof s.nominal === "number"
              ? t("{flow} m³/h sur {nominal} m³/h nominaux", {
                  flow: Math.round(s.flow),
                  nominal: Math.round(s.nominal),
                })
              : t("Niveau de ventilation")}
          </span>
          <Sparkline
            points={history?.flow ?? []}
            format={fieldFormat("flow")}
            className="h-16 text-air"
            label={curve}
          />
        </div>
        {typeof s.humidity === "number" && (
          <div className="grid gap-1.5 rounded-2xl bg-muted/50 p-3">
            <div className="flex justify-between text-sm">
              <span className="flex items-center gap-1.5 font-medium">
                <Droplets className="size-4 text-cool" /> {t("Humidité")}
              </span>
              <b className="tabular">{Math.round(s.humidity)} %</b>
            </div>
            <Sparkline
              points={history?.humidity ?? []}
              format={fieldFormat("humidity")}
              className="h-16 text-cool"
              label={curve}
            />
          </div>
        )}
        <Measures s={{ co2: s.co2, voc: s.voc }} />
        {s.boost ? (
          <Button
            variant="secondary"
            disabled={disabled}
            onClick={() =>
              command.mutate({
                id: device.id,
                trait: "ventilation.room",
                action: "stop_boost",
              })
            }
          >
            <Square className="fill-current" /> {t("Arrêter le boost")}
          </Button>
        ) : (
          <div className="flex flex-wrap gap-2">
            <Button disabled={disabled} onClick={() => boost(30)}>
              {command.isPending ? (
                <Loader2 className="animate-spin" />
              ) : (
                <Fan />
              )}
              {t("Boost 30 min")}
            </Button>
            <Button
              variant="outline"
              disabled={disabled}
              onClick={() => boost(60)}
            >
              {t("1 h")}
            </Button>
            <Button
              variant="outline"
              disabled={disabled}
              onClick={() => boost(120)}
            >
              {t("2 h")}
            </Button>
          </div>
        )}
        <div className="grid gap-1.5">
          <span className="text-xs text-muted-foreground">{t("Profil")}</span>
          <div className="flex gap-1 rounded-2xl bg-muted/60 p-1" role="group">
            {(["eco", "health", "intense"] as const).map((p) => (
              <Button
                key={p}
                size="sm"
                variant={s.profile === p ? "default" : "ghost"}
                aria-pressed={s.profile === p}
                className="flex-1"
                disabled={disabled}
                onClick={() =>
                  s.profile !== p &&
                  command.mutate({
                    id: device.id,
                    trait: "ventilation.room",
                    action: "set_profile",
                    params: { profile: p },
                  })
                }
              >
                {PROFILE[p]}
              </Button>
            ))}
          </div>
        </div>
        {admin && (
          <div className="grid gap-1.5">
            <span className="text-xs text-muted-foreground">{t("Pièce")}</span>
            <RoomSelect
              deviceId={device.id}
              roomId={device.room_id}
              rooms={rooms}
            />
          </div>
        )}
      </CardContent>
    </Card>
  )
}

export function House({
  unit,
  rooms,
  canDrive,
}: {
  unit: DeviceView
  rooms: DeviceView[]
  canDrive: boolean
}) {
  const command = useCommand()
  const s = unitState(unit)
  const flow = useHistory(unit.id).data?.series.flow ?? []
  const nominal = rooms.reduce((n, d) => n + (roomState(d).nominal ?? 0), 0)
  const valves = rooms.reduce((n, d) => n + (roomState(d).flow ?? 0), 0)
  const level = nominal > 0 ? Math.round((valves / nominal) * 100) : null
  const demanding = s.worst_room
    ? t("{room} demande le plus d'air, à cause de {cause}", {
        room: s.worst_room,
        cause: CAUSE[s.main_pollutant ?? "other"],
      })
    : null
  return (
    <Card className="rounded-3xl bg-[radial-gradient(80%_90%_at_0%_0%,color-mix(in_oklch,var(--air)_16%,transparent),transparent_70%)]">
      <CardContent className="grid gap-4">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="text-sm text-muted-foreground">
              {t("Niveau de ventilation")}
            </div>
            <div className="tabular font-heading text-4xl font-bold tracking-tight">
              {level !== null ? `${level} %` : "–"}
            </div>
            <div className="text-sm text-muted-foreground">
              {[
                typeof s.flow === "number" &&
                  t("{n} m³/h", { n: Math.round(s.flow) }),
                typeof s.power === "number" && `${Math.round(s.power)} W`,
                s.boost ? t("boost en cours") : t("mode automatique"),
              ]
                .filter(Boolean)
                .join(" · ")}
            </div>
            {demanding && <div className="mt-1 text-sm">{demanding}</div>}
            {s.sensors === false && (
              <p className="mt-2 max-w-md text-xs text-muted-foreground">
                {t(
                  "Sans la clé API Renson, la Healthbox ne donne que les débits : l'humidité, le CO₂ et la température de chaque pièce viennent avec la clé."
                )}{" "}
                <Link
                  to="/settings?tab=modules"
                  className="text-primary underline-offset-4 hover:underline"
                >
                  {t("Ajouter la clé")}
                </Link>
              </p>
            )}
          </div>
          {s.boost ? (
            <Button
              variant="secondary"
              disabled={!canDrive || command.isPending}
              onClick={() =>
                command.mutate({
                  id: unit.id,
                  trait: "ventilation.unit",
                  action: "stop_boost",
                })
              }
            >
              <Square className="fill-current" /> {t("Arrêter les boosts")}
            </Button>
          ) : (
            <Button
              variant="outline"
              disabled={!canDrive || !unit.available || command.isPending}
              onClick={() =>
                command.mutate({
                  id: unit.id,
                  trait: "ventilation.unit",
                  action: "boost",
                  params: { minutes: 60 },
                })
              }
            >
              {command.isPending ? (
                <Loader2 className="animate-spin" />
              ) : (
                <Fan />
              )}
              {t("Boost maison 1 h")}
            </Button>
          )}
        </div>
        <Sparkline
          points={flow}
          format={fieldFormat("flow")}
          className="h-20 text-air"
          label={t("Débit des dernières 24 h")}
        />
      </CardContent>
    </Card>
  )
}

/** The valves of the ventilation unit, each with its air and its controls. */
export function VentilationValvesBlock() {
  const { devices } = useOverview()
  const { selected, select } = useSelection()
  const unit = devices.find(isUnit)
  const valves = devices.filter(
    (d) => isRoom(d) && (!unit || d.source_id === unit.source_id)
  )
  if (valves.length === 0) return null
  const worst = unit ? unitState(unit).worst_room : undefined
  return (
    <div className="grid gap-3 sm:grid-cols-2">
      {valves.map((d) => (
        <RoomCard
          key={d.id}
          device={d}
          demanding={!!worst && worst === d.name}
          selected={selected === d.id}
          onSelect={() => select(d.id)}
          canDrive={d.can_control}
        />
      ))}
    </div>
  )
}

export function VentilationPage() {
  const admin = currentUser()?.role === "admin"
  const home = useHomeData()
  const [selected, setSelected] = useState<number | null>(null)
  const all = home.page.devices
  const unit = all.find(isUnit)
  const valves = all.filter(
    (d) => isRoom(d) && (!unit || d.source_id === unit.source_id)
  )
  const worst = unit ? unitState(unit).worst_room : undefined
  const cause = unit ? CAUSE[unitState(unit).main_pollutant ?? "other"] : null
  const shown = valves.find((d) => d.id === selected) ?? valves[0]
  const boosting = valves.filter((d) => roomState(d).boost)
  const generated = useMemo(() => ventilationLayout(), [])

  if (home.loading)
    return (
      <>
        <PageHeader title={t("Ventilation")} />
        <Skeleton className="h-72 w-full rounded-3xl" />
      </>
    )
  if (!unit)
    return (
      <>
        <PageHeader title={t("Ventilation")} />
        <Card className="rounded-3xl">
          <CardContent className="flex flex-col items-center gap-3 py-12 text-center">
            <span className="grid size-12 place-items-center rounded-2xl bg-air/15 text-air">
              <Fan className="size-6" />
            </span>
            <div className="font-heading text-lg font-semibold">
              {t("Aucune VMC")}
            </div>
            <p className="max-w-md text-sm text-muted-foreground">
              {t(
                "Ajoutez votre Renson Healthbox dans Réglages › Modules : Breezone la trouve sur le réseau."
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
      </>
    )
  return (
    <SelectionContext.Provider
      value={{ selected: shown?.id ?? null, select: setSelected }}
    >
      <Board
        page="ventilation"
        title={t("Ventilation")}
        description={[
          tn(valves.length, "{n} bouche", "{n} bouches"),
          boosting.length > 0
            ? tn(boosting.length, "{n} en boost", "{n} en boost")
            : null,
          t("API locale"),
        ]}
        data={home.page}
        generated={generated}
        aside={
          shown && (
            <Detail
              key={shown.id}
              device={shown}
              demanding={!!worst && worst === shown.name}
              cause={cause}
              rooms={home.page.rooms}
              canDrive={shown.can_control}
              admin={admin}
            />
          )
        }
      />
    </SelectionContext.Provider>
  )
}
