import { useId } from "react"
import { Area, AreaChart, ResponsiveContainer, YAxis } from "recharts"
import { cn } from "@/lib/utils"

/** A small line of `[ts, value]` points, drawn in the current text color. */
export function Sparkline({
  points,
  className,
  label,
}: {
  points: [number, number][]
  className?: string
  /** Accessible description. */
  label?: string
}) {
  const gradient = useId()
  if (points.length < 2) return null
  const data = points.map(([ts, value]) => ({ ts, value }))
  return (
    <div className={cn(className)} role="img" aria-label={label} title={label}>
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
          <Area
            dataKey="value"
            type="monotone"
            stroke="currentColor"
            strokeWidth={1.75}
            fill={`url(#${gradient})`}
            dot={false}
            activeDot={false}
            isAnimationActive={false}
          />
        </AreaChart>
      </ResponsiveContainer>
    </div>
  )
}
