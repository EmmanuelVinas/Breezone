import { useState } from "react"
import { Link } from "react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { toast } from "sonner"
import { Lightbulb, Loader2, Power, Sparkles } from "lucide-react"
import type { DeviceView, Room, SceneView } from "@/lib/api"
import { api, currentUser } from "@/lib/api"
import { t, tn } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import { PageHeader } from "@/components/app-shell"
import { Level } from "@/components/level"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { Slider } from "@/components/ui/slider"
import { Switch } from "@/components/ui/switch"

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e))

/** The state of a light, as `light@1` describes it. */
type LightState = {
  on?: boolean
  brightness?: number
  color_temp?: number
  color_temp_min?: number
  color_temp_max?: number
  color?: string
  mode?: "white" | "color"
  reachable?: boolean
}

const stateOf = (d: DeviceView) => (d.state ?? {}) as LightState

/** The color of a white of `kelvin` (approximation of Tanner Helland), for the display only. */
function kelvinColor(kelvin: number) {
  const k = kelvin / 100
  const clamp = (v: number) => Math.round(Math.min(255, Math.max(0, v)))
  const r = k <= 66 ? 255 : 329.698727446 * Math.pow(k - 60, -0.1332047592)
  const g =
    k <= 66
      ? 99.4708025861 * Math.log(k) - 161.1195681661
      : 288.1221695283 * Math.pow(k - 60, -0.0755148492)
  const b =
    k >= 66
      ? 255
      : k <= 19
        ? 0
        : 138.5177312231 * Math.log(k - 10) - 305.0447927307
  return `rgb(${clamp(r)}, ${clamp(g)}, ${clamp(b)})`
}

/** What a lit light looks like: its color, or its white. */
function glow(s: LightState) {
  if (s.mode === "color" && s.color) return s.color
  if (s.color_temp) return kelvinColor(s.color_temp)
  return "#ffd59a"
}

function describe(s: LightState) {
  if (s.reachable === false) return t("Ne répond pas")
  if (!s.on) return t("Éteinte")
  const parts = [
    typeof s.brightness === "number" ? `${s.brightness} %` : null,
    s.mode === "color"
      ? t("Couleur")
      : s.color_temp
        ? s.color_temp < 3300
          ? t("Blanc chaud")
          : s.color_temp < 5000
            ? t("Blanc neutre")
            : t("Blanc froid")
        : null,
  ]
  return parts.filter(Boolean).join(" · ") || t("Allumée")
}

/** Colors offered for the lights that have colors. */
const SWATCHES = [
  "#ffb35c",
  "#ffd59a",
  "#fff3e0",
  "#e7f0ff",
  "#ff6b6b",
  "#ffa94d",
  "#69db7c",
  "#4dabf7",
  "#9775fa",
  "#f783ac",
]

function useSet() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({
      id,
      params,
    }: {
      id: number
      params: Record<string, unknown>
    }) => api.act(id, "light", "set", params),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["devices"] }),
    onError: (e) => toast.error(errorText(e)),
  })
}

function LightCard({
  light,
  selected,
  onSelect,
  canDrive,
}: {
  light: DeviceView
  selected: boolean
  onSelect: () => void
  canDrive: boolean
}) {
  const s = stateOf(light)
  const set = useSet()
  const [draft, setDraft] = useState<number | null>(null)
  const lit = !!s.on && s.reachable !== false && light.available
  const color = glow(s)
  return (
    <Card
      className={cn(
        "cursor-pointer gap-3 rounded-3xl py-4 transition-shadow",
        selected && "ring-2 ring-primary"
      )}
      style={
        lit
          ? {
              background: `radial-gradient(90% 80% at 0% 0%, color-mix(in oklch, ${color} 16%, transparent), transparent 70%), var(--card)`,
            }
          : undefined
      }
      onClick={onSelect}
    >
      <CardContent className="grid gap-3 px-4">
        <div className="flex items-center justify-between gap-2">
          <span className="flex min-w-0 items-center gap-2">
            <span
              className="grid size-8 shrink-0 place-items-center rounded-xl"
              style={{
                background: lit
                  ? `color-mix(in oklch, ${color} 28%, transparent)`
                  : "var(--muted)",
                color: lit ? color : "var(--muted-foreground)",
              }}
            >
              <Lightbulb className="size-4.5" />
            </span>
            <b className="truncate">{light.name}</b>
          </span>
          <Switch
            checked={!!s.on}
            disabled={!canDrive || !light.available}
            loading={set.isPending}
            onClick={(e) => e.stopPropagation()}
            onCheckedChange={(on) =>
              set.mutate({ id: light.id, params: { on } })
            }
            aria-label={light.name}
          />
        </div>
        <div className="text-xs text-muted-foreground">
          {light.available ? describe(s) : t("Indisponible")}
        </div>
        {lit && typeof s.brightness === "number" && (
          <div onClick={(e) => e.stopPropagation()}>
            <Slider
              min={1}
              max={100}
              step={1}
              value={[draft ?? s.brightness]}
              disabled={!canDrive || set.isPending}
              onValueChange={([v]) => setDraft(v)}
              onValueCommit={([v]) =>
                set.mutate(
                  { id: light.id, params: { brightness: v } },
                  { onSettled: () => setDraft(null) }
                )
              }
              aria-label={t("Intensité de {name}", { name: light.name })}
            />
          </div>
        )}
      </CardContent>
    </Card>
  )
}

/** The brand of a light's module, for the detail's subtitle. */
const BRAND: Record<string, string> = { hue: "Philips Hue" }

/** The detail of a light, as in the mockups: big brightness bar, white gradient, colors. */
function Detail({
  light,
  room,
  canDrive,
}: {
  light: DeviceView
  room: string | null
  canDrive: boolean
}) {
  const s = stateOf(light)
  const set = useSet()
  const [brightness, setBrightness] = useState<number | null>(null)
  const [temp, setTemp] = useState<number | null>(null)
  const lit = !!s.on && light.available
  const color = glow(s)
  const min = s.color_temp_min ?? 2000
  const max = s.color_temp_max ?? 6500
  const disabled = !canDrive || !light.available || set.isPending
  const level = brightness ?? s.brightness ?? 0
  const kelvin = temp ?? s.color_temp ?? min
  const send = (params: Record<string, unknown>, done: () => void) =>
    set.mutate({ id: light.id, params }, { onSettled: done })
  return (
    <Card
      className="rounded-3xl lg:sticky lg:top-4"
      style={
        lit
          ? {
              background: `radial-gradient(90% 60% at 50% 0%, color-mix(in oklch, ${color} 16%, transparent), transparent 70%), var(--card)`,
            }
          : undefined
      }
    >
      <CardHeader className="flex flex-row items-start justify-between gap-3">
        <div className="min-w-0">
          <CardTitle className="truncate">{light.name}</CardTitle>
          <CardDescription>
            {[room, BRAND[light.source] ?? light.source]
              .filter(Boolean)
              .join(" · ")}
          </CardDescription>
        </div>
        <Switch
          checked={!!s.on}
          disabled={!canDrive || !light.available}
          loading={set.isPending}
          onCheckedChange={(on) => set.mutate({ id: light.id, params: { on } })}
          aria-label={light.name}
        />
      </CardHeader>
      <CardContent className="grid gap-6">
        {typeof s.brightness === "number" && (
          <div className="flex justify-center py-2">
            <Level
              vertical
              min={1}
              max={100}
              value={level}
              disabled={disabled}
              label={t("Intensité de {name}", { name: light.name })}
              valueText={`${level} %`}
              onChange={setBrightness}
              onCommit={(v) =>
                send({ brightness: v }, () => setBrightness(null))
              }
              className="h-64 w-30 overflow-hidden rounded-[36px] bg-muted"
            >
              <div
                className="absolute inset-x-0 bottom-0 transition-[height] duration-150"
                style={{
                  height: `${lit ? level : 0}%`,
                  background: `linear-gradient(180deg, color-mix(in oklch, ${color} 70%, white), ${color})`,
                }}
              />
              <div
                className={cn(
                  "tabular absolute inset-0 grid place-items-center font-heading text-3xl font-bold",
                  lit && level > 45 ? "text-neutral-800" : "text-foreground"
                )}
              >
                {lit ? `${level} %` : t("Éteinte")}
              </div>
            </Level>
          </div>
        )}
        {typeof s.color_temp === "number" && (
          <div className="grid gap-2">
            <span className="text-xs text-muted-foreground">
              {t("Température de blanc · {k} K", {
                k: kelvin.toLocaleString("fr-FR"),
              })}
            </span>
            <Level
              min={min}
              max={max}
              step={50}
              value={kelvin}
              disabled={disabled}
              label={t("Température de blanc")}
              valueText={`${kelvin} K`}
              onChange={setTemp}
              onCommit={(v) => send({ color_temp: v }, () => setTemp(null))}
              className="h-3.5 rounded-full"
              style={{
                background: `linear-gradient(90deg, ${kelvinColor(min)}, ${kelvinColor((min + max) / 2)}, ${kelvinColor(max)})`,
              }}
            >
              <span
                className="absolute top-1/2 size-5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-foreground shadow-[0_0_0_3px_oklch(0_0_0/30%)]"
                style={{ left: `${((kelvin - min) / (max - min)) * 100}%` }}
              />
            </Level>
            <div className="flex justify-between text-xs text-muted-foreground">
              <span>{t("Blanc chaud")}</span>
              <span>{t("Blanc froid")}</span>
            </div>
          </div>
        )}
        {s.color !== undefined && (
          <div className="grid gap-2">
            <span className="text-xs text-muted-foreground">
              {t("Couleurs")}
            </span>
            <div className="flex flex-wrap gap-2">
              {SWATCHES.map((c) => (
                <button
                  key={c}
                  type="button"
                  disabled={disabled}
                  aria-label={t("Couleur {c}", { c })}
                  aria-pressed={
                    s.mode === "color" && s.color?.toLowerCase() === c
                  }
                  onClick={() =>
                    set.mutate({ id: light.id, params: { color: c } })
                  }
                  className={cn(
                    "size-8 rounded-full transition-transform hover:scale-110 disabled:opacity-50",
                    s.mode === "color" &&
                      s.color?.toLowerCase() === c &&
                      "ring-3 ring-foreground"
                  )}
                  style={{ background: c }}
                />
              ))}
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  )
}

/** The scenes of the Hue app, one button each. */
function Scenes({
  scenes,
  canDrive,
}: {
  scenes: SceneView[]
  canDrive: boolean
}) {
  const qc = useQueryClient()
  const activate = useMutation({
    mutationFn: (s: SceneView) => api.activateScene(s.source_id, s.id),
    onSuccess: (_, s) => {
      qc.invalidateQueries({ queryKey: ["devices"] })
      toast.success(t("Scène « {name} » activée", { name: s.name }))
    },
    onError: (e) => toast.error(errorText(e)),
  })
  if (scenes.length === 0) return null
  return (
    <div className="flex flex-wrap gap-2">
      {scenes.map((s) => {
        const busy =
          activate.isPending &&
          activate.variables?.id === s.id &&
          activate.variables?.source_id === s.source_id
        return (
          <Button
            key={`${s.source_id}/${s.id}`}
            variant="outline"
            size="sm"
            className="rounded-full"
            disabled={!canDrive || activate.isPending}
            onClick={() => activate.mutate(s)}
          >
            {busy ? <Loader2 className="animate-spin" /> : <Sparkles />}
            {s.name}
          </Button>
        )
      })}
    </div>
  )
}

/** The Breezone room a scene belongs to: the one all its lights are in, if they share one. */
function sceneRoom(scene: SceneView, lights: DeviceView[]) {
  const rooms = new Set(
    scene.device_ids.map(
      (id) => lights.find((d) => d.id === id)?.room_id ?? null
    )
  )
  const [only] = [...rooms]
  return rooms.size === 1 ? only : null
}

export function LightsPage() {
  const admin = currentUser()?.role === "admin"
  const qc = useQueryClient()
  const devices = useQuery({
    queryKey: ["devices"],
    queryFn: api.devices,
    refetchInterval: 5_000,
  })
  const rooms = useQuery({ queryKey: ["rooms"], queryFn: api.rooms })
  const scenes = useQuery({
    queryKey: ["scenes"],
    queryFn: api.scenes,
    staleTime: 60_000,
  })
  const [selected, setSelected] = useState<number | null>(null)
  const all = (devices.data ?? []).filter((d) => d.traits.includes("light@1"))
  const roomOf = new Map(
    (scenes.data ?? []).map((s) => [s, sceneRoom(s, all)] as const)
  )
  const scenesIn = (room: number | null) =>
    (scenes.data ?? []).filter((s) => roomOf.get(s) === room)
  // Scenes over several rooms (or with no light known here): above the rooms.
  const wide = (scenes.data ?? []).filter(
    (s) =>
      roomOf.get(s) === null &&
      !all.some((d) => d.room_id === null && s.device_ids.includes(d.id))
  )
  const lit = all.filter((d) => stateOf(d).on && d.available)
  // Drives several lights at once ("Tout allumer").
  const many = useMutation({
    mutationFn: (input: { ids: number[]; on: boolean }) =>
      Promise.all(
        input.ids.map((id) => api.act(id, "light", "set", { on: input.on }))
      ),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["devices"] }),
    onError: (e) => toast.error(errorText(e)),
  })
  const groups: { room: Room | null; lights: DeviceView[] }[] = [
    ...(rooms.data ?? []).map((r) => ({
      room: r,
      lights: all.filter((d) => d.room_id === r.id),
    })),
    { room: null, lights: all.filter((d) => d.room_id === null) },
  ].filter((g) => g.lights.length > 0)
  const shown = all.find((d) => d.id === selected) ?? all[0]

  return (
    <>
      <PageHeader
        title={t("Lumières")}
        description={
          all.length
            ? t("{lit} allumée(s) sur {total}", {
                lit: lit.length,
                total: all.length,
              })
            : undefined
        }
        actions={
          all.length > 0 && (
            <Button
              variant="outline"
              size="sm"
              disabled={many.isPending || lit.length === 0}
              onClick={() =>
                many.mutate({ ids: lit.map((d) => d.id), on: false })
              }
            >
              <Power /> {t("Tout éteindre")}
            </Button>
          )
        }
      />
      {devices.isLoading ? (
        <Skeleton className="h-72 w-full rounded-3xl" />
      ) : all.length === 0 ? (
        <Card className="rounded-3xl">
          <CardContent className="flex flex-col items-center gap-3 py-12 text-center">
            <span className="grid size-12 place-items-center rounded-2xl bg-warning/15 text-warning">
              <Lightbulb className="size-6" />
            </span>
            <div className="font-heading text-lg font-semibold">
              {t("Aucune lumière")}
            </div>
            <p className="max-w-md text-sm text-muted-foreground">
              {t(
                "Ajoutez votre pont Philips Hue dans Réglages › Modules : Breezone le trouve sur le réseau."
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
        <div className="grid items-start gap-5 lg:grid-cols-[minmax(0,1fr)_340px]">
          <div className="grid gap-6">
            {wide.length > 0 && <Scenes scenes={wide} canDrive={admin} />}
            {groups.map((g) => {
              const on = g.lights.filter((d) => stateOf(d).on && d.available)
              return (
                <section key={g.room?.id ?? "none"} className="grid gap-3">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="flex items-baseline gap-2">
                      <h2 className="font-heading text-lg font-semibold">
                        {g.room?.name ?? t("Sans pièce")}
                      </h2>
                      <span className="text-sm text-muted-foreground">
                        {tn(on.length, "{n} allumée", "{n} allumées")} /{" "}
                        {g.lights.length}
                      </span>
                    </div>
                    <div className="flex gap-1">
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={!admin || many.isPending}
                        onClick={() =>
                          many.mutate({
                            ids: g.lights.map((d) => d.id),
                            on: true,
                          })
                        }
                      >
                        <Power /> {t("Tout allumer")}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={!admin || many.isPending}
                        onClick={() =>
                          many.mutate({
                            ids: g.lights.map((d) => d.id),
                            on: false,
                          })
                        }
                      >
                        {t("Tout éteindre")}
                      </Button>
                    </div>
                  </div>
                  <Scenes
                    scenes={scenesIn(g.room?.id ?? null).filter(
                      (s) => g.room !== null || !wide.includes(s)
                    )}
                    canDrive={admin}
                  />
                  <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
                    {g.lights.map((d) => (
                      <LightCard
                        key={d.id}
                        light={d}
                        selected={shown?.id === d.id}
                        onSelect={() => setSelected(d.id)}
                        canDrive={admin}
                      />
                    ))}
                  </div>
                </section>
              )
            })}
          </div>
          {shown && (
            <Detail
              key={shown.id}
              light={shown}
              room={
                rooms.data?.find((r) => r.id === shown.room_id)?.name ?? null
              }
              canDrive={admin}
            />
          )}
        </div>
      )}
    </>
  )
}
