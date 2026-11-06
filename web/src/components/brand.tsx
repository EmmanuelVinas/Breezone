import { Wind } from "lucide-react"
import { cn } from "@/lib/utils"

export function BrandMark({ className }: { className?: string }) {
  return (
    <span
      className={cn(
        "relative inline-flex size-9 items-center justify-center rounded-2xl text-primary-foreground shadow-lg",
        "bg-[conic-gradient(from_200deg,var(--cool),var(--primary),var(--heat),var(--cool))]",
        className
      )}
    >
      <Wind className="size-5 drop-shadow" strokeWidth={2.4} />
    </span>
  )
}

export function Brand({ className }: { className?: string }) {
  return (
    <span className={cn("inline-flex items-center gap-2.5", className)}>
      <BrandMark />
      <span className="font-heading text-lg font-semibold tracking-tight">
        Free<span className="text-primary">Zone</span>
      </span>
    </span>
  )
}
