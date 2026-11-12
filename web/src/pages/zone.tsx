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
import { toast } from "sonner"
import { CommandIndicator } from "@/components/command-indicator"
import type { Mode, Zone } from "@/lib/api"
import { api } from "@/lib/api"
import { MODE_ICON, MODE_LABEL, MODE_TONE, formatTemp } from "@/lib/format"
import { useZoneControl } from "@/hooks/use-zone-control"
import { useSetpointDraft } from "@/components/zone-card"
import { SetpointDial } from "@/components/setpoint-dial"
import { DailyComfort } from "@/components/daily-comfort"
import { AirQualityCard } from "@/components/air-quality"
import { ZoneHistory } from "@/components/zone-history"
import { cn } from "@/lib/utils"
import { ConfirmDialog } from "@/components/confirm"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
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
import { t } from "@/lib/i18n"

const COLOR_VAR: Record<Mode, string> = {
  stop: "--muted-foreground",
  cooling: "--cool",
  heating: "--heat",
  fan: "--fan",
  dry: "--dry",
  aux_heating: "--heat",
  auto: "--primary",
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
      toast.success(t("Zone mise à jour"))
      setOpen(false)
    },
    onError: (e) => toast.error(e instanceof Error ? e.message : String(e)),
  })
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="outline" size="sm">
          <Pencil /> {t("Personnaliser")}
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("Personnaliser la zone")}</DialogTitle>
          <DialogDescription>
            {t(
              "Ces réglages sont propres à Breezone. Nom Airzone d'origine : « {name} ».",
              {
                name: zone.airzone_name ?? "—",
              }
            )}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor="zn">{t("Nom affiché")}</Label>
            <Input
              id="zn"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="zg">{t("Groupe (étage, pièce…)")}</Label>
            <Input
              id="zg"
              value={group}
              placeholder={t("ex. Étage")}
              onChange={(e) => setGroup(e.target.value)}
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="zp">{t("Ordre dans le groupe")}</Label>
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
            {t("Enregistrer")}
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
  // A mode applies to the whole system: it is confirmed first.
  const [pendingMode, setPendingMode] = useState<Mode | null>(null)

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
              aria-label={t("Baisser")}
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
              <Power /> {zone.on ? t("Allumée") : t("Éteinte")}
            </Button>
            <CommandIndicator zone={zone} />
            <Button
              size="icon"
              variant="outline"
              className="size-12 rounded-2xl"
              onClick={() => nudge(step)}
              aria-label={t("Monter")}
            >
              <Plus />
            </Button>
          </div>
          <div className="grid w-full grid-cols-2 gap-3 text-sm">
            <div className="rounded-2xl bg-muted/50 p-3">
              <div className="text-muted-foreground">{t("Température")}</div>
              <div className="tabular text-xl font-semibold">
                {formatTemp(zone.room_temp)}
              </div>
            </div>
            <div className="rounded-2xl bg-muted/50 p-3">
              <div className="inline-flex items-center gap-1 text-muted-foreground">
                <Droplet className="size-3.5" /> {t("Humidité")}
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
              <CardTitle>{t("Mode")}</CardTitle>
              <CardDescription>
                {t(
                  "Le mode s'applique à toutes les zones du système (réglé sur la zone maître)."
                )}
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
                      onClick={() => !active && setPendingMode(m)}
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
              <ConfirmDialog
                open={pendingMode !== null}
                title={t("Passer en mode {mode} ?", {
                  mode: pendingMode
                    ? MODE_LABEL[pendingMode].toLowerCase()
                    : "",
                })}
                description={t(
                  "Le mode s'applique à toutes les zones du système."
                )}
                onCancel={() => setPendingMode(null)}
                onConfirm={() => {
                  const m = pendingMode
                  setPendingMode(null)
                  if (m)
                    control.mutate(
                      { zone, cmd: { mode: m } },
                      {
                        onSuccess: () =>
                          toast.success(
                            t("Mode {mode} appliqué à tout le système", {
                              mode: MODE_LABEL[m].toLowerCase(),
                            })
                          ),
                      }
                    )
                }}
              />
            </CardContent>
          </Card>
        )}

        <div className="grid gap-6 sm:grid-cols-2">
          {zone.speeds.length > 0 && (
            <Card className="rounded-3xl">
              <CardHeader>
                <CardTitle className="inline-flex items-center gap-2">
                  <Fan className="size-4" /> {t("Ventilation")}
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
                      {s === 0 ? t("Auto") : s}
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
                  <Moon className="size-4" /> {t("Veille")}
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
                      {s === 0 ? t("Non") : t("{n} min", { n: s })}
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
                {t("Alertes Airzone")}
              </CardTitle>
              <CardDescription>{zone.errors.join(" · ")}</CardDescription>
            </CardHeader>
          </Card>
        )}

        <ZoneHistory zone={zone} />
        <DailyComfort zone={zone} />
        {zone.air_quality && <AirQualityCard zone={zone} />}
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
              aria-label={t("Retour")}
            >
              <ArrowLeft />
            </Link>
          </Button>
          <div>
            <h1 className="font-heading text-2xl font-semibold tracking-tight">
              {zone?.name ?? t("Zone")}
            </h1>
            <p className="text-sm text-muted-foreground">
              {installation?.name}
              {zone?.group ? ` · ${zone.group}` : ""}
            </p>
          </div>
        </div>
        {zone && installation?.controllable && installation.rights.zones && (
          <EditZone key={`${zone.name}-${zone.group}`} zone={zone} />
        )}
      </div>
      {status.isLoading ? (
        <Skeleton className="h-96 rounded-3xl" />
      ) : zone ? (
        <>
          {!installation?.controllable && (
            <p className="mb-4 rounded-2xl border p-3 text-sm text-muted-foreground">
              {t(
                "Lecture seule : vous pouvez consulter cette installation, pas la piloter."
              )}
            </p>
          )}
          {/* A disabled fieldset disables every control of the zone at once. */}
          <fieldset
            disabled={!installation?.controllable}
            className={cn(
              "contents",
              !installation?.controllable &&
                "[&_.setpoint-dial]:pointer-events-none"
            )}
          >
            <ZoneDetail zone={zone} />
          </fieldset>
        </>
      ) : (
        <p className="text-muted-foreground">{t("Zone introuvable.")}</p>
      )}
    </>
  )
}
