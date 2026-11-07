import { useRef, useState } from "react"
import type { Zone } from "@/lib/api"
import { clampSetpoint, formatTemp } from "@/lib/format"
import { cn } from "@/lib/utils"

const START = 135 // degrees, 0 = east, clockwise (SVG coordinates)
const SWEEP = 270

function polar(cx: number, cy: number, r: number, deg: number) {
  const a = (deg * Math.PI) / 180
  return { x: cx + r * Math.cos(a), y: cy + r * Math.sin(a) }
}

function arc(cx: number, cy: number, r: number, from: number, to: number) {
  const s = polar(cx, cy, r, from)
  const e = polar(cx, cy, r, to)
  const large = to - from > 180 ? 1 : 0
  return `M ${s.x} ${s.y} A ${r} ${r} 0 ${large} 1 ${e.x} ${e.y}`
}

type Props = {
  zone: Zone
  value: number
  onChange: (v: number) => void
  onCommit: (v: number) => void
  colorVar: string
  size?: number
}

/** Circular thermostat dial: drag along the arc to change the setpoint. */
export function SetpointDial({
  zone,
  value,
  onChange,
  onCommit,
  colorVar,
  size = 280,
}: Props) {
  const ref = useRef<SVGSVGElement>(null)
  const [dragging, setDragging] = useState(false)
  const min = zone.min_temp ?? 15
  const max = zone.max_temp ?? 30
  const c = size / 2
  const r = c - 22
  const frac = (v: number) =>
    Math.min(1, Math.max(0, (v - min) / (max - min || 1)))
  const valueAngle = START + SWEEP * frac(value)
  const knob = polar(c, c, r, valueAngle)
  const room = zone.room_temp
  const roomPoint =
    room !== null ? polar(c, c, r, START + SWEEP * frac(room)) : null

  const fromPointer = (e: React.PointerEvent) => {
    const rect = ref.current!.getBoundingClientRect()
    const x = ((e.clientX - rect.left) / rect.width) * size - c
    const y = ((e.clientY - rect.top) / rect.height) * size - c
    let deg = (Math.atan2(y, x) * 180) / Math.PI
    if (deg < 0) deg += 360
    let rel = (deg - START + 360) % 360
    if (rel > SWEEP) rel = rel > SWEEP + (360 - SWEEP) / 2 ? 0 : SWEEP
    return clampSetpoint(zone, min + (rel / SWEEP) * (max - min))
  }

  const ticks = []
  for (let i = 0; i <= 30; i++) {
    const a = START + (SWEEP * i) / 30
    const p1 = polar(c, c, r + 13, a)
    const p2 = polar(c, c, r + (i % 5 === 0 ? 5 : 9), a)
    ticks.push(
      <line
        key={i}
        x1={p1.x}
        y1={p1.y}
        x2={p2.x}
        y2={p2.y}
        className="stroke-muted-foreground/40"
        strokeWidth={i % 5 === 0 ? 2 : 1}
      />
    )
  }

  return (
    <svg
      ref={ref}
      viewBox={`0 0 ${size} ${size}`}
      className={cn(
        "setpoint-dial",
        "w-full max-w-[320px] touch-none select-none",
        dragging ? "cursor-grabbing" : "cursor-grab"
      )}
      onPointerDown={(e) => {
        ;(e.target as Element).setPointerCapture?.(e.pointerId)
        setDragging(true)
        onChange(fromPointer(e))
      }}
      onPointerMove={(e) => dragging && onChange(fromPointer(e))}
      onPointerUp={(e) => {
        setDragging(false)
        onCommit(fromPointer(e))
      }}
      role="slider"
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      aria-label="Consigne"
      tabIndex={0}
      onKeyDown={(e) => {
        const step = zone.step || 0.5
        if (e.key === "ArrowUp" || e.key === "ArrowRight")
          onCommit(clampSetpoint(zone, value + step))
        if (e.key === "ArrowDown" || e.key === "ArrowLeft")
          onCommit(clampSetpoint(zone, value - step))
      }}
    >
      <defs>
        <linearGradient id="fz-dial" x1="0" y1="1" x2="1" y2="0">
          <stop
            offset="0%"
            stopColor={`color-mix(in oklch, var(${colorVar}) 45%, transparent)`}
          />
          <stop offset="100%" stopColor={`var(${colorVar})`} />
        </linearGradient>
        <filter id="fz-glow" x="-50%" y="-50%" width="200%" height="200%">
          <feGaussianBlur stdDeviation="6" />
        </filter>
      </defs>
      {ticks}
      <path
        d={arc(c, c, r, START, START + SWEEP)}
        className="stroke-muted"
        strokeWidth={14}
        fill="none"
        strokeLinecap="round"
      />
      <path
        d={arc(c, c, r, START, valueAngle)}
        stroke={`var(${colorVar})`}
        strokeWidth={14}
        fill="none"
        strokeLinecap="round"
        filter="url(#fz-glow)"
        opacity={0.45}
      />
      <path
        d={arc(c, c, r, START, valueAngle)}
        stroke="url(#fz-dial)"
        strokeWidth={14}
        fill="none"
        strokeLinecap="round"
      />
      {roomPoint && (
        <circle
          cx={roomPoint.x}
          cy={roomPoint.y}
          r={4}
          className="fill-foreground"
        />
      )}
      <circle
        cx={knob.x}
        cy={knob.y}
        r={dragging ? 15 : 13}
        className="fill-card"
        stroke={`var(${colorVar})`}
        strokeWidth={4}
      />
      <text
        x={c}
        y={c - 30}
        textAnchor="middle"
        className="fill-muted-foreground text-[13px] tracking-widest uppercase"
      >
        Consigne
      </text>
      <text
        x={c}
        y={c + 22}
        textAnchor="middle"
        className="tabular fill-foreground text-[64px] font-semibold"
      >
        {value.toFixed(1).replace(".", ",")}°
      </text>
      <text
        x={c}
        y={c + 56}
        textAnchor="middle"
        className="fill-muted-foreground text-[14px]"
      >
        Ambiante {formatTemp(room)}
      </text>
    </svg>
  )
}
