import { AlertCircle, Check, Loader2 } from "lucide-react"
import { useCommandStatus } from "@/lib/command-status"
import { cn } from "@/lib/utils"

/** Small acknowledgement next to a zone's controls: sending… → applied ✓ (or refused). */
export function CommandIndicator({
  zone,
  className,
}: {
  zone: { installation_id: number; system_id: number; zone_id: number }
  className?: string
}) {
  const status = useCommandStatus(zone)
  if (!status) return null
  const label =
    status === "pending"
      ? "Envoi en cours"
      : status === "done"
        ? "Appliqué"
        : "Refusé"
  return (
    <span
      role="status"
      aria-label={label}
      title={label}
      className={cn(
        "inline-flex size-5 items-center justify-center",
        className
      )}
    >
      {status === "pending" && (
        <Loader2 className="size-4 animate-spin text-muted-foreground" />
      )}
      {status === "done" && (
        <Check className="size-4 animate-in text-success duration-200 zoom-in-50 fade-in" />
      )}
      {status === "error" && (
        <AlertCircle className="size-4 text-destructive" />
      )}
    </span>
  )
}
