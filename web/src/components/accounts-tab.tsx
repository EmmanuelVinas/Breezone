import { useMemo, useState } from "react"
import { Link } from "react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  BatteryMedium,
  Blinds,
  CalendarDays,
  Cctv,
  CloudSun,
  ExternalLink,
  Gauge,
  House,
  Loader2,
  Puzzle,
  RefreshCw,
  Search,
  Snowflake,
  Sun,
  Trash2,
  type LucideIcon,
} from "lucide-react"
import { toast } from "sonner"
import type { AccountKind, AccountView, ModuleView } from "@/lib/api"
import { api } from "@/lib/api"
import { errorText, relativeTime } from "@/lib/format"
import { lang, t } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import { ConfirmDialog } from "@/components/confirm"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Switch } from "@/components/ui/switch"

/** A field to fill to connect an account. */
type Field = {
  name: string
  label: string
  secret?: boolean
  placeholder?: string
  /** Not needed to connect (a code asked only sometimes). */
  optional?: boolean
}

/** What each kind of account is for, and how to get what it asks. */
type Kind = {
  title: string
  icon: LucideIcon
  description: string
  steps: string[]
  /** Where to get what it asks (none for a module's own account). */
  link?: { url: string; label: string }
  fields: Field[]
  /** Connected from elsewhere (its module). */
  elsewhere?: { to: string; label: string }
  /** What switching it off does. */
  off: string
}

const KINDS: Partial<Record<AccountKind, Kind>> = {
  airzone: {
    title: "Airzone Cloud",
    icon: Snowflake,
    description: t(
      "Les noms, les groupes et les programmations de vos installations Airzone. La climatisation, elle, se pilote toujours en local."
    ),
    steps: [],
    link: { url: "https://www.airzonecloud.com", label: "airzonecloud.com" },
    fields: [],
    elsewhere: {
      to: "/settings?tab=modules&airzone=cloud",
      label: t("Gérer dans Modules › Airzone"),
    },
    off: t("Désactivé : plus de synchronisation avec Airzone Cloud."),
  },
  rte: {
    title: "RTE",
    icon: CalendarDays,
    description: t(
      "La couleur officielle des jours Tempo, publiée par RTE, pour les prix et les automatismes. Sans compte, Breezone lit un relais public."
    ),
    steps: [
      t("Créez un compte gratuit sur data.rte-france.com."),
      t(
        "Créez une application, puis abonnez-la à l'API « Tempo Like Supply Contract »."
      ),
      t("Copiez l'ID client et l'ID secret de l'application ici."),
    ],
    link: {
      url: "https://data.rte-france.com/catalog/-/api/consumption/Tempo-Like-Supply-Contract/v1.1",
      label: "data.rte-france.com",
    },
    fields: [
      { name: "client_id", label: t("ID client") },
      { name: "client_secret", label: t("ID secret"), secret: true },
    ],
    off: t("Désactivé : les couleurs Tempo viennent du relais public."),
  },
  linky: {
    title: "Linky",
    icon: Gauge,
    description: t(
      "Votre consommation relevée par Enedis, jour par jour et à la demi-heure, et ce qu'elle a coûté à votre tarif, sans boîtier. Les journées arrivent le lendemain."
    ),
    steps: [
      t(
        "Sur conso.boris.sh, donnez votre accord à Enedis : vous recevez un jeton personnel."
      ),
      t(
        "Le numéro du compteur (PRM, 14 chiffres) est sur votre facture, ou sur le compteur (touche +)."
      ),
      t(
        "Pour les demi-heures, activez la collecte de la courbe de charge dans votre espace Enedis."
      ),
    ],
    link: { url: "https://conso.boris.sh", label: "conso.boris.sh" },
    fields: [
      { name: "token", label: t("Jeton Conso API"), secret: true },
      {
        name: "prm",
        label: t("Numéro du compteur (PRM)"),
        placeholder: "12345678901234",
      },
    ],
    off: t("Désactivé : plus de relevés Enedis ; ceux déjà lus restent."),
  },
  netatmo: {
    title: "Netatmo · Legrand Home + Control",
    icon: House,
    description: t(
      "Ce que la passerelle Legrand ne donne qu'à son cloud : l'Ecometer, l'arrêt des volets en cours de route, les noms et les pièces. Les volets restent pilotés en local (HomeKit)."
    ),
    steps: [
      t("Sur dev.netatmo.com, créez une application (« Create »)."),
      t(
        "Dans son « Token generator », cochez read_magellan et write_magellan, puis « Generate Token »."
      ),
      t(
        "Copiez ici l'ID client, le secret et le refresh token : Breezone renouvelle l'accès lui-même."
      ),
    ],
    link: { url: "https://dev.netatmo.com/apps", label: "dev.netatmo.com" },
    fields: [
      { name: "client_id", label: t("ID client") },
      { name: "client_secret", label: t("Secret client"), secret: true },
      { name: "refresh_token", label: t("Refresh token"), secret: true },
    ],
    off: t(
      "Désactivé : le module Netatmo s'arrête, les appareils locaux continuent."
    ),
  },
  ezviz: {
    title: "EZVIZ",
    icon: Cctv,
    description: t(
      "Vos caméras et sonnettes EZVIZ : leur dernier événement et son image, et les coups de sonnette pour les automatismes. Le même compte que l'application EZVIZ, sans clé développeur."
    ),
    steps: [
      t(
        "Conseil : créez un second compte EZVIZ et partagez-lui vos appareils depuis l'application ; Breezone n'a pas le mot de passe de votre compte principal."
      ),
      t(
        "Si le compte a la double authentification, une première connexion fait envoyer un code par EZVIZ : saisissez-le dans « Code reçu », puis connectez à nouveau."
      ),
      t(
        "Si EZVIZ chiffre les images, donnez le code de vérification des caméras (6 lettres, sur leur étiquette)."
      ),
    ],
    link: { url: "https://www.ezviz.com", label: "ezviz.com" },
    fields: [
      { name: "account", label: t("Identifiant (e-mail ou téléphone)") },
      { name: "password", label: t("Mot de passe"), secret: true },
      { name: "code", label: t("Code reçu"), optional: true },
      {
        name: "image_code",
        label: t("Code de vérification des caméras"),
        secret: true,
        optional: true,
      },
    ],
    off: t(
      "Désactivé : le module EZVIZ s'arrête, plus d'images ni de coups de sonnette."
    ),
  },
  anker_cloud: {
    title: "Anker SOLIX",
    icon: BatteryMedium,
    description: t(
      "Votre SolarBank : sa charge, ce qu'elle charge ou restitue, ce que donnent ses panneaux et ce qu'elle envoie à la maison, toutes les minutes. Anker ne lui donne pas d'accès local : elle passe par le cloud d'Anker, avec le compte de l'application. Les prises et le compteur Anker se lisent en local (module Anker SOLIX Modbus TCP)."
    ),
    steps: [
      t(
        "Conseil : créez un second compte Anker et partagez-lui le système depuis l'application (Membres) ; une connexion à votre compte principal pourrait déconnecter l'application."
      ),
      t(
        "Si Anker demande un code de vérification, connectez-vous une fois à ce compte dans l'application Anker, puis réessayez ici."
      ),
    ],
    link: { url: "https://www.ankersolix.com", label: "ankersolix.com" },
    fields: [
      { name: "email", label: t("E-mail du compte Anker") },
      { name: "password", label: t("Mot de passe"), secret: true },
      {
        name: "country",
        label: t("Pays du compte (FR par défaut)"),
        placeholder: "FR",
        optional: true,
      },
    ],
    off: t(
      "Désactivé : le module Anker SOLIX (cloud) s'arrête, la SolarBank n'est plus lue."
    ),
  },
  deltadore: {
    title: "Delta Dore",
    icon: Blinds,
    description: t(
      "Le compte de l'application Tydom, seulement pour le mot de passe de vos passerelles Tydom : Delta Dore le garde, il n'est sur aucune étiquette. Breezone le reçoit une fois, le garde chiffré, puis pilote les passerelles en local, sans le cloud."
    ),
    steps: [
      t(
        "Connectez ici le compte de l'application Tydom (e-mail et mot de passe)."
      ),
      t(
        "Ajoutez ensuite la passerelle dans Modules (Delta Dore Tydom) avec son adresse IP et son adresse MAC, en laissant son mot de passe vide."
      ),
      t(
        "Une passerelle ajoutée au compte plus tard : utilisez « Tester » pour que Breezone apprenne son mot de passe."
      ),
    ],
    link: { url: "https://www.deltadore.fr", label: "deltadore.fr" },
    fields: [
      { name: "email", label: t("E-mail du compte Delta Dore") },
      { name: "password", label: t("Mot de passe"), secret: true },
    ],
    off: t(
      "Désactivé : les passerelles déjà ajoutées gardent leur connexion locale tant que Breezone tourne ; au prochain démarrage, celles sans mot de passe saisi ne se connectent plus."
    ),
  },
  weather: {
    title: t("Météo"),
    icon: CloudSun,
    description: t(
      "Prévisions et vigilances de chaque installation, par Open-Meteo (données Météo-France) et MeteoAlarm. Sans compte : seules les coordonnées de la ville sont envoyées."
    ),
    steps: [],
    link: { url: "https://open-meteo.com", label: "open-meteo.com" },
    fields: [],
    off: t(
      "Désactivé : plus de météo ni de température extérieure ; les automatismes qui en dépendent attendent."
    ),
  },
  forecast_solar: {
    title: "Forecast.Solar",
    icon: Sun,
    description: t(
      "La production solaire prévue selon la météo : maintenant, aujourd'hui, ce qui reste à produire et demain. Sans compte : seules la position de la ville et la description des panneaux sont envoyées."
    ),
    steps: [
      t(
        "Donnez la puissance crête des panneaux (en kWc, sur la facture de l'installateur), leur inclinaison et leur orientation."
      ),
      t(
        "La prévision est faite pour la ville de l'installation. Une clé d'API Forecast.Solar, facultative, donne des prévisions plus fines."
      ),
    ],
    link: { url: "https://forecast.solar", label: "forecast.solar" },
    fields: [
      {
        name: "kwp",
        label: t("Puissance crête (kWc)"),
        placeholder: "4,5",
      },
      {
        name: "declination",
        label: t("Inclinaison (°, 0 = à plat)"),
        placeholder: "30",
        optional: true,
      },
      {
        name: "azimuth",
        label: t("Orientation (°, 0 = sud, -90 = est, 90 = ouest)"),
        placeholder: "0",
        optional: true,
      },
      {
        name: "api_key",
        label: t("Clé d'API (facultative)"),
        secret: true,
        optional: true,
      },
    ],
    off: t(
      "Désactivé : plus de prévision de production ; les automatismes qui en dépendent attendent."
    ),
  },
}

/**
 * What an account is: the words written for those Breezone knows, else what the manifest of its
 * module says (a module installed from a library brings its own account and its form).
 */
function kindOf(a: AccountView, modules: ModuleView[] | undefined): Kind {
  const known = KINDS[a.kind]
  if (known) return known
  const m = modules?.find((x) => x.id === a.kind)?.manifest
  const title = m?.name[lang()] ?? a.kind
  return {
    title,
    icon: Puzzle,
    description:
      m?.description?.[lang()] ??
      t("Le compte que demande le module {name}.", { name: title }),
    steps: [],
    fields: (m?.config ?? []).map((f) => ({
      name: f.name,
      label: f.label[lang()],
      secret: f.type === "password",
      optional: f.optional,
    })),
    off: t("Désactivé : le module ne s'en sert plus."),
  }
}

/** The modules of this server (for the accounts their manifests describe). */
function useKind(a: AccountView): Kind {
  const modules = useQuery({ queryKey: ["modules"], queryFn: api.modules })
  return kindOf(a, modules.data)
}

function StateBadge({ a }: { a: AccountView }) {
  const [label, tone] = !a.enabled
    ? [t("Désactivé"), "bg-muted-foreground"]
    : a.service
      ? a.state === "error"
        ? [t("En erreur"), "bg-destructive"]
        : [t("Actif"), "bg-success"]
      : !a.connected
        ? [t("Non connecté"), "bg-muted-foreground"]
        : a.state === "error" || a.state === "expired"
          ? [t("En erreur"), "bg-destructive"]
          : [t("Connecté"), "bg-success"]
  return (
    <Badge variant="outline" className="gap-1.5">
      <span className={cn("size-2 rounded-full", tone)} />
      {label}
    </Badge>
  )
}

/** What the account reaches, in a few words. */
function summary(a: AccountView): string | null {
  const c = a.config
  if (a.kind === "airzone") return c.email ?? null
  if (a.kind === "linky")
    return c.prm ? t("Compteur {prm}", { prm: c.prm }) : null
  if (a.kind === "ezviz" && c.email)
    return t("{email} : {n} appareil(s)", {
      email: c.email,
      n: c.devices?.length ?? 0,
    })
  if (a.kind === "deltadore" && c.email)
    return t("{email} : {n} passerelle(s)", {
      email: c.email,
      n: c.gateways?.length ?? 0,
    })
  if (a.kind === "anker_cloud" && c.email)
    return t("{email} : {n} SolarBank", {
      email: c.email,
      n: c.solarbanks ?? 0,
    })
  if (a.kind === "netatmo" && c.homes)
    return c.homes
      .map((h) =>
        t("{name} : {rooms} pièces, {modules} modules", {
          name: h.name,
          rooms: h.rooms,
          modules: h.modules,
        })
      )
      .join(" · ")
  if (a.kind === "forecast_solar" && typeof c.kwp === "number")
    return t("{kwp} kWc · inclinaison {dec}° · orientation {az}°", {
      kwp: c.kwp,
      dec: c.declination ?? 30,
      az: c.azimuth ?? 0,
    })
  return c.client_id ? t("Application {id}", { id: c.client_id }) : null
}

function useDone() {
  const qc = useQueryClient()
  return (v?: AccountView) => {
    qc.invalidateQueries({ queryKey: ["accounts"] })
    qc.invalidateQueries({ queryKey: ["linky"] })
    if (v)
      qc.setQueryData<AccountView[]>(["accounts"], (list) =>
        list?.map((x) => (x.kind === v.kind ? v : x))
      )
  }
}

/** Switching an account or a service on and off, from its tile or its detail. */
function useEnable(a: AccountView) {
  const qc = useQueryClient()
  const done = useDone()
  const enable = useMutation({
    mutationFn: (on: boolean) => api.setAccountEnabled(a.kind, on),
    onSuccess: (v) => {
      done(v)
      qc.invalidateQueries({ queryKey: ["sources"] })
      qc.invalidateQueries({ queryKey: ["weather"] })
      qc.invalidateQueries({ queryKey: ["cloud"] })
    },
    onError: (e) => toast.error(errorText(e)),
  })
  return { enable, enabled: enable.isPending ? enable.variables : a.enabled }
}

/** An account or a service in the grid: what it is and its state; its detail opens on a click. */
function AccountTile({ a }: { a: AccountView }) {
  const kind = useKind(a)
  const [open, setOpen] = useState(false)
  const { enable, enabled } = useEnable(a)
  const what = a.connected && enabled ? summary(a) : null
  const failed =
    enabled && (a.state === "error" || a.state === "expired") && a.connected
  return (
    <>
      <Card
        role="button"
        tabIndex={0}
        onClick={() => setOpen(true)}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault()
            setOpen(true)
          }
        }}
        className="cursor-pointer rounded-3xl py-4 transition-colors hover:bg-muted/40 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
      >
        <CardContent className="flex items-start gap-3 px-4">
          <span
            className={cn(
              "grid size-10 shrink-0 place-items-center rounded-2xl",
              (a.connected || a.service) && enabled
                ? "bg-primary/15 text-primary"
                : "bg-muted text-muted-foreground"
            )}
          >
            <kind.icon className="size-5" />
          </span>
          <div className="min-w-0 flex-1">
            <div className="truncate font-medium">{kind.title}</div>
            <div className="mt-1">
              <StateBadge a={a} />
            </div>
            <p
              className={cn(
                "mt-2 line-clamp-2 text-xs",
                failed ? "text-destructive" : "text-muted-foreground"
              )}
            >
              {failed && a.last_error
                ? a.last_error
                : (what ?? kind.description)}
            </p>
          </div>
          {(a.connected || a.service) && (
            <Switch
              checked={enabled}
              disabled={enable.isPending}
              onClick={(e) => e.stopPropagation()}
              onKeyDown={(e) => e.stopPropagation()}
              onCheckedChange={(on) => enable.mutate(on)}
              aria-label={t("Activer {name}", { name: kind.title })}
            />
          )}
        </CardContent>
      </Card>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-xl">
          <DialogHeader>
            <div className="flex items-center gap-3 pr-6">
              <span className="grid size-10 shrink-0 place-items-center rounded-2xl bg-primary/15 text-primary">
                <kind.icon className="size-5" />
              </span>
              <div className="min-w-0 flex-1">
                <DialogTitle className="flex flex-wrap items-center gap-2">
                  {kind.title} <StateBadge a={a} />
                </DialogTitle>
              </div>
              {(a.connected || a.service) && (
                <Switch
                  checked={enabled}
                  disabled={enable.isPending}
                  onCheckedChange={(on) => enable.mutate(on)}
                  aria-label={t("Activer {name}", { name: kind.title })}
                />
              )}
            </div>
            <DialogDescription>{kind.description}</DialogDescription>
          </DialogHeader>
          <AccountDetail
            key={`${a.kind}-${a.connected}`}
            a={a}
            enabled={enabled}
          />
        </DialogContent>
      </Dialog>
    </>
  )
}

function AccountDetail({ a, enabled }: { a: AccountView; enabled: boolean }) {
  const kind = useKind(a)
  const [editing, setEditing] = useState(!a.connected)
  const [values, setValues] = useState<Record<string, string>>({})
  const [removing, setRemoving] = useState(false)
  const done = useDone()
  const connect = useMutation({
    mutationFn: () => api.connectAccount(a.kind, values),
    onSuccess: (v) => {
      toast.success(t("Compte {name} connecté", { name: kind.title }))
      setEditing(false)
      setValues({})
      done(v)
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const test = useMutation({
    mutationFn: () => api.testAccount(a.kind),
    onSuccess: (v) => {
      if (v.state === "ok") toast.success(t("Le compte répond"))
      else toast.error(v.last_error ?? t("Le compte ne répond pas"))
      done(v)
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const remove = useMutation({
    mutationFn: () => api.deleteAccount(a.kind),
    onSuccess: () => {
      toast.success(t("Compte {name} retiré", { name: kind.title }))
      setEditing(true)
      done()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const filled = kind.fields.every((f) => f.optional || values[f.name]?.trim())
  const what = summary(a)
  return (
    <div className="grid gap-4">
      {!enabled && <p className="text-sm text-muted-foreground">{kind.off}</p>}
      {a.connected && enabled && (
        <div className="grid gap-1 text-sm">
          {what && <span>{what}</span>}
          <span className="text-muted-foreground">
            {a.last_ok
              ? t("A fonctionné {when}", { when: relativeTime(a.last_ok) })
              : t("Pas encore utilisé")}
          </span>
          {a.state === "error" && a.last_error && (
            <span className="text-destructive">{a.last_error}</span>
          )}
        </div>
      )}
      {a.service ? (
        enabled && (
          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="outline"
              onClick={() => test.mutate()}
              disabled={test.isPending}
            >
              <RefreshCw className={test.isPending ? "animate-spin" : ""} />
              {t("Tester")}
            </Button>
            {kind.link && (
              <a
                href={kind.link.url}
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-1 text-sm font-medium text-primary hover:underline"
              >
                {kind.link.label} <ExternalLink className="size-3.5" />
              </a>
            )}
          </div>
        )
      ) : kind.elsewhere ? (
        <div>
          <Button asChild variant="outline">
            <Link to={kind.elsewhere.to}>{kind.elsewhere.label}</Link>
          </Button>
        </div>
      ) : editing ? (
        <form
          className="grid gap-3"
          onSubmit={(e) => {
            e.preventDefault()
            if (filled) connect.mutate()
          }}
        >
          <ol className="grid list-decimal gap-1 pl-5 text-sm text-muted-foreground">
            {kind.steps.map((s) => (
              <li key={s}>{s}</li>
            ))}
          </ol>
          {kind.link && (
            <a
              href={kind.link.url}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-1 text-sm font-medium text-primary hover:underline"
            >
              {kind.link.label} <ExternalLink className="size-3.5" />
            </a>
          )}
          <div className="grid gap-3 sm:grid-cols-2">
            {kind.fields.map((f) => (
              <div key={f.name} className="grid gap-1.5">
                <Label htmlFor={`${a.kind}-${f.name}`}>{f.label}</Label>
                <Input
                  id={`${a.kind}-${f.name}`}
                  type={f.secret ? "password" : "text"}
                  autoComplete="off"
                  placeholder={f.placeholder}
                  value={values[f.name] ?? ""}
                  onChange={(e) =>
                    setValues({ ...values, [f.name]: e.target.value })
                  }
                />
              </div>
            ))}
          </div>
          <div className="flex flex-wrap gap-2">
            <Button type="submit" disabled={!filled || connect.isPending}>
              {connect.isPending && <Loader2 className="animate-spin" />}
              {t("Connecter")}
            </Button>
            {a.connected && (
              <Button
                type="button"
                variant="ghost"
                onClick={() => setEditing(false)}
              >
                {t("Annuler")}
              </Button>
            )}
          </div>
        </form>
      ) : (
        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            onClick={() => test.mutate()}
            disabled={test.isPending}
          >
            <RefreshCw className={test.isPending ? "animate-spin" : ""} />
            {t("Tester")}
          </Button>
          <Button variant="outline" onClick={() => setEditing(true)}>
            {t("Changer les identifiants")}
          </Button>
          <Button
            variant="ghost"
            className="text-destructive"
            onClick={() => setRemoving(true)}
          >
            <Trash2 /> {t("Retirer")}
          </Button>
        </div>
      )}
      {a.kind === "airzone" && a.connected && enabled && (
        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            onClick={() => test.mutate()}
            disabled={test.isPending}
          >
            <RefreshCw className={test.isPending ? "animate-spin" : ""} />
            {t("Synchroniser")}
          </Button>
        </div>
      )}
      <ConfirmDialog
        open={removing}
        title={t("Retirer le compte {name} ?", { name: kind.title })}
        description={t(
          "Breezone oublie ses identifiants. Ce qu'il a déjà lu reste ; ce qui en dépend s'arrête."
        )}
        onConfirm={() => {
          setRemoving(false)
          remove.mutate()
        }}
        onCancel={() => setRemoving(false)}
      />
    </div>
  )
}

/** Lower case, without accents: « Météo » is found by « meteo ». */
const plain = (s: string) =>
  s
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()

/**
 * Settings › Accounts and services: the online services Breezone may use, always in addition to
 * the local. Those in use first, then the others; a search narrows them.
 */
export function AccountsTab() {
  const accounts = useQuery({ queryKey: ["accounts"], queryFn: api.accounts })
  const modules = useQuery({ queryKey: ["modules"], queryFn: api.modules })
  const [query, setQuery] = useState("")
  const shown = useMemo(() => {
    const words = plain(query).split(/\s+/).filter(Boolean)
    const list = (accounts.data ?? []).filter((a) => {
      const k = kindOf(a, modules.data)
      const text = plain(`${k.title} ${k.description} ${k.link?.label ?? ""}`)
      return words.every((w) => text.includes(w))
    })
    const rank = (a: AccountView) => (a.connected ? 0 : 1)
    return [...list].sort((x, y) => rank(x) - rank(y))
  }, [accounts.data, modules.data, query])
  return (
    <div className="grid gap-4">
      <p className="text-sm text-muted-foreground">
        {t(
          "Un compte ou un service relie Breezone à internet, toujours en plus de ce qu'il lit en local : rien n'en dépend pour piloter la maison. Chacun se désactive sans être oublié ; les identifiants restent chiffrés sur ce serveur."
        )}
      </p>
      <div className="relative max-w-md">
        <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          type="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("Rechercher un compte ou un service")}
          aria-label={t("Rechercher un compte ou un service")}
          className="pl-9"
        />
      </div>
      {accounts.data && shown.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          {t("Aucun compte ni service ne correspond à « {query} ».", {
            query: query.trim(),
          })}
        </p>
      ) : (
        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
          {shown.map((a) => (
            <AccountTile key={a.kind} a={a} />
          ))}
        </div>
      )}
    </div>
  )
}
