import { useState } from "react"
import { Link } from "react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  CloudSun,
  ExternalLink,
  Gauge,
  House,
  Loader2,
  RefreshCw,
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
  },
  rte: {
    title: "RTE",
    icon: CloudSun,
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
  },
}

function StateBadge({ a }: { a: AccountView }) {
  const [label, tone] = !a.connected
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
        </div>
        {a.connected && (
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
        {kind.elsewhere ? (
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
        {a.kind === "airzone" && a.connected && (
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

/** Settings › Accounts: the online services Breezone may use, always in addition to the local. */
export function AccountsTab() {
  const accounts = useQuery({ queryKey: ["accounts"], queryFn: api.accounts })
  return (
    <div className="grid gap-4">
      <p className="text-sm text-muted-foreground">
        {t(
          "Un compte relie Breezone à un service en ligne, toujours en plus de ce qu'il lit en local : rien n'en dépend pour piloter la maison. Les identifiants restent chiffrés sur ce serveur."
        )}
      </p>
      <div className="grid gap-4 lg:grid-cols-2">
        {(accounts.data ?? []).map((a) => (
          <AccountCard key={`${a.kind}-${a.connected}`} a={a} />
        ))}
      </div>
    </div>
  )
}
