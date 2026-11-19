import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  Bell,
  Copy,
  History,
  Pencil,
  Play,
  Trash2,
  Workflow,
} from "lucide-react"
import { toast } from "sonner"
import type { Automation, AutomationRun } from "@/lib/api"
import { api, currentUser } from "@/lib/api"
import { relativeTime, dateTime } from "@/lib/format"
import { t } from "@/lib/i18n"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Skeleton } from "@/components/ui/skeleton"
import { Switch } from "@/components/ui/switch"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog"
import { ELSE_TONE, IF_TONE, THEN_TONE, WHEN_TONE } from "./steps"

export function Line({
  badge,
  tone,
  children,
}: {
  badge: string
  tone: string
  children: React.ReactNode
}) {
  return (
    <div className="flex items-start gap-2 text-sm">
      <Badge variant="outline" className={`mt-0.5 ${tone}`}>
        {badge}
      </Badge>
      <div className="min-w-0">{children}</div>
    </div>
  )
}

export const BRANCH: Record<AutomationRun["branch"], string> = {
  then: t("Exécuté"),
  else: t("Sinon"),
  skipped: t("Pas déclenché"),
  busy: t("Déjà en cours"),
}

export const ENDED: Record<NonNullable<AutomationRun["ended"]>, string> = {
  done: "",
  stopped: t("arrêté : attente écoulée"),
  cancelled: t("recommencé"),
  interrupted: t("interrompu par un arrêt du serveur"),
}

/** The last runs of a rule, those that did not happen included. */
export function RunsPanel({ id }: { id: number }) {
  const runs = useQuery({
    queryKey: ["automation-runs", id],
    queryFn: () => api.automationRuns(id),
    refetchInterval: 10_000,
  })
  if (runs.isLoading) return <Skeleton className="h-16 rounded-2xl" />
  if (!runs.data?.length)
    return (
      <p className="text-sm text-muted-foreground">
        {t("Rien encore : l'historique garde les 20 derniers passages.")}
      </p>
    )
  return (
    <ul className="grid gap-2">
      {runs.data.map((r, i) => (
        <li key={i} className="rounded-2xl border bg-muted/30 p-3 text-sm">
          <div className="flex flex-wrap items-center gap-2">
            <Badge
              variant={
                r.branch === "then" || r.branch === "else"
                  ? "secondary"
                  : "outline"
              }
            >
              {BRANCH[r.branch]}
            </Badge>
            <span className="text-xs text-muted-foreground">
              {dateTime(r.started_at)}
            </span>
            {r.ended === null && (
              <Badge variant="outline" className={THEN_TONE}>
                {t("En cours")}
              </Badge>
            )}
            {r.ended && ENDED[r.ended] && (
              <span className="text-xs text-muted-foreground">
                {ENDED[r.ended]}
              </span>
            )}
          </div>
          <p className="mt-1">{r.reason}</p>
          {r.steps.length > 0 && (
            <ul className="mt-1 grid gap-0.5 text-xs text-muted-foreground">
              {r.steps.map((s, j) => (
                <li key={j} className={s.ok ? "" : "text-destructive"}>
                  {s.ok ? "✓" : "✗"} {s.what}
                  {s.error ? ` — ${s.error}` : ""}
                </li>
              ))}
            </ul>
          )}
        </li>
      ))}
    </ul>
  )
}

export function AutomationCard({
  automation: a,
  onEdit,
  onDuplicate,
}: {
  automation: Automation
  /** Absent when the user may not change this rule. */
  onEdit?: () => void
  onDuplicate?: () => void
}) {
  const qc = useQueryClient()
  const [history, setHistory] = useState(false)
  const refresh = () => {
    qc.invalidateQueries({ queryKey: ["automations"] })
    qc.invalidateQueries({ queryKey: ["automation-runs", a.id] })
  }
  const onError = (e: unknown) =>
    toast.error(e instanceof Error ? e.message : String(e))
  const enable = useMutation({
    mutationFn: (enabled: boolean) => api.enableAutomation(a.id, enabled),
    onSuccess: refresh,
    onError,
  })
  const run = useMutation({
    mutationFn: () => api.runAutomation(a.id),
    onSuccess: (r) => {
      const failed = r.results.filter((x) => !x.ok).length
      if (r.running)
        toast.success(
          t("« {name} » a démarré ; elle continue en attendant.", {
            name: a.name,
          })
        )
      else if (failed)
        toast.warning(
          t("« {name} » : {failed} action(s) en échec", {
            name: a.name,
            failed,
          })
        )
      else toast.success(t("« {name} » exécutée", { name: a.name }))
      refresh()
    },
    onError,
  })
  const remove = useMutation({
    mutationFn: () => api.deleteAutomation(a.id),
    onSuccess: refresh,
    onError,
  })
  const now =
    a.status.value === null
      ? null
      : `${Math.round(a.status.value * 10) / 10}${a.status.unit ? ` ${a.status.unit}` : ""}`

  return (
    <Card className="rounded-3xl">
      <CardContent className="flex flex-col gap-3">
        <div className="flex items-start justify-between gap-3">
          <div className="flex min-w-0 items-center gap-2">
            <Workflow className="size-4 shrink-0 text-fan" />
            <h3 className="truncate font-semibold">{a.name}</h3>
            {a.status.running && (
              <Badge variant="outline" className={THEN_TONE}>
                {t("En cours")}
              </Badge>
            )}
          </div>
          <Switch
            checked={a.enabled}
            disabled={!onEdit}
            loading={enable.isPending}
            onCheckedChange={(v) => enable.mutate(v)}
            aria-label={t("Activer « {name} »", { name: a.name })}
          />
        </div>
        <div className={`grid gap-1.5 ${a.enabled ? "" : "opacity-60"}`}>
          <Line badge={t("Quand")} tone={WHEN_TONE}>
            {(a.summary.triggers ?? [a.summary.when]).map((w, i) => (
              <div key={i}>
                {i > 0 && (
                  <span className="text-muted-foreground">{t("ou")} </span>
                )}
                {w}
              </div>
            ))}
            {now && (
              <span className="text-muted-foreground">
                {t("maintenant : {value}", { value: now })}
              </span>
            )}
          </Line>
          {a.summary.conditions.length > 0 && (
            <Line badge={t("Si")} tone={IF_TONE}>
              {a.summary.conditions.map((c, i) => (
                <div
                  key={i}
                  className={
                    a.status.conditions[i] ? "" : "text-muted-foreground"
                  }
                >
                  {i > 0 && (
                    <span className="text-muted-foreground">
                      {a.summary.joined}{" "}
                    </span>
                  )}
                  {c}
                  {a.status.conditions[i] === false &&
                    ` · ${t("pas maintenant")}`}
                  {a.status.conditions[i] === null && ` · ${t("inconnu")}`}
                </div>
              ))}
            </Line>
          )}
          <Line badge={t("Alors")} tone={THEN_TONE}>
            {a.summary.actions.map((s, i) => (
              <div key={i}>{s}</div>
            ))}
          </Line>
          {(a.conflicts ?? []).length > 0 && (
            <p className="rounded-xl border border-warning/40 bg-warning/10 px-3 py-2 text-xs">
              {t(
                "Peut contredire {names} : {what}, au même moment. Vérifiez qu'elles ne se battent pas.",
                {
                  names: a.conflicts
                    .map((c) => t("« {name} »", { name: c.name }))
                    .join(", "),
                  what: a.conflicts[0].what,
                }
              )}
            </p>
          )}
          {(a.surplus ?? []).length > 0 && (
            <p className="rounded-xl border border-warning/40 bg-warning/10 px-3 py-2 text-xs">
              {t(
                "{names} suit aussi le pilotage sur surplus solaire : les deux se contrediraient. Retirez-le de l'un ou de l'autre.",
                { names: a.surplus!.join(", ") }
              )}
            </p>
          )}
          {(a.summary.over ?? []).length > 0 && (
            <Line badge={t("Ensuite")} tone={THEN_TONE}>
              <div className="text-muted-foreground">
                {t("quand ce n'est plus vrai :")}
              </div>
              {a.summary.over!.map((s, i) => (
                <div key={i}>{s}</div>
              ))}
            </Line>
          )}
          {(a.summary.else ?? []).length > 0 && (
            <Line badge={t("Sinon")} tone={ELSE_TONE}>
              {a.summary.else.map((s, i) => (
                <div key={i}>{s}</div>
              ))}
            </Line>
          )}
        </div>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="grid gap-0.5 text-xs text-muted-foreground">
            {a.last_run_at ? (
              <span>
                {t("Dernier : {when}", { when: relativeTime(a.last_run_at) })}
                {a.last_reason ? ` — ${a.last_reason}` : ""}
              </span>
            ) : (
              <span>{t("Jamais déclenché")}</span>
            )}
            {a.enabled && a.status.next_at && (
              <span>
                {t("Prochain : {when}", { when: dateTime(a.status.next_at) })}
              </span>
            )}
          </div>
          <div className="flex flex-wrap gap-1">
            <Button
              size="sm"
              variant="ghost"
              onClick={() => run.mutate()}
              disabled={run.isPending}
            >
              <Play /> {t("Essayer")}
            </Button>
            <Button
              size="icon"
              variant={history ? "secondary" : "ghost"}
              onClick={() => setHistory(!history)}
              aria-label={t("Historique")}
              aria-pressed={history}
            >
              <History />
            </Button>
            {onDuplicate && (
              <Button
                size="icon"
                variant="ghost"
                onClick={onDuplicate}
                aria-label={t("Dupliquer")}
              >
                <Copy />
              </Button>
            )}
            {onEdit && (
              <Button
                size="icon"
                variant="ghost"
                onClick={onEdit}
                aria-label={t("Modifier")}
              >
                <Pencil />
              </Button>
            )}
            {onEdit && (
              <AlertDialog>
                <AlertDialogTrigger asChild>
                  <Button
                    size="icon"
                    variant="ghost"
                    aria-label={t("Supprimer")}
                  >
                    <Trash2 />
                  </Button>
                </AlertDialogTrigger>
                <AlertDialogContent>
                  <AlertDialogHeader>
                    <AlertDialogTitle>
                      {t("Supprimer « {name} » ?", { name: a.name })}
                    </AlertDialogTitle>
                    <AlertDialogDescription>
                      {t("Cet automatisme ne se déclenchera plus.")}
                    </AlertDialogDescription>
                  </AlertDialogHeader>
                  <AlertDialogFooter>
                    <AlertDialogCancel>{t("Annuler")}</AlertDialogCancel>
                    <AlertDialogAction onClick={() => remove.mutate()}>
                      {t("Supprimer")}
                    </AlertDialogAction>
                  </AlertDialogFooter>
                </AlertDialogContent>
              </AlertDialog>
            )}
          </div>
        </div>
        {history && <RunsPanel id={a.id} />}
      </CardContent>
    </Card>
  )
}

/** Where the notifications go: an ntfy topic (administrators). */
export function NotificationsDialog({
  current,
  onClose,
}: {
  current: string
  onClose: () => void
}) {
  const qc = useQueryClient()
  const [url, setUrl] = useState(current)
  const onError = (e: unknown) =>
    toast.error(e instanceof Error ? e.message : String(e))
  const save = useMutation({
    mutationFn: () =>
      api.saveAutomationSettings({ ntfy_url: url.trim() || null }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["automation-settings"] })
      toast.success(t("Enregistré"))
    },
    onError,
  })
  const test = useMutation({
    mutationFn: async () => {
      await api.saveAutomationSettings({ ntfy_url: url.trim() || null })
      await api.notifyTest()
    },
    onSuccess: () => toast.success(t("Notification de test envoyée")),
    onError,
  })
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("Notifications")}</DialogTitle>
          <DialogDescription>
            {t(
              "L'action « Prévenir » écrit dans Activité et s'affiche dans Breezone ouvert. Pour la recevoir aussi sur votre téléphone, installez l'application ntfy (gratuite, sans compte) et abonnez-vous à un sujet que vous seul connaissez ; collez son adresse ici."
            )}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-2">
          <Label htmlFor="ntfy">{t("Adresse du sujet ntfy")}</Label>
          <Input
            id="ntfy"
            value={url}
            placeholder="https://ntfy.sh/breezone-maison-7f3k2"
            onChange={(e) => setUrl(e.target.value)}
          />
          <p className="text-xs text-muted-foreground">
            {t(
              "Un sujet sur ntfy.sh est public pour qui connaît son nom : choisissez un nom long et difficile à deviner, ou votre propre serveur ntfy."
            )}
          </p>
        </div>
        <DialogFooter className="gap-2">
          <Button
            variant="outline"
            onClick={() => test.mutate()}
            disabled={test.isPending}
          >
            {t("Envoyer un test")}
          </Button>
          <Button onClick={() => save.mutate()} disabled={save.isPending}>
            {t("Enregistrer")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** "I'm home", the holiday pause, the notifications. */
export function AutomationsBar({ admin }: { admin: boolean }) {
  const qc = useQueryClient()
  const settings = useQuery({
    queryKey: ["automation-settings"],
    queryFn: api.automationSettings,
  })
  const presence = useQuery({ queryKey: ["presence"], queryFn: api.presence })
  const [notifications, setNotifications] = useState(false)
  const onError = (e: unknown) =>
    toast.error(e instanceof Error ? e.message : String(e))
  const pause = useMutation({
    mutationFn: (paused: boolean) =>
      api.saveAutomationSettings({ paused_until: paused ? 0 : null }),
    onSuccess: () =>
      qc.invalidateQueries({ queryKey: ["automation-settings"] }),
    onError,
  })
  const home = useMutation({
    mutationFn: api.setPresence,
    onSuccess: (p) => qc.setQueryData(["presence"], p),
    onError,
  })
  const paused = settings.data?.paused_until != null
  const others = (presence.data?.people ?? []).filter(
    (p) => p.username !== currentUser()?.username
  )
  return (
    <div className="flex flex-wrap items-center gap-x-6 gap-y-3 rounded-2xl border bg-muted/30 px-4 py-3 text-sm">
      <label className="flex items-center gap-2">
        <Switch
          checked={presence.data?.me ?? false}
          loading={home.isPending}
          onCheckedChange={(v) => home.mutate(v)}
        />
        {t("Je suis à la maison")}
      </label>
      {others.length > 0 && (
        <span className="text-xs text-muted-foreground">
          {others
            .map((p) =>
              p.home
                ? t("{name} : à la maison", { name: p.username })
                : t("{name} : absent", { name: p.username })
            )
            .join(" · ")}
        </span>
      )}
      {admin && (
        <label className="flex items-center gap-2">
          <Switch
            checked={paused}
            loading={pause.isPending}
            onCheckedChange={(v) => pause.mutate(v)}
          />
          {t("Pause vacances")}
        </label>
      )}
      {paused && (
        <Badge variant="outline" className="border-warning/40 text-warning">
          {t("Aucun automatisme ne se déclenche")}
        </Badge>
      )}
      {admin && (
        <Button
          size="sm"
          variant="ghost"
          className="ml-auto"
          onClick={() => setNotifications(true)}
        >
          <Bell /> {t("Notifications")}
        </Button>
      )}
      {notifications && (
        <NotificationsDialog
          current={settings.data?.ntfy_url ?? ""}
          onClose={() => setNotifications(false)}
        />
      )}
    </div>
  )
}

/** Rules "quand… si… alors… sinon…" run by the server. */
/** The rules a newer version wrote (then the server went back): they do not run here, and are
 * named so that they do not vanish without a word. */
export function UnreadableAutomations() {
  const qc = useQueryClient()
  const unreadable = useQuery({
    queryKey: ["automations-unreadable"],
    queryFn: api.unreadableAutomations,
  })
  const remove = useMutation({
    mutationFn: api.deleteAutomation,
    onSuccess: () =>
      qc.invalidateQueries({ queryKey: ["automations-unreadable"] }),
    onError: (e) => toast.error(e instanceof Error ? e.message : String(e)),
  })
  const list = unreadable.data ?? []
  if (!list.length) return null
  return (
    <Card className="rounded-3xl border-warning/40">
      <CardContent className="grid gap-3">
        <p className="text-sm">
          {t(
            "Ces automatismes viennent d'une version plus récente de Breezone : ils ne s'exécutent pas sur ce serveur. Mettez-le à jour pour les retrouver, ou supprimez-les."
          )}
        </p>
        <ul className="grid gap-2">
          {list.map((a) => (
            <li key={a.id} className="flex items-center justify-between gap-2">
              <span className="truncate font-medium">{a.name}</span>
              <Button
                size="sm"
                variant="ghost"
                disabled={remove.isPending}
                onClick={() => remove.mutate(a.id)}
              >
                <Trash2 /> {t("Supprimer")}
              </Button>
            </li>
          ))}
        </ul>
      </CardContent>
    </Card>
  )
}
