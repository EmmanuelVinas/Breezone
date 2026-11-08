import { useEffect, useMemo } from "react"
import { Link, useNavigate, useParams } from "react-router"
import { useMutation, useQuery } from "@tanstack/react-query"
import {
  CloudOff,
  Home,
  MapPin,
  Power,
  PowerOff,
  RefreshCw,
  Search,
  Thermometer,
} from "lucide-react"
import { toast } from "sonner"
import type { CloudInstallation, Installation, Mode, Zone } from "@/lib/api"
import { api, currentUser } from "@/lib/api"
import {
  MODE_ICON,
  MODE_LABEL,
  MODE_TONE,
  formatTemp,
  groupZones,
  systemModePlacement,
  installationAddress,
} from "@/lib/format"
import { useGroupPower, useZoneControl } from "@/hooks/use-zone-control"
import { cn } from "@/lib/utils"
import { PageHeader } from "@/components/app-shell"
import { ConnectionBadge, ConnectionDot } from "@/components/connection-badge"
import { ZoneCard } from "@/components/zone-card"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"

const LAST_KEY = "breezone.last_installation"

/** A stable hue per installation, like the colored cards of the official app. */
function hue(seed: string) {
  let h = 0
  for (const c of seed) h = (h * 31 + c.charCodeAt(0)) % 360
  return h
}

function InstallationTile({
  inst,
  active,
}: {
  inst: Installation
  active: boolean
}) {
  const on = inst.zones.filter((z) => z.on).length
  const temps = inst.zones
    .map((z) => z.room_temp)
    .filter((t): t is number => t !== null)
  const avg = temps.length
    ? temps.reduce((a, b) => a + b, 0) / temps.length
    : null
  const h = hue(inst.name)
  return (
    <Link
      to={`/installations/${inst.id}`}
      className={cn(
        "relative flex w-64 shrink-0 snap-start flex-col justify-between overflow-hidden rounded-3xl p-4 text-white shadow-lg transition-all",
        active
          ? "ring-2 ring-primary ring-offset-2 ring-offset-background"
          : "opacity-80 hover:opacity-100"
      )}
      style={{
        background: `linear-gradient(135deg, oklch(0.55 0.16 ${h}), oklch(0.38 0.12 ${(h + 40) % 360}))`,
      }}
    >
      <Home className="absolute -right-4 -bottom-4 size-28 opacity-15" />
      <div className="flex items-center justify-between">
        <span className="inline-flex items-center gap-1.5 rounded-full bg-black/25 px-2.5 py-1 text-[11px] font-medium backdrop-blur">
          <ConnectionDot state={inst.connection.state} className="size-2" />
          {inst.connection.state === "online"
            ? "Connectée"
            : inst.connection.state === "offline"
              ? "Hors ligne"
              : "En attente"}
        </span>
        {avg !== null && (
          <span className="tabular inline-flex items-center gap-1 text-sm font-semibold">
            <Thermometer className="size-3.5" />
            {formatTemp(avg)}
          </span>
        )}
      </div>
      <div className="mt-8">
        <div className="truncate text-lg leading-tight font-semibold">
          {inst.name}
        </div>
        <div className="truncate text-xs opacity-85">
          {[inst.location, inst.node && `via ${inst.node.name}`]
            .filter(Boolean)
            .join(" · ") || installationAddress(inst)}
        </div>
        <div className="mt-1 text-xs opacity-85">
          {on}/{inst.zones.length} zone{inst.zones.length > 1 ? "s" : ""}{" "}
          allumée{on > 1 ? "s" : ""}
        </div>
      </div>
    </Link>
  )
}

function RemoteTile({ inst }: { inst: CloudInstallation }) {
  const admin = currentUser()?.role === "admin"
  return (
    <Dialog>
      <DialogTrigger asChild>
        <button className="relative flex w-64 shrink-0 snap-start flex-col justify-between overflow-hidden rounded-3xl border border-dashed bg-card/50 p-4 text-left">
          <span className="inline-flex w-fit items-center gap-1.5 rounded-full bg-muted px-2.5 py-1 text-[11px] font-medium text-muted-foreground">
            <CloudOff className="size-3" /> Non jointe
          </span>
          <div className="mt-8">
            <div className="truncate text-lg leading-tight font-semibold text-muted-foreground">
              {inst.name}
            </div>
            <div className="truncate text-xs text-muted-foreground">
              {inst.location ?? "—"}
            </div>
            <div className="mt-1 text-xs text-muted-foreground">
              {inst.zones} zones
            </div>
          </div>
        </button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{inst.name}</DialogTitle>
          <DialogDescription>
            Cette installation figure dans votre compte Airzone, mais ce serveur
            Breezone ne joint pas son WebServer. S'il est sur le même réseau,
            indiquez son adresse locale ; s'il est sur un autre site, il sera
            joint via le nœud Breezone installé là-bas (fonction à venir).
          </DialogDescription>
        </DialogHeader>
        {admin && (
          <Button asChild>
            <Link
              to={`/settings?tab=installations&access=${encodeURIComponent(inst.cloud_id)}`}
            >
              Configurer l'accès local
            </Link>
          </Button>
        )}
      </DialogContent>
    </Dialog>
  )
}

/** Mode selectors of the given systems (one master zone per system). */
function SystemModes({
  masters,
  labelled,
}: {
  masters: Zone[]
  /** Name each system ("Système 2"): the installation has several. */
  labelled: boolean
}) {
  const control = useZoneControl()
  if (!masters.length) return null
  return (
    <div className="flex flex-wrap gap-3">
      {masters.map((m) => (
        <div key={m.system_id} className="flex items-center gap-2">
          {labelled && (
            <span className="text-xs text-muted-foreground">
              Système {m.system_id}
            </span>
          )}
          <ToggleGroup
            type="single"
            value={m.mode ?? undefined}
            onValueChange={(v) =>
              v &&
              control.mutate(
                { zone: m, cmd: { mode: v as Mode } },
                {
                  onSuccess: () =>
                    toast.success(
                      `Mode ${MODE_LABEL[v as Mode].toLowerCase()} appliqué${labelled ? ` au système ${m.system_id}` : ""}`
                    ),
                }
              )
            }
            className="rounded-2xl border bg-card/60 p-1"
          >
            {m.modes.map((mode) => {
              const Icon = MODE_ICON[mode]
              return (
                <Tooltip key={mode}>
                  <TooltipTrigger asChild>
                    <ToggleGroupItem
                      value={mode}
                      aria-label={MODE_LABEL[mode]}
                      className={cn(
                        "rounded-xl",
                        m.mode === mode && MODE_TONE[mode].text
                      )}
                    >
                      <Icon />
                    </ToggleGroupItem>
                  </TooltipTrigger>
                  <TooltipContent>{MODE_LABEL[mode]}</TooltipContent>
                </Tooltip>
              )
            })}
          </ToggleGroup>
        </div>
      ))}
    </div>
  )
}

function ZoneGroup({
  name,
  zones,
  readOnly,
  masters,
  labelled,
}: {
  name: string | null
  zones: Zone[]
  readOnly: boolean
  /** Systems whose zones all belong to this group: their mode selector sits on its row. */
  masters: Zone[]
  labelled: boolean
}) {
  const power = useGroupPower()
  const on = zones.filter((z) => z.on).length
  return (
    <section className="mb-8">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-baseline gap-3">
          <h2 className="text-lg font-semibold">{name ?? "Zones"}</h2>
          <span className="text-sm text-muted-foreground">
            {on}/{zones.length} allumée{on > 1 ? "s" : ""}
          </span>
        </div>
        <div
          className={cn(
            "flex flex-wrap items-center gap-3",
            readOnly && "hidden"
          )}
        >
          <SystemModes masters={masters} labelled={labelled} />
          <div className="flex gap-1">
            <Button
              size="sm"
              variant="ghost"
              disabled={on === zones.length}
              onClick={() =>
                power.mutate({ zones, on: true, label: name ?? undefined })
              }
            >
              <Power /> Tout allumer
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={on === 0}
              onClick={() =>
                power.mutate({ zones, on: false, label: name ?? undefined })
              }
            >
              <PowerOff /> Tout éteindre
            </Button>
          </div>
        </div>
      </div>
      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
        {zones.map((z) => (
          <ZoneCard
            key={`${z.system_id}-${z.zone_id}`}
            zone={z}
            readOnly={readOnly}
          />
        ))}
      </div>
    </section>
  )
}

function EmptyState() {
  const admin = currentUser()?.role === "admin"
  return (
    <div className="flex flex-col items-center rounded-3xl border border-dashed bg-card/40 px-6 py-16 text-center">
      <Home className="size-10 text-muted-foreground" />
      <h2 className="mt-4 text-lg font-semibold">Aucune installation</h2>
      <p className="mt-1 max-w-md text-sm text-muted-foreground">
        {admin
          ? "Connectez votre compte Airzone pour tout importer automatiquement, ou recherchez les WebServers Airzone sur votre réseau."
          : "Un administrateur doit d'abord ajouter une installation."}
      </p>
      {admin && (
        <div className="mt-6 flex flex-wrap justify-center gap-2">
          <Button asChild>
            <Link to="/settings?tab=cloud">Importer depuis Airzone</Link>
          </Button>
          <Button asChild variant="outline">
            <Link to="/settings?tab=installations">
              <Search /> Rechercher sur le réseau
            </Link>
          </Button>
        </div>
      )}
    </div>
  )
}

export function DashboardPage() {
  const params = useParams()
  const navigate = useNavigate()
  const status = useQuery({ queryKey: ["status"], queryFn: api.status })
  const cloud = useQuery({ queryKey: ["cloud"], queryFn: api.cloud })
  const refresh = useMutation({
    mutationFn: api.refreshInstallation,
    onSuccess: (i) => toast.success(`${i.name} actualisée`),
    onError: (e) => toast.error(e instanceof Error ? e.message : String(e)),
  })

  const installations = useMemo(() => status.data ?? [], [status.data])
  const remote = (cloud.data?.installations ?? []).filter((c) => !c.reachable)
  const selectedId = Number(
    params.id ?? localStorage.getItem(LAST_KEY) ?? installations[0]?.id
  )
  const selected =
    installations.find((i) => i.id === selectedId) ?? installations[0]
  const groups = useMemo(() => groupZones(selected?.zones ?? []), [selected])
  const modes = useMemo(
    () => systemModePlacement(selected?.zones ?? []),
    [selected]
  )

  useEffect(() => {
    if (selected) localStorage.setItem(LAST_KEY, String(selected.id))
    if (
      params.id &&
      installations.length &&
      !installations.some((i) => i.id === Number(params.id))
    )
      navigate("/", { replace: true })
  }, [selected, params.id, installations, navigate])

  if (status.isLoading) {
    return (
      <div className="space-y-6">
        <Skeleton className="h-9 w-64" />
        <div className="flex gap-4">
          <Skeleton className="h-36 w-64 rounded-3xl" />
          <Skeleton className="h-36 w-64 rounded-3xl" />
        </div>
        <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
          {[0, 1, 2].map((i) => (
            <Skeleton key={i} className="h-44 rounded-3xl" />
          ))}
        </div>
      </div>
    )
  }

  return (
    <>
      <PageHeader title="Mes installations" />
      {installations.length === 0 && remote.length === 0 ? (
        <EmptyState />
      ) : (
        <>
          <div className="-mx-4 mb-6 flex snap-x [scrollbar-width:none] gap-4 overflow-x-auto px-4 pt-1.5 pb-2.5 md:-mx-1.5 md:px-1.5">
            {installations.map((i) => (
              <InstallationTile
                key={i.id}
                inst={i}
                active={i.id === selected?.id}
              />
            ))}
            {remote.map((c) => (
              <RemoteTile key={c.cloud_id} inst={c} />
            ))}
          </div>

          {selected && (
            <>
              <div className="mb-6 flex flex-wrap items-center justify-between gap-4">
                <div className="min-w-0">
                  <h2 className="truncate text-xl font-semibold">
                    {selected.name}
                  </h2>
                  <div className="mt-1 flex flex-wrap items-center gap-3 text-sm text-muted-foreground">
                    {selected.location && (
                      <span className="inline-flex items-center gap-1">
                        <MapPin className="size-3.5" /> {selected.location}
                      </span>
                    )}
                    <ConnectionBadge connection={selected.connection} />
                    {!selected.enabled && (
                      <Badge variant="secondary">Désactivée</Badge>
                    )}
                    <Button
                      variant="ghost"
                      size="icon"
                      className="size-7"
                      onClick={() => refresh.mutate(selected.id)}
                      aria-label="Actualiser"
                    >
                      <RefreshCw
                        className={cn(refresh.isPending && "animate-spin")}
                      />
                    </Button>
                  </div>
                </div>
                {selected.controllable && (
                  <SystemModes
                    masters={modes.header}
                    labelled={modes.systems > 1}
                  />
                )}
              </div>

              {selected.zones.length === 0 ? (
                <div className="rounded-3xl border border-dashed p-10 text-center text-sm text-muted-foreground">
                  {selected.connection.state === "offline"
                    ? selected.node
                      ? `Le serveur ${selected.node.name} ne transmet pas cette installation : ${selected.connection.last_error ?? ""}`
                      : `Le WebServer ${selected.host}:${selected.port} ne répond pas : ${selected.connection.last_error ?? ""}`
                    : "En attente des données du WebServer…"}
                </div>
              ) : (
                groups.map((g) => (
                  <ZoneGroup
                    key={g.name ?? "_"}
                    name={g.name}
                    zones={g.zones}
                    readOnly={!selected.controllable}
                    masters={modes.byGroup.get(g.name) ?? []}
                    labelled={modes.systems > 1}
                  />
                ))
              )}
            </>
          )}
        </>
      )}
    </>
  )
}
