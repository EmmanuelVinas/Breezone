import { useEffect, useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  CircleCheck,
  Download,
  ExternalLink,
  Loader2,
  RefreshCw,
  TriangleAlert,
} from "lucide-react"
import Markdown from "react-markdown"
import { toast } from "sonner"
import type { UpdateView } from "@/lib/api"
import { api } from "@/lib/api"
import { relativeTime } from "@/lib/format"
import { t } from "@/lib/i18n"
import { ConfirmDialog } from "@/components/confirm"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Label } from "@/components/ui/label"
import { Skeleton } from "@/components/ui/skeleton"
import { Switch } from "@/components/ui/switch"

const isPrerelease = (version: string) => version.includes("-")

function BetaBadge() {
  return (
    <Badge variant="outline" className="border-warning/50 text-warning">
      {t("bêta")}
    </Badge>
  )
}

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e))

const REASON: Record<NonNullable<UpdateView["reason"]>, string> = {
  disabled: t(
    "Les mises à jour depuis l'interface sont désactivées sur ce serveur (BREEZONE_SELF_UPDATE=false)."
  ),
  platform: t(
    "Breezone ne publie pas de version pour ce système : mettez à jour l'image Docker."
  ),
  development: t(
    "Ce serveur est une version de développement : mettez-le à jour depuis les sources."
  ),
  assets: t(
    "Les fichiers de cette version ne sont pas encore publiés : réessayez dans quelques minutes."
  ),
  failed: t(
    "Cette version n'a pas démarré sur ce serveur : elle n'est plus proposée. Mettez à jour l'image Docker, ou attendez la suivante."
  ),
}

/** While installing: the server stops, then comes back with its new version. */
function useWaitForVersion(expected: string | null, active: boolean) {
  const [timedOut, setTimedOut] = useState(false)
  useEffect(() => {
    if (!active || !expected) return
    let cancelled = false
    const started = Date.now()
    const tick = async () => {
      if (cancelled) return
      try {
        const v = await api.version()
        if (v.version === expected) {
          toast.success(
            t("Breezone {version} est installé", { version: expected })
          )
          // The web UI comes with the server: load the new one.
          window.location.reload()
          return
        }
      } catch {
        // Restarting: not answering for a few seconds.
      }
      if (Date.now() - started > 120_000) setTimedOut(true)
      else setTimeout(tick, 2000)
    }
    const timer = setTimeout(tick, 2000)
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [expected, active])
  return timedOut
}

/** The version of Breezone, the latest release, and the update from here (administrators). */
export function UpdateTab() {
  const qc = useQueryClient()
  const [confirm, setConfirm] = useState(false)
  // Version being installed: waited for until the server answers with it, whatever the state the
  // restarted server reports (it starts again "idle").
  const [target, setTarget] = useState<string | null>(null)
  const status = useQuery({
    queryKey: ["system-update"],
    queryFn: api.update,
    // Following an update in progress; otherwise the server checks once a day.
    refetchInterval: (q) => {
      const state = q.state.data?.state
      return state && state !== "idle" && state !== "failed" ? 1000 : false
    },
  })
  const check = useMutation({
    mutationFn: api.checkUpdate,
    onSuccess: (v) => {
      qc.setQueryData(["system-update"], v)
      if (!v.available) toast.success(t("Breezone est à jour"))
    },
    onError: (e) => toast.error(errorText(e)),
  })
  // Changing channel forgets the release found on the other one: look again at once.
  const channel = useMutation({
    mutationFn: api.setUpdateChannel,
    onSuccess: (v) => {
      qc.setQueryData(["system-update"], v)
      check.mutate()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const install = useMutation({
    mutationFn: api.installUpdate,
    onSuccess: (v) => qc.setQueryData(["system-update"], v),
    onSettled: (_, error) => error && setTarget(null),
    onError: (e) => toast.error(errorText(e)),
  })
  const v = status.data
  const failed = v?.state === "failed"
  const restarting =
    v?.state === "restarting" ||
    (target !== null && !failed && v?.state === "idle")
  const timedOut = useWaitForVersion(target, target !== null && !failed)

  if (status.isLoading || !v) return <Skeleton className="h-48 rounded-3xl" />
  const busy =
    restarting ||
    ["checking", "downloading", "installing", "restarting"].includes(v.state)

  return (
    <div className="grid gap-6">
      {v.rolled_back && (
        <Alert variant="destructive" className="rounded-2xl">
          <TriangleAlert />
          <AlertTitle>
            {t("La mise à jour vers Breezone {version} a été annulée", {
              version: v.rolled_back.version,
            })}
          </AlertTitle>
          <AlertDescription>
            {t(
              "Cette version n'a pas démarré : Breezone est revenu à la version {to}, avec la base de données d'avant la mise à jour.",
              { to: v.rolled_back.to }
            )}
          </AlertDescription>
        </Alert>
      )}

      <Card className="rounded-3xl">
        <CardHeader>
          <CardTitle>{t("Version de Breezone")}</CardTitle>
          <CardDescription>
            {v.checked_at
              ? t("Dernière recherche de mise à jour : {when}", {
                  when: relativeTime(v.checked_at),
                })
              : t(
                  "Breezone cherche une nouvelle version une fois par jour, sur son dépôt officiel."
                )}
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap items-center gap-x-6 gap-y-2">
          <div>
            <div className="text-xs text-muted-foreground">
              {t("Version installée")}
            </div>
            <div className="flex items-center gap-2">
              <span className="tabular text-2xl font-semibold">
                {v.current}
              </span>
              {isPrerelease(v.current) && <BetaBadge />}
            </div>
          </div>
          {v.latest && (
            <div>
              <div className="text-xs text-muted-foreground">
                {t("Dernière version publiée")}
              </div>
              <div className="flex items-center gap-2">
                <span className="tabular text-2xl font-semibold">
                  {v.latest}
                </span>
                {v.prerelease && <BetaBadge />}
              </div>
            </div>
          )}
          {v.checked_at && !v.available && (
            <span className="inline-flex items-center gap-1.5 text-sm text-success">
              <CircleCheck className="size-4" /> {t("Breezone est à jour")}
            </span>
          )}
        </CardContent>
        <CardContent className="border-t pt-4">
          <div className="flex items-start justify-between gap-4">
            <div className="space-y-1">
              <Label htmlFor="beta-channel">
                {t("Recevoir les versions bêta")}
              </Label>
              <p className="text-sm text-muted-foreground">
                {t(
                  "Les nouveautés avant tout le monde, avec un risque de défauts. Si une bêta ne démarre pas, Breezone revient seul à la version d'avant."
                )}
              </p>
              {!v.beta && isPrerelease(v.current) && (
                <p className="text-sm text-muted-foreground">
                  {t(
                    "Ce serveur reste sur cette bêta jusqu'à la prochaine version stable : revenir en arrière n'est pas possible, la base de données a pu évoluer."
                  )}
                </p>
              )}
            </div>
            <Switch
              id="beta-channel"
              checked={v.beta}
              disabled={channel.isPending || busy}
              onCheckedChange={(beta) => channel.mutate(beta)}
            />
          </div>
        </CardContent>
        <CardFooter>
          <Button
            variant="outline"
            onClick={() => check.mutate()}
            disabled={check.isPending || busy}
          >
            {check.isPending ? (
              <Loader2 className="animate-spin" />
            ) : (
              <RefreshCw />
            )}
            {t("Rechercher une mise à jour")}
          </Button>
        </CardFooter>
      </Card>

      {v.available && v.latest && (
        <Card className="rounded-3xl">
          <CardHeader>
            <CardTitle>
              {t("Breezone {version} est disponible", { version: v.latest })}
            </CardTitle>
            <CardDescription>
              {t(
                "Le serveur et l'interface web sont mis à jour ensemble. Les applications mobiles se mettent à jour par leur magasin."
              )}
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {v.notes && (
              <div className="max-h-72 space-y-2 overflow-auto rounded-2xl bg-muted/60 p-4 text-sm [&_a]:text-primary [&_a]:underline [&_h3]:font-semibold [&_li]:ml-4 [&_li]:list-disc [&_ul]:space-y-1">
                {/* Release notes from GitHub: Markdown, no raw HTML. */}
                <Markdown
                  components={{
                    a: (props) => (
                      <a {...props} target="_blank" rel="noreferrer" />
                    ),
                  }}
                >
                  {v.notes}
                </Markdown>
              </div>
            )}
            {v.state === "downloading" && (
              <div className="space-y-1.5">
                <div className="text-sm">{t("Téléchargement…")}</div>
                <div className="h-2 overflow-hidden rounded-full bg-muted">
                  <div
                    className="h-full bg-primary transition-all"
                    style={{ width: `${Math.round((v.progress ?? 0) * 100)}%` }}
                  />
                </div>
              </div>
            )}
            {v.state === "installing" && (
              <p className="flex items-center gap-2 text-sm">
                <Loader2 className="size-4 animate-spin" />
                {t(
                  "Vérification de la signature, installation et sauvegarde de la base…"
                )}
              </p>
            )}
            {restarting && (
              <p className="flex items-center gap-2 text-sm">
                <Loader2 className="size-4 animate-spin" />
                {timedOut
                  ? t(
                      "Le serveur ne répond pas encore. S'il ne revient pas, redémarrez le conteneur : la version précédente reprendra automatiquement."
                    )
                  : t(
                      "Redémarrage sur la nouvelle version : la page se rechargera d'elle-même."
                    )}
              </p>
            )}
            {v.state === "failed" && v.error && (
              <Alert variant="destructive" className="rounded-2xl">
                <TriangleAlert />
                <AlertTitle>{t("La mise à jour a échoué")}</AlertTitle>
                <AlertDescription>
                  {v.error}. {t("Breezone n'a pas changé de version.")}
                </AlertDescription>
              </Alert>
            )}
            {!v.can_install && v.reason && (
              <Alert className="rounded-2xl">
                <AlertDescription>{REASON[v.reason]}</AlertDescription>
              </Alert>
            )}
            <p className="text-xs text-muted-foreground">
              {t(
                "Avec Docker, vous pouvez aussi mettre à jour l'image : « docker compose pull && docker compose up -d », ou « Mettre à jour » dans Container Manager sur Synology."
              )}
            </p>
          </CardContent>
          <CardFooter className="flex flex-wrap gap-2">
            {v.can_install && (
              <Button onClick={() => setConfirm(true)} disabled={busy}>
                {busy ? <Loader2 className="animate-spin" /> : <Download />}
                {t("Mettre à jour vers {version}", { version: v.latest })}
              </Button>
            )}
            {v.url && (
              <Button variant="ghost" asChild>
                <a href={v.url} target="_blank" rel="noreferrer">
                  <ExternalLink /> {t("Notes de version")}
                </a>
              </Button>
            )}
          </CardFooter>
        </Card>
      )}

      <ConfirmDialog
        open={confirm}
        title={t("Mettre à jour vers Breezone {version} ?", {
          version: v.latest ?? "",
        })}
        description={
          t(
            "Breezone télécharge cette version depuis son dépôt officiel, vérifie sa signature, sauvegarde la base de données puis redémarre : quelques secondes d'interruption. Si la nouvelle version ne démarre pas, Breezone revient seul à la version actuelle."
          ) +
          (v.prerelease
            ? " " +
              t(
                "C'est une version bêta : une fois installée, ce serveur ne pourra revenir qu'à une version plus récente."
              )
            : "")
        }
        onConfirm={() => {
          setConfirm(false)
          setTarget(v.latest)
          install.mutate()
        }}
        onCancel={() => setConfirm(false)}
      />
    </div>
  )
}
