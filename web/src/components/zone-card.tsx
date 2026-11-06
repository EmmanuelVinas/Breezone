import { useEffect, useRef, useState } from "react"
import { Link } from "react-router"
import {
  BatteryWarning,
  ChevronRight,
  Droplet,
  Minus,
  Plus,
  TriangleAlert,
} from "lucide-react"
import type { Zone } from "@/lib/api"
import {
  MODE_ICON,
  MODE_LABEL,
  clampSetpoint,
  formatTemp,
  zoneTone,
} from "@/lib/format"
import { useZoneControl } from "@/hooks/use-zone-control"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"

/** Debounces quick +/- taps into a single setpoint command. */
export function useSetpointDraft(zone: Zone) {
  const control = useZoneControl()
  const [draft, setDraft] = useState<number | null>(null)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  useEffect(() => () => clearTimeout(timer.current), [])
  const value = draft ?? zone.setpoint ?? 20
  const commit = (v: number) => {
    clearTimeout(timer.current)
    setDraft(null)
    if (v !== zone.setpoint) control.mutate({ zone, cmd: { setpoint: v } })
  }
  const nudge = (delta: number) => {
    const next = clampSetpoint(zone, value + delta)
    setDraft(next)
    clearTimeout(timer.current)
    timer.current = setTimeout(() => commit(next), 700)
  }
  return { value, setDraft, commit, nudge, pending: draft !== null }
}

export function ZoneCard({ zone }: { zone: Zone }) {
  const control = useZoneControl()
  const { value, nudge, pending } = useSetpointDraft(zone)
  const tone = zoneTone(zone)
  const ModeIcon = zone.mode ? MODE_ICON[zone.mode] : null

  return (
    <div
      className={cn(
        "group relative overflow-hidden rounded-3xl border bg-card p-5 transition-all",
        zone.on && "shadow-xl ring-1",
        zone.on && tone.ring
      )}
    >
      {zone.on && (
        <div
          className={cn(
            "pointer-events-none absolute -top-16 -right-16 size-48 rounded-full blur-3xl",
            tone.bg
          )}
        />
      )}
      <div className="relative flex items-start justify-between gap-3">
        <Link
          to={`/zones/${zone.installation_id}/${zone.system_id}/${zone.zone_id}`}
          className="min-w-0"
        >
          <div className="flex items-center gap-1.5">
            <h3 className="truncate text-base font-semibold">{zone.name}</h3>
            <ChevronRight className="size-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5" />
          </div>
          <div className="mt-0.5 flex items-center gap-1.5 text-xs text-muted-foreground">
            {zone.on && ModeIcon && zone.mode ? (
              <span
                className={cn(
                  "inline-flex items-center gap-1 font-medium",
                  tone.text
                )}
              >
                <ModeIcon
                  className={cn("size-3.5", zone.demand && "animate-pulse")}
                />
                {zone.demand ? "En demande" : MODE_LABEL[zone.mode]}
              </span>
            ) : (
              <span>Éteinte</span>
            )}
            {zone.battery_low && (
              <BatteryWarning className="size-3.5 text-warning" />
            )}
            {zone.errors.length > 0 && (
              <TriangleAlert className="size-3.5 text-destructive" />
            )}
          </div>
        </Link>
        <Switch
          checked={zone.on}
          onCheckedChange={(on) => control.mutate({ zone, cmd: { on } })}
          aria-label={zone.on ? "Éteindre" : "Allumer"}
        />
      </div>

      <div className="relative mt-5 flex items-end justify-between">
        <div>
          <div className="tabular text-5xl leading-none font-semibold tracking-tight">
            {formatTemp(zone.room_temp)}
          </div>
          {zone.humidity !== null && (
            <div className="mt-2 inline-flex items-center gap-1 text-sm text-muted-foreground">
              <Droplet className="size-3.5" /> {zone.humidity} %
            </div>
          )}
        </div>
        <div
          className={cn(
            "flex items-center gap-1 rounded-2xl bg-muted/60 p-1",
            !zone.on && "opacity-60"
          )}
        >
          <Button
            size="icon"
            variant="ghost"
            className="size-8 rounded-xl"
            onClick={() => nudge(-(zone.step || 0.5))}
            aria-label="Baisser"
          >
            <Minus />
          </Button>
          <span
            className={cn(
              "tabular w-14 text-center text-lg font-semibold",
              pending && tone.text
            )}
          >
            {formatTemp(value)}
          </span>
          <Button
            size="icon"
            variant="ghost"
            className="size-8 rounded-xl"
            onClick={() => nudge(zone.step || 0.5)}
            aria-label="Monter"
          >
            <Plus />
          </Button>
        </div>
      </div>
    </div>
  )
}
