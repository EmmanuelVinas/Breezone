import type { Connection } from "@/lib/api"
import { relativeTime } from "@/lib/format"
import { cn } from "@/lib/utils"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"

const LABEL = {
  online: "Connectée",
  offline: "Hors ligne",
  unknown: "En attente",
} as const

export function ConnectionDot({
  state,
  className,
}: {
  state: Connection["state"]
  className?: string
}) {
  return (
    <span className={cn("relative inline-flex size-2.5", className)}>
      {state === "online" && (
        <span className="absolute inline-flex size-full animate-ping rounded-full bg-success opacity-50" />
      )}
      <span
        className={cn(
          "relative inline-flex size-2.5 rounded-full",
          state === "online"
            ? "bg-success"
            : state === "offline"
              ? "bg-destructive"
              : "bg-muted-foreground"
        )}
      />
    </span>
  )
}

export function ConnectionBadge({ connection }: { connection: Connection }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className="inline-flex items-center gap-2 rounded-full border bg-card/60 px-3 py-1 text-xs font-medium backdrop-blur">
          <ConnectionDot state={connection.state} />
          {LABEL[connection.state]}
          {connection.latency_ms !== null && connection.state === "online" && (
            <span className="tabular text-muted-foreground">
              {connection.latency_ms} ms
            </span>
          )}
        </span>
      </TooltipTrigger>
      <TooltipContent>
        {connection.last_error ??
          `Dernière réponse ${relativeTime(connection.last_seen)}`}
      </TooltipContent>
    </Tooltip>
  )
}
