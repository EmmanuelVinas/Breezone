import { useId } from "react"
import { Area, AreaChart, ResponsiveContainer, Tooltip, YAxis } from "recharts"
import { dateTime, number, watts } from "@/lib/format"
import { cn } from "@/lib/utils"

/** How a value of a device's field reads ("1,2 kW", "21,4°", "48 %"). */
export function fieldFormat(field: string): (v: number) => string {
  if (field === "power" || field.startsWith("power_")) return (v) => watts(v)
  if (field.startsWith("energy")) return (v) => `${number(v, 2)} kWh`
  if (field.includes("temperature") || field === "setpoint")
    return (v) => `${number(v, 1)}°`
  if (
    ["humidity", "level", "position", "brightness", "battery"].includes(field)
  )
    return (v) => `${number(v, 0)} %`
  if (field === "co2") return (v) => `${number(v, 0)} ppm`
  if (field === "flow") return (v) => `${number(v, 0)} m³/h`
  if (field === "illuminance") return (v) => `${number(v, 0)} lx`
  if (field === "pressure") return (v) => `${number(v, 0)} hPa`
  return (v) => number(v, 1)
}

/**
 * A small line of `[ts, value]` points, drawn in the current text color. Hovered, it tells the
 * time (`at`, a date and a time by default) and the value (`format`) of the point under the mouse.
 */
export function Sparkline({
  points,
  className,
  label,
  format = (v) => number(v, 1),
  at = dateTime,
}: {
  points: [number, number][]
  className?: string
  /** Accessible description. */
  label?: string
  format?: (v: number) => string
  at?: (ts: number) => string
}) {
  const gradient = useId()
  if (points.length < 2) return null
  const data = points.map(([ts, value]) => ({ ts, value }))
  return (
    <div className={cn(className)} role="img" aria-label={label}>
      <ResponsiveContainer width="100%" height="100%">
        <AreaChart
          data={data}
          margin={{ top: 2, right: 1, bottom: 0, left: 1 }}
        >
          <defs>
            <linearGradient id={gradient} x1="0" x2="0" y1="0" y2="1">
              <stop offset="0" stopColor="currentColor" stopOpacity={0.18} />
              <stop offset="1" stopColor="currentColor" stopOpacity={0} />
            </linearGradient>
          </defs>
          {/* A flat line sits in the middle rather than at the bottom. */}
          <YAxis
            hide
            domain={([lo, hi]: readonly number[]) => {
              const pad = Math.max(0.5 - (hi - lo), 0) / 2
              return [lo - pad, hi + pad]
            }}
          />
          <Tooltip
            cursor={{ stroke: "currentColor", strokeOpacity: 0.3 }}
            isAnimationActive={false}
            allowEscapeViewBox={{ x: false, y: true }}
            // Above the line: the cards cut what goes past their edges, and a small curve sits
            // at their bottom.
            position={{ y: -46 }}
            wrapperStyle={{ zIndex: 50, outline: "none" }}
            content={({ active, payload }) => {
              const p = payload?.[0]?.payload as
                { ts: number; value: number } | undefined
              if (!active || !p) return null
              return (
                <div className="rounded-xl border bg-popover px-2.5 py-1.5 text-xs text-popover-foreground shadow-md">
                  <div className="text-muted-foreground">{at(p.ts)}</div>
                  <div className="tabular font-medium">{format(p.value)}</div>
                </div>
              )
            }}
          />
          <Area
            dataKey="value"
            type="monotone"
            stroke="currentColor"
            strokeWidth={1.75}
            fill={`url(#${gradient})`}
            dot={false}
            activeDot={{ r: 3, fill: "currentColor", strokeWidth: 0 }}
            isAnimationActive={false}
          />
        </AreaChart>
      </ResponsiveContainer>
    </div>
  )
}
