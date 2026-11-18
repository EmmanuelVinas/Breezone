import { useMemo, useState } from "react"
import { Link } from "react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  CalendarDays,
  CloudSun,
  ExternalLink,
  Gauge,
  House,
  Loader2,
  RefreshCw,
  Search,
  Snowflake,
  Trash2,
  type LucideIcon,
} from "lucide-react"
import { toast } from "sonner"
import type { AccountKind, AccountView } from "@/lib/api"
import { api } from "@/lib/api"
import { errorText, relativeTime } from "@/lib/format"
import { t } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import { ConfirmDialog } from "@/components/confirm"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Switch } from "@/components/ui/switch"

/** A field to fill to connect an account. */
type Field = {
  name: string
  label: string
  secret?: boolean
  placeholder?: string
}

/** What each kind of account is for, and how to get what it asks. */
type Kind = {
  title: string
  icon: LucideIcon
  description: string
  steps: string[]
  link: { url: string; label: string }
  fields: Field[]
  /** Connected from elsewhere (its module). */
  elsewhere?: { to: string; label: string }
  /** What switching it off does. */
  off: string
}

const KINDS: Record<AccountKind, Kind> = {
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
  return c.client_id ? t("Application {id}", { id: c.client_id }) : null
}

function AccountCard({ a }: { a: AccountView }) {
  const qc = useQueryClient()
  const kind = KINDS[a.kind]
  const [editing, setEditing] = useState(!a.connected)
  const [values, setValues] = useState<Record<string, string>>({})
  const [removing, setRemoving] = useState(false)
  const done = (v?: AccountView) => {
    qc.invalidateQueries({ queryKey: ["accounts"] })
    qc.invalidateQueries({ queryKey: ["linky"] })
    if (v)
      qc.setQueryData<AccountView[]>(["accounts"], (list) =>
        list?.map((x) => (x.kind === v.kind ? v : x))
      )
  }
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
  const enabled = enable.isPending ? enable.variables : a.enabled
  const remove = useMutation({
    mutationFn: () => api.deleteAccount(a.kind),
    onSuccess: () => {
      toast.success(t("Compte {name} retiré", { name: kind.title }))
      setEditing(true)
      done()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const filled = kind.fields.every((f) => values[f.name]?.trim())
  const what = summary(a)
  return (
    <Card className="rounded-3xl">
      <CardContent className="grid gap-4">
        <div className="flex flex-wrap items-start gap-3">
          <span className="grid size-10 shrink-0 place-items-center rounded-2xl bg-primary/15 text-primary">
            <kind.icon className="size-5" />
          </span>
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <h3 className="font-heading text-lg font-semibold">
                {kind.title}
              </h3>
              <StateBadge a={a} />
            </div>
            <p className="text-sm text-muted-foreground">{kind.description}</p>
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
        {!enabled && (
          <p className="text-sm text-muted-foreground">{kind.off}</p>
        )}
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
              <a
                href={kind.link.url}
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-1 text-sm font-medium text-primary hover:underline"
              >
                {kind.link.label} <ExternalLink className="size-3.5" />
              </a>
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
            <a
              href={kind.link.url}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-1 text-sm font-medium text-primary hover:underline"
            >
              {kind.link.label} <ExternalLink className="size-3.5" />
            </a>
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
      </CardContent>
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
    </Card>
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
  const [query, setQuery] = useState("")
  const shown = useMemo(() => {
    const words = plain(query).split(/\s+/).filter(Boolean)
    const list = (accounts.data ?? []).filter((a) => {
      const k = KINDS[a.kind]
      const text = plain(`${k.title} ${k.description} ${k.link.label}`)
      return words.every((w) => text.includes(w))
    })
    const rank = (a: AccountView) => (a.connected ? 0 : 1)
    return [...list].sort((x, y) => rank(x) - rank(y))
  }, [accounts.data, query])
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
        <div className="grid gap-4 lg:grid-cols-2">
          {shown.map((a) => (
            <AccountCard key={`${a.kind}-${a.connected}`} a={a} />
          ))}
        </div>
      )}
    </div>
  )
}
