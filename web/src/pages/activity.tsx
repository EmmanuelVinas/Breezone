import { useState } from "react"
import { useQuery, useQueryClient } from "@tanstack/react-query"
import {
  CalendarClock,
  ChevronDown,
  KeyRound,
  Plug,
  Server,
  SlidersHorizontal,
  TriangleAlert,
} from "lucide-react"
import type { LucideIcon } from "lucide-react"
import type { FeedEvent, Incident } from "@/lib/api"
import { api, currentUser } from "@/lib/api"
import { dateTime, installationAddress, relativeTime } from "@/lib/format"
import { cn } from "@/lib/utils"
import { PageHeader } from "@/components/app-shell"
import { ConnectionDot } from "@/components/connection-badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { t, tn } from "@/lib/i18n"

const KIND: Record<FeedEvent["kind"], { label: string; icon: LucideIcon }> = {
  command: { label: t("Commandes"), icon: SlidersHorizontal },
  connection: { label: t("Connexion"), icon: Plug },
  routine: { label: t("Programmations"), icon: CalendarClock },
  system: { label: t("Système"), icon: Server },
  auth: { label: t("Sécurité"), icon: KeyRound },
}

/** The details of an event (its data) or of a server error (its request and cause), unfolded. */
function Details({ text }: { text: string }) {
  const [open, setOpen] = useState(false)
  return (
    <div className="mt-1">
      <button
        type="button"
        className="inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        <ChevronDown
          className={cn("size-3.5 transition-transform", open && "rotate-180")}
        />
        {open ? t("Masquer les détails") : t("Détails")}
      </button>
      {open && (
        <pre className="mt-2 max-h-64 overflow-auto rounded-xl bg-muted/60 p-3 font-mono text-xs break-all whitespace-pre-wrap">
          {text}
        </pre>
      )}
    </div>
  )
}

function eventDetails(e: FeedEvent) {
  if (!e.data) return null
  try {
    return JSON.stringify(JSON.parse(e.data), null, 2)
  } catch {
    return e.data
  }
}

function incidentDetails(i: Incident) {
  return [
    `${i.method} ${i.path} → HTTP ${i.status}`,
    dateTime(i.ts),
    i.cause ? `\n${i.cause}` : null,
  ]
    .filter(Boolean)
    .join("\n")
}

export function ActivityPage() {
  const qc = useQueryClient()
  const [filter, setFilter] = useState<string>("all")
  const [loadingMore, setLoadingMore] = useState(false)
  const status = useQuery({ queryKey: ["status"], queryFn: api.status })
  const events = useQuery({
    queryKey: ["events"],
    queryFn: () => api.events(150),
    staleTime: Infinity,
  })
  const admin = currentUser()?.role === "admin"
  // The last errors of the server itself (database, internal), for administrators.
  const incidents = useQuery({
    queryKey: ["incidents"],
    queryFn: api.incidents,
    enabled: admin,
    refetchInterval: 30_000,
  })
  const list = (events.data ?? []).filter((e) =>
    filter === "errors"
      ? e.level !== "info"
      : filter === "all" || e.kind === filter
  )
  // Errors: the server's errors and the events in error or warning, the newest first.
  const errors: { ts: number; incident?: Incident; event?: FeedEvent }[] = [
    ...(incidents.data ?? []).map((i) => ({ ts: i.ts, incident: i })),
    ...list.map((e) => ({ ts: e.ts, event: e })),
  ].sort((a, b) => b.ts - a.ts)
  const errorCount =
    (incidents.data?.length ?? 0) +
    (events.data ?? []).filter((e) => e.level === "error").length

  const loadMore = async () => {
    const last = events.data?.[events.data.length - 1]
    if (!last) return
    setLoadingMore(true)
    try {
      const older = await api.events(150, last.id)
      qc.setQueryData<FeedEvent[]>(["events"], (old) => [
        ...(old ?? []),
        ...older,
      ])
    } finally {
      setLoadingMore(false)
    }
  }

  return (
    <>
      <PageHeader
        title={t("Activité")}
        description={t("État des centrales Airzone et journal en temps réel.")}
      />

      <div className="mb-8 grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
        {(status.data ?? []).map((i) => (
          <Card key={i.id} className="rounded-3xl">
            <CardContent className="flex items-start gap-3">
              <ConnectionDot state={i.connection.state} className="mt-1.5" />
              <div className="min-w-0 flex-1">
                <div className="flex items-center justify-between gap-2">
                  <span className="truncate font-semibold">{i.name}</span>
                  {i.connection.latency_ms !== null &&
                    i.connection.state === "online" && (
                      <span className="tabular text-xs text-muted-foreground">
                        {i.connection.latency_ms} ms
                      </span>
                    )}
                </div>
                <div className="text-xs text-muted-foreground">
                  {installationAddress(i)} ·{" "}
                  {tn(i.zones.length, "{n} zone", "{n} zones")} ·{" "}
                  {t("vue {when}", {
                    when: relativeTime(i.connection.last_seen),
                  })}
                </div>
                {i.connection.last_error && i.connection.state !== "online" && (
                  <div className="mt-2 line-clamp-2 text-xs text-destructive">
                    {i.connection.last_error}
                  </div>
                )}
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      <div className="mb-4 overflow-x-auto">
        <ToggleGroup
          type="single"
          variant="outline"
          size="sm"
          value={filter}
          onValueChange={(v) => v && setFilter(v)}
          className="justify-start"
        >
          <ToggleGroupItem value="all">{t("Tout")}</ToggleGroupItem>
          <ToggleGroupItem value="errors">
            {t("Erreurs")}
            {errorCount > 0 && (
              <span className="tabular rounded-full bg-destructive/15 px-1.5 text-[11px] text-destructive">
                {errorCount}
              </span>
            )}
          </ToggleGroupItem>
          {(Object.keys(KIND) as FeedEvent["kind"][]).map((k) => (
            <ToggleGroupItem key={k} value={k}>
              {KIND[k].label}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </div>

      <Card className="rounded-3xl">
        <CardContent className="p-0">
          {events.isLoading ? (
            <div className="space-y-3 p-6">
              {[0, 1, 2, 3].map((i) => (
                <Skeleton key={i} className="h-10" />
              ))}
            </div>
          ) : filter === "errors" ? (
            errors.length === 0 ? (
              <p className="p-10 text-center text-sm text-muted-foreground">
                {t("Aucune erreur récente.")}
              </p>
            ) : (
              <ul className="divide-y">
                {errors.map((x, n) =>
                  x.incident ? (
                    <li
                      key={`i${n}`}
                      className="flex items-start gap-3 px-5 py-3.5"
                    >
                      <span className="mt-0.5 inline-flex size-8 shrink-0 items-center justify-center rounded-xl bg-destructive/15 text-destructive">
                        <TriangleAlert className="size-4" />
                      </span>
                      <div className="min-w-0 flex-1">
                        <p className="text-sm">
                          {t("Erreur du serveur : {message}", {
                            message: x.incident.message,
                          })}
                        </p>
                        <p className="font-mono text-xs text-muted-foreground">
                          {x.incident.method} {x.incident.path}
                        </p>
                        <Details text={incidentDetails(x.incident)} />
                      </div>
                      <span className="shrink-0 text-xs whitespace-nowrap text-muted-foreground">
                        {relativeTime(x.incident.ts)}
                      </span>
                    </li>
                  ) : (
                    <EventRow
                      key={x.event!.id}
                      e={x.event!}
                      name={
                        status.data?.find(
                          (i) => i.id === x.event!.installation_id
                        )?.name
                      }
                    />
                  )
                )}
              </ul>
            )
          ) : list.length === 0 ? (
            <p className="p-10 text-center text-sm text-muted-foreground">
              {t("Rien pour le moment.")}
            </p>
          ) : (
            <ul className="divide-y">
              {list.map((e) => (
                <EventRow
                  key={e.id}
                  e={e}
                  name={
                    status.data?.find((i) => i.id === e.installation_id)?.name
                  }
                />
              ))}
            </ul>
          )}
        </CardContent>
      </Card>
      {(events.data?.length ?? 0) >= 150 && (
        <div className="mt-4 flex justify-center">
          <Button variant="outline" onClick={loadMore} disabled={loadingMore}>
            {t("Charger plus")}
          </Button>
        </div>
      )}
    </>
  )
}

function EventRow({ e, name }: { e: FeedEvent; name?: string }) {
  const { icon: Icon } = KIND[e.kind] ?? KIND.system
  const details = eventDetails(e)
  return (
    <li className="flex items-start gap-3 px-5 py-3.5">
      <span
        className={cn(
          "mt-0.5 inline-flex size-8 shrink-0 items-center justify-center rounded-xl bg-muted",
          e.level === "error" && "bg-destructive/15 text-destructive",
          e.level === "warn" && "bg-warning/15 text-warning"
        )}
      >
        <Icon className="size-4" />
      </span>
      <div className="min-w-0 flex-1">
        <p className="text-sm">{e.message}</p>
        <p className="text-xs text-muted-foreground">
          {[e.actor, name].filter(Boolean).join(" · ") || "Breezone"}
        </p>
        {details && <Details text={details} />}
      </div>
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="shrink-0 text-xs whitespace-nowrap text-muted-foreground">
            {relativeTime(e.ts)}
          </span>
        </TooltipTrigger>
        <TooltipContent>{dateTime(e.ts)}</TooltipContent>
      </Tooltip>
    </li>
  )
}
