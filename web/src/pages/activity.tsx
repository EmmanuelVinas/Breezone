import { useState } from "react"
import { useQuery, useQueryClient } from "@tanstack/react-query"
import {
  CalendarClock,
  KeyRound,
  Plug,
  Server,
  SlidersHorizontal,
} from "lucide-react"
import type { LucideIcon } from "lucide-react"
import type { FeedEvent } from "@/lib/api"
import { api } from "@/lib/api"
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

const KIND: Record<FeedEvent["kind"], { label: string; icon: LucideIcon }> = {
  command: { label: "Commandes", icon: SlidersHorizontal },
  connection: { label: "Connexion", icon: Plug },
  routine: { label: "Programmations", icon: CalendarClock },
  system: { label: "Système", icon: Server },
  auth: { label: "Sécurité", icon: KeyRound },
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
  const list = (events.data ?? []).filter(
    (e) => filter === "all" || e.kind === filter
  )

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
        title="Activité"
        description="État des centrales Airzone et journal en temps réel."
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
                  {installationAddress(i)} · {i.zones.length} zones · vue{" "}
                  {relativeTime(i.connection.last_seen)}
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
          <ToggleGroupItem value="all">Tout</ToggleGroupItem>
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
          ) : list.length === 0 ? (
            <p className="p-10 text-center text-sm text-muted-foreground">
              Rien pour le moment.
            </p>
          ) : (
            <ul className="divide-y">
              {list.map((e) => {
                const { icon: Icon } = KIND[e.kind] ?? KIND.system
                const name = status.data?.find(
                  (i) => i.id === e.installation_id
                )?.name
                return (
                  <li key={e.id} className="flex items-start gap-3 px-5 py-3.5">
                    <span
                      className={cn(
                        "mt-0.5 inline-flex size-8 shrink-0 items-center justify-center rounded-xl bg-muted",
                        e.level === "error" &&
                          "bg-destructive/15 text-destructive",
                        e.level === "warn" && "bg-warning/15 text-warning"
                      )}
                    >
                      <Icon className="size-4" />
                    </span>
                    <div className="min-w-0 flex-1">
                      <p className="text-sm">{e.message}</p>
                      <p className="text-xs text-muted-foreground">
                        {[e.actor, name].filter(Boolean).join(" · ") ||
                          "Breezone"}
                      </p>
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
              })}
            </ul>
          )}
        </CardContent>
      </Card>
      {(events.data?.length ?? 0) >= 150 && (
        <div className="mt-4 flex justify-center">
          <Button variant="outline" onClick={loadMore} disabled={loadingMore}>
            Charger plus
          </Button>
        </div>
      )}
    </>
  )
}
