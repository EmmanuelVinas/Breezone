import { useEffect, useState, type ReactNode } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  AirVent,
  Home,
  Loader2,
  PlugZap,
  Plus,
  RefreshCw,
  Search,
  Sun,
  Trash2,
  Zap,
} from "lucide-react"
import { useSearchParams } from "react-router"
import { toast } from "sonner"
import type {
  DeviceView,
  FoundProduct,
  MeasureRole,
  Room,
  SourceView,
} from "@/lib/api"
import { api } from "@/lib/api"
import { CHARGER_STATUS } from "@/lib/charger"
import { relativeTime } from "@/lib/format"
import { cn } from "@/lib/utils"
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

/** Drivers that can be added by their address. */
const BY_ADDRESS = [
  { driver: "shelly", label: "Shelly" },
  { driver: "trydan", label: "V2C Trydan" },
  { driver: "hoymiles", label: "Hoymiles (DTU)" },
]

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
    typeof s.status === "string"
      ? (CHARGER_STATUS[s.status] ?? s.status)
      : null,
    watts(s.power),
    typeof s.on === "boolean" ? (s.on ? t("allumé") : t("éteint")) : null,
    kwh(s.energy_in) && t("{value} consommés", { value: kwh(s.energy_in)! }),
    typeof s.energy_out === "number" && s.energy_out > 0
      ? t("{value} injectés", { value: kwh(s.energy_out)! })
      : null,
    typeof s.energy_today === "number"
      ? t("{value} aujourd'hui", { value: kwh(s.energy_today)! })
      : null,
    typeof s.session_energy === "number" && s.session_energy > 0
      ? t("{value} cette session", { value: kwh(s.session_energy)! })
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

/** What each driver reads and writes, and how it is reached (Settings › Modules › detail). */
const DRIVER: Record<
  string,
  {
    icon: typeof Zap
    tone: string
    connection: string
    every: string
    reads: string
    writes: string
    note?: string
  }
> = {
  shelly: {
    icon: Zap,
    tone: "bg-warning/15 text-warning",
    connection: t(
      "API RPC locale du Shelly : aucun compte Shelly Cloud nécessaire"
    ),
    every: t("toutes les 5 s"),
    reads: t(
      "puissance, tension, intensité, énergie consommée et injectée de chaque voie ; état des relais"
    ),
    writes: t("marche et arrêt des relais"),
  },
  trydan: {
    icon: PlugZap,
    tone: "bg-primary/15 text-primary",
    connection: t(
      "API HTTP locale de la borne : aucun compte ni réglage dans V2C Cloud"
    ),
    every: t("toutes les 5 s"),
    reads: t(
      "état, véhicule branché, puissance, énergie et durée de la session, intensité, pause, verrou"
    ),
    writes: t(
      "intensité (6 à 32 A), lancement et arrêt de la charge, verrou, mode dynamique"
    ),
    note: t(
      "L'API locale de la Trydan n'a pas de mot de passe : tout appareil du réseau local peut la piloter. Réservez-lui une adresse IP fixe (bail DHCP) ; si elle change, « Rechercher » retrouve la borne."
    ),
  },
  hoymiles: {
    icon: Sun,
    tone: "bg-warning/15 text-warning",
    connection: t(
      "Protocole local de la clé Wi-Fi (port 10081) : aucun compte S-Miles Cloud nécessaire"
    ),
    every: t("toutes les 30 s (la clé supporte mal les relevés fréquents)"),
    reads: t(
      "puissance, tension, fréquence, température et production de chaque micro-onduleur ; puissance et production de chaque panneau"
    ),
    writes: t("rien : lecture seule"),
  },
}

function Tile({
  label,
  value,
  sub,
}: {
  label: string
  value: ReactNode
  sub?: string
}) {
  return (
    <div className="rounded-2xl bg-muted/60 px-3 py-2.5">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="font-medium">{value}</div>
      {sub && <div className="text-xs text-muted-foreground">{sub}</div>}
    </div>
  )
}

function ModuleHead({
  icon,
  tone,
  name,
  sub,
  actions,
}: {
  icon: ReactNode
  tone: string
  name: string
  sub: string
  actions?: ReactNode
}) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <div className="flex min-w-0 items-center gap-3">
        <span
          className={cn(
            "grid size-11 shrink-0 place-items-center rounded-2xl",
            tone
          )}
        >
          {icon}
        </span>
        <div className="min-w-0">
          <h2 className="font-heading text-xl font-semibold">{name}</h2>
          <div className="text-sm text-muted-foreground">{sub}</div>
        </div>
      </div>
      {actions && <div className="flex flex-wrap gap-2">{actions}</div>}
    </div>
  )
}

/** Who sets the charging current: Breezone, or the charger's own dynamic mode. */
function CurrentOwner({ charger }: { charger: DeviceView }) {
  const qc = useQueryClient()
  const set = useMutation({
    mutationFn: (on: boolean) =>
      api.act(charger.id, "ev.charger", "set_auto_limit", { on }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["devices"] }),
    onError: (e) => toast.error(errorText(e)),
  })
  const auto = charger.state?.auto_limit === true
  const choices = [
    {
      on: false,
      icon: Home,
      title: "Breezone",
      text: t(
        "Breezone désactive le mode dynamique de la borne et règle lui-même l'intensité."
      ),
    },
    {
      on: true,
      icon: PlugZap,
      title: t("La borne (mode dynamique V2C)"),
      text: t(
        "Breezone ne fait que lire : utile si la borne a déjà sa pince de mesure et que ses réglages vous conviennent."
      ),
    },
  ]
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{t("Qui règle l'intensité ?")}</CardTitle>
        <CardDescription>
          {t(
            "La Trydan a son propre mode solaire (« dynamique ») : un seul des deux doit piloter."
          )}
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-2.5 sm:grid-cols-2">
        {choices.map((c) => (
          <button
            key={c.title}
            type="button"
            role="radio"
            aria-checked={auto === c.on}
            disabled={set.isPending || !charger.available}
            onClick={() => auto !== c.on && set.mutate(c.on)}
            className={cn(
              "flex flex-col items-start gap-1.5 rounded-2xl border p-3.5 text-left transition-colors disabled:opacity-60",
              auto === c.on
                ? "border-transparent bg-primary/12 ring-2 ring-primary"
                : "hover:bg-muted/60"
            )}
          >
            <c.icon
              className={cn(
                "size-5",
                auto === c.on ? "text-primary" : "text-muted-foreground"
              )}
            />
            <b className="text-sm">{c.title}</b>
            <span className="text-xs text-muted-foreground">{c.text}</span>
          </button>
        ))}
      </CardContent>
    </Card>
  )
}

function SourceDetail({
  source,
  devices,
  all,
  rooms,
  onSearch,
}: {
  source: SourceView
  devices: DeviceView[]
  all: DeviceView[]
  rooms: Room[]
  onSearch: () => void
}) {
  const qc = useQueryClient()
  const refresh = useMutation({
    mutationFn: () => api.refreshSource(source.id),
    onSuccess: (s) => {
      qc.invalidateQueries({ queryKey: ["sources"] })
      qc.invalidateQueries({ queryKey: ["devices"] })
      if (s.status.state === "online")
        toast.success(t("« {name} » répond", { name: s.name }))
      else toast.error(s.status.last_error ?? t("Injoignable"))
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
  const d = DRIVER[source.driver]
  const Icon = d?.icon ?? Zap
  const host = typeof source.config.host === "string" ? source.config.host : ""
  const charger = devices.find((x) => x.traits.includes("ev.charger@1"))
  return (
    <>
      <ModuleHead
        icon={<Icon className="size-5" />}
        tone={d?.tone ?? "bg-muted text-muted-foreground"}
        name={source.name}
        sub={[
          modelDetail(source),
          devices.length === 1
            ? t("1 appareil")
            : t("{n} appareils", { n: devices.length }),
        ]
          .filter(Boolean)
          .join(" · ")}
        actions={
          <Confirm
            title={t("Retirer « {name} » ?", { name: source.name })}
            description={t(
              "Ses appareils disparaissent de Breezone, avec leurs réglages. Le matériel n'est pas modifié."
            )}
            onConfirm={() => remove.mutate()}
          >
            <Button variant="outline" className="text-destructive">
              <Trash2 /> {t("Retirer")}
            </Button>
          </Confirm>
        }
      />
      <Card className="rounded-3xl">
        <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-3">
          <div>
            <CardTitle>{t("Connexion")}</CardTitle>
            {d && <CardDescription>{d.connection}</CardDescription>}
          </div>
          <StateBadge status={source.status} />
        </CardHeader>
        <CardContent className="grid gap-4">
          <div className="grid gap-1.5">
            <span className="text-xs text-muted-foreground">
              {t("Adresse IP ou nom")}
            </span>
            <div className="flex flex-wrap gap-2">
              <span className="tabular min-w-40 flex-1 rounded-2xl bg-muted/60 px-3 py-2 font-mono text-sm text-primary">
                {host || "–"}
              </span>
              <Button variant="outline" onClick={onSearch}>
                <Search /> {t("Rechercher")}
              </Button>
              <Button
                variant="outline"
                onClick={() => refresh.mutate()}
                disabled={refresh.isPending}
              >
                <RefreshCw
                  className={refresh.isPending ? "animate-spin" : ""}
                />{" "}
                {t("Tester")}
              </Button>
            </div>
          </div>
          <div className="grid grid-cols-2 gap-2 md:grid-cols-4">
            <Tile label={t("État")} value={STATE_LABEL[source.status.state]} />
            <Tile label={t("Firmware")} value={source.status.firmware ?? "–"} />
            <Tile
              label={t("Dernier relevé")}
              value={relativeTime(source.status.last_seen)}
            />
            <Tile label={t("Relevé")} value={d?.every ?? "–"} />
          </div>
          {source.status.state !== "online" && source.status.last_error && (
            <p className="text-sm text-destructive">
              {source.status.last_error}
            </p>
          )}
        </CardContent>
      </Card>
      {charger && <CurrentOwner charger={charger} />}
      <Card className="rounded-3xl">
        <CardHeader>
          <CardTitle>{t("Appareils")}</CardTitle>
          <CardDescription>
            {devices.some((x) => x.traits.includes("energy.power@1"))
              ? t(
                  "Nommez chaque appareil et dites ce que mesure chaque voie : la page Énergie s'en sert."
                )
              : t("Nommez chaque appareil.")}
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-2">
          {devices.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              {t("Aucun appareil lu pour l'instant.")}
            </p>
          ) : (
            devices.map((x) => (
              <DeviceRow key={x.id} device={x} devices={all} rooms={rooms} />
            ))
          )}
        </CardContent>
      </Card>
      {d && (
        <Card className="rounded-3xl">
          <CardHeader>
            <CardTitle>{t("Ce que fait le module")}</CardTitle>
          </CardHeader>
          <CardContent className="grid gap-0 divide-y text-sm">
            <div className="flex gap-4 py-2.5">
              <b className="w-14 shrink-0">{t("Lu")}</b>
              <span className="text-muted-foreground">{d.reads}</span>
            </div>
            <div className="flex gap-4 py-2.5">
              <b className="w-14 shrink-0">{t("Écrit")}</b>
              <span className="text-muted-foreground">{d.writes}</span>
            </div>
          </CardContent>
        </Card>
      )}
      {d?.note && (
        <p className="rounded-2xl border border-dashed px-4 py-3 text-sm text-muted-foreground">
          {d.note}
        </p>
      )}
    </>
  )
}

/** Airzone, built in: its installations and the Airzone account (given by the settings page). */
function AirzoneDetail({ children }: { children: ReactNode }) {
  const status = useQuery({ queryKey: ["status"], queryFn: api.status })
  const installations = status.data ?? []
  const zones = installations.reduce((n, i) => n + i.zones.length, 0)
  return (
    <>
      <ModuleHead
        icon={<AirVent className="size-5" />}
        tone="bg-heat/15 text-heat"
        name="Airzone"
        sub={[
          t("Climatisation · intégré à Breezone"),
          installations.length === 1
            ? t("1 installation")
            : t("{n} installations", { n: installations.length }),
          zones ? t("{n} zones", { n: zones }) : null,
        ]
          .filter(Boolean)
          .join(" · ")}
      />
      <div>{children}</div>
    </>
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
  const [driver, setDriver] = useState(BY_ADDRESS[0].driver)
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
      driver: string
      name: string
      host: string
      device_id?: string | null
    }) =>
      api.createSource({
        driver: p.driver,
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
          add.mutate({
            driver: p.driver,
            name: p.model,
            host: p.host,
            device_id: p.device_id,
          })
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
              "Breezone cherche sur le réseau les produits qu'il sait piloter : WebServers Airzone, Shelly (Plus, Pro, Gen3, Gen4), bornes V2C Trydan et clés Wi-Fi Hoymiles."
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
          <Label htmlFor="module-host">{t("Ou par son adresse")}</Label>
          <div className="flex gap-2">
            <Select value={driver} onValueChange={setDriver}>
              <SelectTrigger className="w-40" aria-label={t("Produit")}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {BY_ADDRESS.map((d) => (
                  <SelectItem key={d.driver} value={d.driver}>
                    {d.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Input
              id="module-host"
              value={host}
              placeholder="192.168.1.40"
              onChange={(e) => setHost(e.target.value)}
            />
            <Button
              variant="outline"
              disabled={!host.trim() || add.isPending}
              onClick={() =>
                add.mutate({
                  driver,
                  name:
                    BY_ADDRESS.find((d) => d.driver === driver)?.label ??
                    driver,
                  host: host.trim(),
                })
              }
            >
              <Plus /> {t("Ajouter")}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}

/** Settings › Modules: the list of modules (Airzone first), and the detail of the one chosen. */
export function ModulesTab({
  airzone,
  initial,
}: {
  /** The Airzone configuration (installations, account), given by the settings page. */
  airzone: ReactNode
  /** Module shown first ("airzone" or a source id). */
  initial?: string
}) {
  const [adding, setAdding] = useState(false)
  const [params, setParams] = useSearchParams()
  const sources = useQuery({ queryKey: ["sources"], queryFn: api.sources })
  const devices = useQuery({
    queryKey: ["devices"],
    queryFn: api.devices,
    refetchInterval: 5000,
  })
  const rooms = useQuery({ queryKey: ["rooms"], queryFn: api.rooms })
  const status = useQuery({ queryKey: ["status"], queryFn: api.status })

  if (sources.isLoading) return <Skeleton className="h-48 rounded-3xl" />
  const all = devices.data ?? []
  const list = sources.data ?? []
  const wanted = params.get("module") ?? initial ?? "airzone"
  const selected = list.find((s) => String(s.id) === wanted)
  const choose = (id: string) =>
    setParams((p) => {
      const next = new URLSearchParams(p)
      next.set("tab", "modules")
      next.set("module", id)
      next.delete("airzone")
      return next
    })
  const installations = status.data ?? []
  const airzoneOnline =
    installations.length > 0 &&
    installations.every((i) => i.connection.state === "online")
  const item = (
    id: string,
    active: boolean,
    icon: ReactNode,
    name: string,
    dot: string | null
  ) => (
    <button
      key={id}
      type="button"
      onClick={() => choose(id)}
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex shrink-0 items-center gap-2.5 rounded-xl px-3 py-2.5 text-left text-sm font-medium text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground md:w-full",
        active && "bg-muted text-foreground"
      )}
    >
      {icon}
      <span className="min-w-0 flex-1 truncate">{name}</span>
      {dot && <span className={cn("size-2 shrink-0 rounded-full", dot)} />}
    </button>
  )
  const dotOf = (s: SourceView) =>
    s.status.state === "online"
      ? "bg-success"
      : s.status.state === "offline" || s.status.state === "error"
        ? "bg-destructive"
        : "bg-muted-foreground"

  return (
    <div className="grid items-start gap-5 md:grid-cols-[220px_minmax(0,1fr)]">
      <nav
        aria-label={t("Modules")}
        className="flex gap-1 overflow-x-auto rounded-3xl border bg-card p-2 md:flex-col md:overflow-visible"
      >
        {item(
          "airzone",
          !selected,
          <AirVent className="size-4 text-heat" />,
          "Airzone",
          installations.length
            ? airzoneOnline
              ? "bg-success"
              : "bg-warning"
            : null
        )}
        {list.map((s) => {
          const Icon = DRIVER[s.driver]?.icon ?? Zap
          return item(
            String(s.id),
            selected?.id === s.id,
            <Icon className="size-4 text-warning" />,
            s.name,
            dotOf(s)
          )
        })}
        <button
          type="button"
          onClick={() => setAdding(true)}
          className="flex shrink-0 items-center gap-2.5 rounded-xl px-3 py-2.5 text-sm font-medium text-primary hover:bg-primary/10 md:w-full"
        >
          <Plus className="size-4" /> {t("Ajouter un module")}
        </button>
      </nav>
      <div className="grid min-w-0 gap-5">
        {selected ? (
          <SourceDetail
            key={selected.id}
            source={selected}
            devices={all.filter((d) => d.source_id === selected.id)}
            all={all}
            rooms={rooms.data ?? []}
            onSearch={() => setAdding(true)}
          />
        ) : (
          <AirzoneDetail>{airzone}</AirzoneDetail>
        )}
      </div>
      <AddModuleDialog open={adding} onClose={() => setAdding(false)} />
    </div>
  )
}
