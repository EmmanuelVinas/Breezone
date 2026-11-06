import { useState } from "react"
import { Link, useParams } from "react-router"
import { useMutation, useQuery } from "@tanstack/react-query"
import {
  ArrowLeft,
  Droplet,
  Fan,
  Minus,
  Moon,
  Pencil,
  Plus,
  Power,
} from "lucide-react"
import { CartesianGrid, Line, LineChart, XAxis, YAxis } from "recharts"
import { toast } from "sonner"
import { CommandIndicator } from "@/components/command-indicator"
import type { Mode, Zone } from "@/lib/api"
import { api } from "@/lib/api"
import {
  MODE_ICON,
  MODE_LABEL,
  MODE_TONE,
  formatTemp,
  time,
} from "@/lib/format"
import { useZoneControl } from "@/hooks/use-zone-control"
import { useSetpointDraft } from "@/components/zone-card"
import { SetpointDial } from "@/components/setpoint-dial"
import { cn } from "@/lib/utils"
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
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { Skeleton } from "@/components/ui/skeleton"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"

const COLOR_VAR: Record<Mode, string> = {
  stop: "--muted-foreground",
  cooling: "--cool",
  heating: "--heat",
  fan: "--fan",
  dry: "--dry",
  aux_heating: "--heat",
  auto: "--primary",
}

const chartConfig = {
  room_temp: { label: "Température", color: "var(--heat)" },
  setpoint: { label: "Consigne", color: "var(--primary)" },
  humidity: { label: "Humidité", color: "var(--cool)" },
} satisfies ChartConfig

function History({ zone }: { zone: Zone }) {
  const [hours, setHours] = useState("24")
  const history = useQuery({
    queryKey: [
      "history",
      zone.installation_id,
      zone.system_id,
      zone.zone_id,
      hours,
    ],
    queryFn: () => api.history(zone, Number(hours)),
    refetchInterval: 60_000,
  })
  const data = (history.data ?? []).map((r) => ({
    ...r,
    setpoint: r.is_on ? r.setpoint : null,
  }))
  const tickFormat = (ts: number) =>
    Number(hours) <= 24
      ? time(ts)
      : new Date(ts * 1000).toLocaleDateString("fr-FR", {
          day: "2-digit",
          month: "2-digit",
        })
  return (
    <Card className="rounded-3xl">
      <CardHeader className="flex flex-row items-start justify-between gap-4">
        <div>
          <CardTitle>Historique</CardTitle>
          <CardDescription>
            Température, consigne (quand la zone est allumée) et humidité
          </CardDescription>
        </div>
        <ToggleGroup
          type="single"
          value={hours}
          onValueChange={(v) => v && setHours(v)}
          variant="outline"
          size="sm"
        >
          <ToggleGroupItem value="24">24 h</ToggleGroupItem>
          <ToggleGroupItem value="168">7 j</ToggleGroupItem>
          <ToggleGroupItem value="720">30 j</ToggleGroupItem>
        </ToggleGroup>
      </CardHeader>
      <CardContent>
        {history.isLoading ? (
          <Skeleton className="h-64 w-full" />
        ) : data.length < 2 ? (
          <div className="flex h-64 items-center justify-center text-sm text-muted-foreground">
            Pas encore assez de relevés : ils s'accumulent toutes les 5 minutes.
          </div>
        ) : (
          <ChartContainer config={chartConfig} className="h-64 w-full">
            <LineChart data={data} margin={{ left: 0, right: 8 }}>
              <CartesianGrid vertical={false} />
              <XAxis
                dataKey="ts"
                tickFormatter={tickFormat}
                tickLine={false}
                axisLine={false}
                minTickGap={40}
              />
              <YAxis
                yAxisId="t"
                tickLine={false}
                axisLine={false}
                width={36}
                domain={["dataMin - 1", "dataMax + 1"]}
                tickFormatter={(v: number) => `${Math.round(v)}°`}
              />
              <YAxis
                yAxisId="h"
                orientation="right"
                tickLine={false}
                axisLine={false}
                width={36}
                domain={[0, 100]}
                tickFormatter={(v: number) => `${v}%`}
              />
              <ChartTooltip
                content={
                  <ChartTooltipContent
                    labelFormatter={(_, p) =>
                      p?.[0]
                        ? new Date(
                            Number(p[0].payload.ts) * 1000
                          ).toLocaleString("fr-FR")
                        : ""
                    }
                  />
                }
              />
              <Line
                yAxisId="t"
                dataKey="room_temp"
                type="monotone"
                stroke="var(--color-room_temp)"
                strokeWidth={2.5}
                dot={false}
              />
              <Line
                yAxisId="t"
                dataKey="setpoint"
                type="stepAfter"
                stroke="var(--color-setpoint)"
                strokeWidth={2}
                strokeDasharray="5 4"
                dot={false}
                connectNulls={false}
              />
              <Line
                yAxisId="h"
                dataKey="humidity"
                type="monotone"
                stroke="var(--color-humidity)"
                strokeWidth={1.5}
                dot={false}
                opacity={0.7}
              />
            </LineChart>
          </ChartContainer>
        )}
      </CardContent>
    </Card>
  )
}

function EditZone({ zone }: { zone: Zone }) {
  const [open, setOpen] = useState(false)
  const [name, setName] = useState(zone.name)
  const [group, setGroup] = useState(zone.group ?? "")
  const [position, setPosition] = useState(String(zone.position))
  const save = useMutation({
    mutationFn: () =>
      api.zoneMeta(zone, {
        display_name:
          name.trim() && name.trim() !== zone.airzone_name ? name.trim() : null,
        group: group.trim() || null,
        position: Number(position) || 0,
      }),
    onSuccess: () => {
      toast.success("Zone mise à jour")
      setOpen(false)
    },
    onError: (e) => toast.error(e instanceof Error ? e.message : String(e)),
  })
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="outline" size="sm">
          <Pencil /> Personnaliser
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Personnaliser la zone</DialogTitle>
          <DialogDescription>
            Ces réglages sont propres à Breezone. Nom Airzone d'origine : «{" "}
            {zone.airzone_name ?? "—"} ».
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor="zn">Nom affiché</Label>
            <Input
              id="zn"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="zg">Groupe (étage, pièce…)</Label>
            <Input
              id="zg"
              value={group}
              placeholder="ex. Étage"
              onChange={(e) => setGroup(e.target.value)}
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="zp">Ordre dans le groupe</Label>
            <Input
              id="zp"
              type="number"
              value={position}
              onChange={(e) => setPosition(e.target.value)}
            />
          </div>
        </div>
        <DialogFooter>
          <Button onClick={() => save.mutate()} disabled={save.isPending}>
            Enregistrer
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function ZoneDetail({ zone }: { zone: Zone }) {
  const control = useZoneControl()
  const { value, setDraft, commit, nudge } = useSetpointDraft(zone)
  const mode: Mode = zone.on && zone.mode ? zone.mode : "stop"
  const tone = MODE_TONE[mode]
  const step = zone.step || 0.5

  return (
    <div className="grid gap-6 lg:grid-cols-[minmax(0,420px)_1fr]">
      <Card className="relative self-start overflow-hidden rounded-3xl">
        <div
          className={cn(
            "pointer-events-none absolute -top-24 left-1/2 size-72 -translate-x-1/2 rounded-full blur-3xl",
            tone.bg
          )}
        />
        <CardContent className="relative flex flex-col items-center gap-6 pt-6">
          <SetpointDial
            zone={zone}
            value={value}
            onChange={setDraft}
            onCommit={commit}
            colorVar={COLOR_VAR[mode]}
          />
          <div className="flex items-center gap-3">
            <Button
              size="icon"
              variant="outline"
              className="size-12 rounded-2xl"
              onClick={() => nudge(-step)}
              aria-label="Baisser"
            >
              <Minus />
            </Button>
            <Button
              size="lg"
              className={cn(
                "h-12 min-w-36 rounded-2xl",
                !zone.on && "bg-muted text-foreground hover:bg-muted/80"
              )}
              onClick={() => control.mutate({ zone, cmd: { on: !zone.on } })}
            >
              <Power /> {zone.on ? "Allumée" : "Éteinte"}
            </Button>
            <CommandIndicator zone={zone} />
            <Button
              size="icon"
              variant="outline"
              className="size-12 rounded-2xl"
              onClick={() => nudge(step)}
              aria-label="Monter"
            >
              <Plus />
            </Button>
          </div>
          <div className="grid w-full grid-cols-2 gap-3 text-sm">
            <div className="rounded-2xl bg-muted/50 p-3">
              <div className="text-muted-foreground">Température</div>
              <div className="tabular text-xl font-semibold">
                {formatTemp(zone.room_temp)}
              </div>
            </div>
            <div className="rounded-2xl bg-muted/50 p-3">
              <div className="inline-flex items-center gap-1 text-muted-foreground">
                <Droplet className="size-3.5" /> Humidité
              </div>
              <div className="tabular text-xl font-semibold">
                {zone.humidity !== null ? `${zone.humidity} %` : "–"}
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      <div className="flex flex-col gap-6">
        {zone.modes.length > 0 && (
          <Card className="rounded-3xl">
            <CardHeader>
              <CardTitle>Mode</CardTitle>
              <CardDescription>
                Le mode s'applique à toutes les zones du système (réglé sur la
                zone maître).
              </CardDescription>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-3 gap-2 sm:grid-cols-4">
                {zone.modes.map((m) => {
                  const Icon = MODE_ICON[m]
                  const active = zone.mode === m
                  return (
                    <button
                      key={m}
                      onClick={() =>
                        control.mutate(
                          { zone, cmd: { mode: m } },
                          {
                            onSuccess: () =>
                              toast.success(
                                `Mode ${MODE_LABEL[m].toLowerCase()} appliqué à tout le système`
                              ),
                          }
                        )
                      }
                      className={cn(
                        "flex flex-col items-center gap-2 rounded-2xl border p-3 text-xs font-medium transition-colors hover:bg-muted",
                        active &&
                          cn(
                            "ring-2",
                            MODE_TONE[m].ring,
                            MODE_TONE[m].bg,
                            MODE_TONE[m].text
                          )
                      )}
                    >
                      <Icon className="size-5" />
                      {MODE_LABEL[m]}
                    </button>
                  )
                })}
              </div>
            </CardContent>
          </Card>
        )}

        <div className="grid gap-6 sm:grid-cols-2">
          {zone.speeds.length > 0 && (
            <Card className="rounded-3xl">
              <CardHeader>
                <CardTitle className="inline-flex items-center gap-2">
                  <Fan className="size-4" /> Ventilation
                </CardTitle>
              </CardHeader>
              <CardContent>
                <ToggleGroup
                  type="single"
                  variant="outline"
                  value={zone.speed !== null ? String(zone.speed) : undefined}
                  onValueChange={(v) =>
                    v && control.mutate({ zone, cmd: { speed: Number(v) } })
                  }
                >
                  {zone.speeds.map((s) => (
                    <ToggleGroupItem key={s} value={String(s)}>
                      {s === 0 ? "Auto" : s}
                    </ToggleGroupItem>
                  ))}
                </ToggleGroup>
              </CardContent>
            </Card>
          )}
          {zone.sleep !== null && (
            <Card className="rounded-3xl">
              <CardHeader>
                <CardTitle className="inline-flex items-center gap-2">
                  <Moon className="size-4" /> Veille
                </CardTitle>
              </CardHeader>
              <CardContent>
                <ToggleGroup
                  type="single"
                  variant="outline"
                  value={String(zone.sleep)}
                  onValueChange={(v) =>
                    v && control.mutate({ zone, cmd: { sleep: Number(v) } })
                  }
                >
                  {[0, 30, 60, 90].map((s) => (
                    <ToggleGroupItem key={s} value={String(s)}>
                      {s === 0 ? "Non" : `${s} min`}
                    </ToggleGroupItem>
                  ))}
                </ToggleGroup>
              </CardContent>
            </Card>
          )}
        </div>

        {zone.errors.length > 0 && (
          <Card className="rounded-3xl border-destructive/40">
            <CardHeader>
              <CardTitle className="text-destructive">
                Alertes Airzone
              </CardTitle>
              <CardDescription>{zone.errors.join(" · ")}</CardDescription>
            </CardHeader>
          </Card>
        )}

        <History zone={zone} />
      </div>
    </div>
  )
}

export function ZonePage() {
  const { inst, sys, zone: zoneId } = useParams()
  const status = useQuery({ queryKey: ["status"], queryFn: api.status })
  const installation = status.data?.find((i) => i.id === Number(inst))
  const zone = installation?.zones.find(
    (z) => z.system_id === Number(sys) && z.zone_id === Number(zoneId)
  )

  return (
    <>
      <div className="mb-6 flex flex-wrap items-center justify-between gap-4">
        <div className="flex items-center gap-3">
          <Button asChild variant="ghost" size="icon" className="rounded-full">
            <Link
              to={installation ? `/installations/${installation.id}` : "/"}
              aria-label="Retour"
            >
              <ArrowLeft />
            </Link>
          </Button>
          <div>
            <h1 className="font-heading text-2xl font-semibold tracking-tight">
              {zone?.name ?? "Zone"}
            </h1>
            <p className="text-sm text-muted-foreground">
              {installation?.name}
              {zone?.group ? ` · ${zone.group}` : ""}
            </p>
          </div>
        </div>
        {zone && <EditZone key={`${zone.name}-${zone.group}`} zone={zone} />}
      </div>
      {status.isLoading ? (
        <Skeleton className="h-96 rounded-3xl" />
      ) : zone ? (
        <ZoneDetail zone={zone} />
      ) : (
        <p className="text-muted-foreground">Zone introuvable.</p>
      )}
    </>
  )
}
