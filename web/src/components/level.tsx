import { useRef, type CSSProperties, type ReactNode } from "react"
import { cn } from "@/lib/utils"

/**
 * A value set by dragging along a track (the mockups' big brightness bar, the white temperature
 * gradient), or with the arrow keys. `onChange` follows the drag, `onCommit` gets the value let go.
 */
export function Level({
  value,
  min,
  max,
  step = 1,
  vertical = false,
  disabled = false,
  label,
  valueText,
  onChange,
  onCommit,
  className,
  style,
  children,
}: {
  value: number
  min: number
  max: number
  step?: number
  vertical?: boolean
  disabled?: boolean
  label: string
  valueText?: string
  onChange: (v: number) => void
  onCommit: (v: number) => void
  className?: string
  style?: CSSProperties
  children?: ReactNode
}) {
  const ref = useRef<HTMLDivElement>(null)
  const last = useRef(value)
  const at = (x: number, y: number) => {
    const r = ref.current!.getBoundingClientRect()
    const f = vertical ? (r.bottom - y) / r.height : (x - r.left) / r.width
    const raw = min + Math.min(1, Math.max(0, f)) * (max - min)
    return Math.round(raw / step) * step
  }
  const move = (v: number) => {
    last.current = v
    onChange(v)
  }
  return (
    <div
      ref={ref}
      role="slider"
      tabIndex={disabled ? -1 : 0}
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      aria-valuetext={valueText}
      aria-orientation={vertical ? "vertical" : "horizontal"}
      aria-disabled={disabled || undefined}
      className={cn(
        "relative touch-none outline-none select-none focus-visible:ring-3 focus-visible:ring-ring/50",
        disabled ? "cursor-not-allowed opacity-60" : "cursor-pointer",
        className
      )}
      style={style}
      onPointerDown={(e) => {
        if (disabled) return
        e.currentTarget.setPointerCapture(e.pointerId)
        move(at(e.clientX, e.clientY))
      }}
      onPointerMove={(e) => {
        if (disabled || !e.currentTarget.hasPointerCapture(e.pointerId)) return
        move(at(e.clientX, e.clientY))
      }}
      onPointerUp={(e) => {
        if (disabled || !e.currentTarget.hasPointerCapture(e.pointerId)) return
        e.currentTarget.releasePointerCapture(e.pointerId)
        onCommit(last.current)
      }}
      onKeyDown={(e) => {
        if (disabled) return
        const delta =
          e.key === "ArrowUp" || e.key === "ArrowRight"
            ? step
            : e.key === "ArrowDown" || e.key === "ArrowLeft"
              ? -step
              : e.key === "PageUp"
                ? step * 10
                : e.key === "PageDown"
                  ? -step * 10
                  : null
        const jump = e.key === "Home" ? min : e.key === "End" ? max : null
        if (delta === null && jump === null) return
        e.preventDefault()
        const v = jump ?? Math.min(max, Math.max(min, value + delta!))
        move(v)
        onCommit(v)
      }}
    >
      {children}
    </div>
  )
}
