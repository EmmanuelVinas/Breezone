import { useEffect, useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  Loader2,
  Plus,
  Puzzle,
  RefreshCw,
  Search,
  Trash2,
  Zap,
} from "lucide-react"
import { toast } from "sonner"
import type {
  DeviceView,
  FoundProduct,
  MeasureRole,
  Room,
  SourceView,
} from "@/lib/api"
import { api } from "@/lib/api"
import { relativeTime } from "@/lib/format"
import { t } from "@/lib/i18n"
import { Confirm } from "@/components/confirm"
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
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e))

const ROLE_LABEL: Record<MeasureRole | "none", string> = {
  none: t("Simple mesure"),
  grid: t("Réseau (général)"),
  solar: t("Production solaire"),
  battery: t("Batterie"),
  consumption: t("Consommation de…"),
}

const STATE_LABEL: Record<SourceView["status"]["state"], string> = {
  online: t("Connecté"),
  offline: t("Injoignable"),
  error: t("Erreur"),
  disabled: t("Désactivé"),
  unknown: t("En attente"),
}

function watts(v: unknown) {
  if (typeof v !== "number") return null
  return Math.abs(v) >= 1000
    ? `${(v / 1000).toLocaleString(undefined, { maximumFractionDigits: 2 })} kW`
    : `${Math.round(v)} W`
}

function kwh(v: unknown) {
  return typeof v === "number"
    ? `${v.toLocaleString(undefined, { maximumFractionDigits: 1 })} kWh`
    : null
}

/** The model, without repeating the source's name ("Shelly Pro 3EM (monophasé…)" → "monophasé…"). */
function modelDetail(source: SourceView) {
  const model = source.status.model
  if (!model || !model.startsWith(source.name)) return model
  const rest = model.slice(source.name.length).trim()
  return rest.replace(/^\((.*)\)$/, "$1") || null
}

function StateBadge({ status }: { status: SourceView["status"] }) {
  const tone =
    status.state === "online"
      ? "bg-success"
      : status.state === "offline" || status.state === "error"
        ? "bg-destructive"
        : "bg-muted-foreground"
  return (
    <Badge variant="outline" className="gap-1.5">
      <span className={`size-2 rounded-full ${tone}`} />
      {STATE_LABEL[status.state]}
    </Badge>
  )
}

/** One measure or relay of a source: its values, its name, and what it stands for. */
function DeviceRow({
  device,
  devices,
  rooms,
}: {
  device: DeviceView
  devices: DeviceView[]
  rooms: Room[]
}) {
  const qc = useQueryClient()
  // Draft while typing; otherwise the current name (which may change from elsewhere).
  const [draft, setDraft] = useState<string | null>(null)
  const name = draft ?? device.name
  const save = useMutation({
    mutationFn: (input: Parameters<typeof api.updateDevice>[1]) =>
      api.updateDevice(device.id, input),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["devices"] }),
    onError: (e) => toast.error(errorText(e)),
  })
  const measuresEnergy = device.traits.some((x) =>
    x.startsWith("energy.power@")
  )
  const target = device.measures.device_id
    ? `d:${device.measures.device_id}`
    : device.measures.room_id
      ? `r:${device.measures.room_id}`
      : "none"
  const s = device.state ?? {}
  const values = [
    watts(s.power),
    typeof s.on === "boolean" ? (s.on ? t("allumé") : t("éteint")) : null,
    kwh(s.energy_in) && t("{value} consommés", { value: kwh(s.energy_in)! }),
    typeof s.energy_out === "number" && s.energy_out > 0
      ? t("{value} injectés", { value: kwh(s.energy_out)! })
      : null,
  ].filter(Boolean)

  return (
    <div className="grid gap-3 rounded-2xl border p-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
      <div className="min-w-0 space-y-1">
        <Input
          value={name}
          aria-label={t("Nom")}
          className="h-8 max-w-72 font-medium"
          onChange={(e) => setDraft(e.target.value)}
          onBlur={() => {
            if (draft !== null && draft.trim() !== device.name)
              save.mutate({ name: draft.trim() })
            setDraft(null)
          }}
        />
        <div className="tabular text-xs text-muted-foreground">
          {device.available ? values.join(" · ") : t("Indisponible")}
        </div>
      </div>
      {measuresEnergy && (
        <div className="flex flex-wrap items-center gap-2">
          <Select
            value={device.role ?? "none"}
            onValueChange={(v) =>
              save.mutate({ role: v === "none" ? null : (v as MeasureRole) })
            }
          >
            <SelectTrigger className="w-48" aria-label={t("Rôle")}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(Object.keys(ROLE_LABEL) as (MeasureRole | "none")[]).map(
                (r) => (
                  <SelectItem key={r} value={r}>
                    {ROLE_LABEL[r]}
                  </SelectItem>
                )
              )}
            </SelectContent>
          </Select>
          {device.role === "consumption" && (
            <Select
              value={target}
              onValueChange={(v) =>
                save.mutate({
                  measures_device_id: v.startsWith("d:")
                    ? Number(v.slice(2))
                    : null,
                  measures_room_id: v.startsWith("r:")
                    ? Number(v.slice(2))
                    : null,
                })
              }
            >
              <SelectTrigger className="w-56" aria-label={t("Mesure")}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="none">{t("Autres usages")}</SelectItem>
                {devices
                  .filter((d) => d.id !== device.id && d.role === null)
                  .map((d) => (
                    <SelectItem key={d.id} value={`d:${d.id}`}>
                      {d.name}
                    </SelectItem>
                  ))}
                {rooms.map((r) => (
                  <SelectItem key={`r${r.id}`} value={`r:${r.id}`}>
                    {t("Pièce : {name}", { name: r.name })}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
        </div>
      )}
    </div>
  )
}

function SourceCard({
  source,
  devices,
  all,
  rooms,
}: {
  source: SourceView
  devices: DeviceView[]
  all: DeviceView[]
  rooms: Room[]
}) {
  const qc = useQueryClient()
  const refresh = useMutation({
    mutationFn: () => api.refreshSource(source.id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["sources"] })
      qc.invalidateQueries({ queryKey: ["devices"] })
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const remove = useMutation({
    mutationFn: () => api.deleteSource(source.id),
    onSuccess: () => {
      toast.success(t("« {name} » retiré", { name: source.name }))
      qc.invalidateQueries({ queryKey: ["sources"] })
      qc.invalidateQueries({ queryKey: ["devices"] })
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const host = typeof source.config.host === "string" ? source.config.host : ""
  return (
    <Card className="rounded-3xl">
      <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-3">
        <div className="flex items-center gap-3">
          <span className="grid size-10 place-items-center rounded-2xl bg-warning/15 text-warning">
            <Zap className="size-5" />
          </span>
          <div>
            <CardTitle className="flex flex-wrap items-center gap-2">
              {source.name} <StateBadge status={source.status} />
            </CardTitle>
            <CardDescription>
              {[
                modelDetail(source),
                host,
                source.status.firmware &&
                  t("firmware {v}", { v: source.status.firmware }),
              ]
                .filter(Boolean)
                .join(" · ")}
            </CardDescription>
            {source.status.state !== "online" && source.status.last_error && (
              <p className="mt-1 text-xs text-destructive">
                {source.status.last_error}
                {source.status.last_seen &&
                  ` · ${t("vu {when}", { when: relativeTime(source.status.last_seen) })}`}
              </p>
            )}
          </div>
        </div>
        <div className="flex gap-1">
          <Button
            variant="ghost"
            size="icon"
            aria-label={t("Actualiser")}
            onClick={() => refresh.mutate()}
            disabled={refresh.isPending}
          >
            <RefreshCw className={refresh.isPending ? "animate-spin" : ""} />
          </Button>
          <Confirm
            title={t("Retirer « {name} » ?", { name: source.name })}
            description={t(
              "Ses appareils disparaissent de Breezone, avec leurs réglages. Le matériel n'est pas modifié."
            )}
            onConfirm={() => remove.mutate()}
          >
            <Button variant="ghost" size="icon" aria-label={t("Retirer")}>
              <Trash2 />
            </Button>
          </Confirm>
        </div>
      </CardHeader>
      <CardContent className="grid gap-2">
        {devices.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            {t("Aucun appareil lu pour l'instant.")}
          </p>
        ) : (
          devices.map((d) => (
            <DeviceRow key={d.id} device={d} devices={all} rooms={rooms} />
          ))
        )}
      </CardContent>
    </Card>
  )
}

/** Finds the supported products on the network; a Shelly is added in one click. */
function AddModuleDialog({
  open,
  onClose,
}: {
  open: boolean
  onClose: () => void
}) {
  const qc = useQueryClient()
  const [subnet, setSubnet] = useState("")
  const [host, setHost] = useState("")
  const scan = useMutation({
    mutationFn: () => api.scanProducts(subnet.trim() || undefined),
    onError: (e) => toast.error(errorText(e)),
  })
  useEffect(() => {
    if (open && !scan.data && !scan.isPending) scan.mutate()
    // Once per opening.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])
  const add = useMutation({
    mutationFn: (p: {
      name: string
      host: string
      device_id?: string | null
    }) =>
      api.createSource({
        driver: "shelly",
        name: p.name,
        config: p.device_id
          ? { host: p.host, device_id: p.device_id }
          : { host: p.host },
      }),
    onSuccess: (s) => {
      toast.success(
        s.status.state === "online"
          ? t("« {name} » ajouté", { name: s.name })
          : t("« {name} » ajouté, mais ne répond pas pour l'instant", {
              name: s.name,
            })
      )
      qc.invalidateQueries({ queryKey: ["sources"] })
      qc.invalidateQueries({ queryKey: ["devices"] })
      onClose()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const products = scan.data?.products ?? []

  const action = (p: FoundProduct) => {
    if (p.added) return <Badge variant="secondary">{t("Déjà ajouté")}</Badge>
    if (p.brand === "airzone")
      return (
        <span className="text-xs text-muted-foreground">
          {t("Onglet Installations")}
        </span>
      )
    if (!p.supported)
      return <Badge variant="outline">{t("Non pris en charge")}</Badge>
    return (
      <Button
        size="sm"
        disabled={add.isPending}
        onClick={() =>
          add.mutate({ name: p.model, host: p.host, device_id: p.device_id })
        }
      >
        <Plus /> {t("Ajouter")}
      </Button>
    )
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{t("Ajouter un module")}</DialogTitle>
          <DialogDescription>
            {t(
              "Breezone cherche sur le réseau les produits qu'il sait piloter : WebServers Airzone et Shelly (Plus, Pro, Gen3, Gen4)."
            )}
          </DialogDescription>
        </DialogHeader>
        <div className="flex gap-2">
          <Input
            value={subnet}
            placeholder={scan.data?.subnet ?? "192.168.1.0/24"}
            aria-label={t("Réseau")}
            onChange={(e) => setSubnet(e.target.value)}
          />
          <Button
            variant="outline"
            onClick={() => scan.mutate()}
            disabled={scan.isPending}
          >
            {scan.isPending ? <Loader2 className="animate-spin" /> : <Search />}
            {t("Rechercher")}
          </Button>
        </div>
        <div className="grid gap-2">
          {scan.isPending && (
            <p className="flex items-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="size-4 animate-spin" />
              {t("Recherche sur le réseau… (quelques secondes)")}
            </p>
          )}
          {scan.data && products.length === 0 && (
            <p className="text-sm text-muted-foreground">
              {t("Aucun produit trouvé sur {subnet}.", {
                subnet: scan.data.subnet,
              })}
            </p>
          )}
          {products.map((p) => (
            <div
              key={`${p.brand}-${p.host}`}
              className="flex flex-wrap items-center gap-3 rounded-2xl border p-3"
            >
              <div className="min-w-0 flex-1">
                <div className="font-medium">{p.model}</div>
                <div className="text-xs text-muted-foreground">
                  {[p.host, p.note].filter(Boolean).join(" · ")}
                </div>
              </div>
              {action(p)}
            </div>
          ))}
        </div>
        <div className="grid gap-2 border-t pt-4">
          <Label htmlFor="shelly-host">
            {t("Ou par son adresse (Shelly)")}
          </Label>
          <div className="flex gap-2">
            <Input
              id="shelly-host"
              value={host}
              placeholder="192.168.1.40"
              onChange={(e) => setHost(e.target.value)}
            />
            <Button
              variant="outline"
              disabled={!host.trim() || add.isPending}
              onClick={() => add.mutate({ name: "Shelly", host: host.trim() })}
            >
              <Plus /> {t("Ajouter")}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}

/** Settings › Modules: the sources of devices (Shelly…) and what each of their measures is. */
export function ModulesTab() {
  const [adding, setAdding] = useState(false)
  const sources = useQuery({ queryKey: ["sources"], queryFn: api.sources })
  const devices = useQuery({
    queryKey: ["devices"],
    queryFn: api.devices,
    refetchInterval: 5000,
  })
  const rooms = useQuery({ queryKey: ["rooms"], queryFn: api.rooms })

  if (sources.isLoading) return <Skeleton className="h-48 rounded-3xl" />
  const all = devices.data ?? []
  return (
    <div className="grid gap-6">
      <Card className="rounded-3xl">
        <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-3">
          <div>
            <CardTitle className="flex items-center gap-2">
              <Puzzle className="size-5" /> {t("Modules")}
            </CardTitle>
            <CardDescription>
              {t(
                "Les produits que Breezone lit et pilote en plus d'Airzone. Donnez un rôle à chaque mesure d'énergie : la page Énergie s'en sert."
              )}
            </CardDescription>
          </div>
          <Button onClick={() => setAdding(true)}>
            <Plus /> {t("Ajouter un module")}
          </Button>
        </CardHeader>
        {(sources.data ?? []).length === 0 && (
          <CardContent>
            <p className="text-sm text-muted-foreground">
              {t(
                "Aucun module pour l'instant : « Ajouter un module » cherche ceux de votre réseau."
              )}
            </p>
          </CardContent>
        )}
      </Card>
      {(sources.data ?? []).map((s) => (
        <SourceCard
          key={s.id}
          source={s}
          devices={all.filter((d) => d.source_id === s.id)}
          all={all}
          rooms={rooms.data ?? []}
        />
      ))}
      <AddModuleDialog open={adding} onClose={() => setAdding(false)} />
    </div>
  )
}
