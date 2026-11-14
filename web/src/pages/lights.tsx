import { useState } from "react"
import { Link } from "react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { toast } from "sonner"
import { Lightbulb, Power } from "lucide-react"
import type { DeviceView, Room } from "@/lib/api"
import { api, currentUser } from "@/lib/api"
import { t, tn } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import { PageHeader } from "@/components/app-shell"
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

function Detail({ light, canDrive }: { light: DeviceView; canDrive: boolean }) {
  const s = stateOf(light)
  const set = useSet()
  const [brightness, setBrightness] = useState<number | null>(null)
  const [temp, setTemp] = useState<number | null>(null)
  const lit = !!s.on && light.available
  const color = glow(s)
  const min = s.color_temp_min ?? 2000
  const max = s.color_temp_max ?? 6500
  const disabled = !canDrive || !light.available || set.isPending
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
          <CardDescription>{describe(s)}</CardDescription>
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
          <div className="grid gap-2">
            <div className="flex items-baseline justify-between">
              <span className="text-xs text-muted-foreground">
                {t("Intensité")}
              </span>
              <span className="tabular font-heading text-3xl font-semibold">
                {lit ? `${brightness ?? s.brightness} %` : "–"}
              </span>
            </div>
            <Slider
              min={1}
              max={100}
              step={1}
              value={[brightness ?? s.brightness]}
              disabled={disabled}
              onValueChange={([v]) => setBrightness(v)}
              onValueCommit={([v]) =>
                set.mutate(
                  { id: light.id, params: { brightness: v } },
                  { onSettled: () => setBrightness(null) }
                )
              }
              aria-label={t("Intensité")}
            />
          </div>
        )}
        {typeof s.color_temp === "number" && (
          <div className="grid gap-2">
            <div className="flex items-baseline justify-between text-xs text-muted-foreground">
              <span>{t("Température de blanc")}</span>
              <span className="tabular">{temp ?? s.color_temp} K</span>
            </div>
            <div
              className="rounded-full p-1"
              style={{
                background: `linear-gradient(90deg, ${kelvinColor(min)}, ${kelvinColor((min + max) / 2)}, ${kelvinColor(max)})`,
              }}
            >
              <Slider
                min={min}
                max={max}
                step={50}
                value={[temp ?? s.color_temp]}
                disabled={disabled}
                onValueChange={([v]) => setTemp(v)}
                onValueCommit={([v]) =>
                  set.mutate(
                    { id: light.id, params: { color_temp: v } },
                    { onSettled: () => setTemp(null) }
                  )
                }
                aria-label={t("Température de blanc")}
              />
            </div>
            <div className="flex justify-between text-xs text-muted-foreground">
              <span>{t("Chaud")}</span>
              <span>{t("Froid")}</span>
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
                  onClick={() =>
                    set.mutate({ id: light.id, params: { color: c } })
                  }
                  className={cn(
                    "size-8 rounded-full transition-transform hover:scale-110 disabled:opacity-50",
                    s.mode === "color" &&
                      s.color?.toLowerCase() === c &&
                      "ring-2 ring-foreground ring-offset-2 ring-offset-card"
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

export function LightsPage() {
  const admin = currentUser()?.role === "admin"
  const qc = useQueryClient()
  const devices = useQuery({
    queryKey: ["devices"],
    queryFn: api.devices,
    refetchInterval: 5_000,
  })
  const rooms = useQuery({ queryKey: ["rooms"], queryFn: api.rooms })
  const [selected, setSelected] = useState<number | null>(null)
  const all = (devices.data ?? []).filter((d) => d.traits.includes("light@1"))
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
          {shown && <Detail key={shown.id} light={shown} canDrive={admin} />}
        </div>
      )}
    </>
  )
}
