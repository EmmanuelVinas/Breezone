import { useRef, useState } from "react"
import { Merge, Scissors, Trash2 } from "lucide-react"
import type { DayProfile, PriceSlot } from "@/lib/api"
import { t } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  clock,
  DAY,
  merge,
  minutes,
  money,
  moveStart,
  parsePrice,
  PROFILE_COLORS,
  slotEnd,
  split,
} from "./model"

/** A price typed freely ("0,16"), given back once it is one. */
export function PriceInput({
  value,
  onChange,
  id,
  label,
}: {
  value: number | null
  onChange: (v: number | null) => void
  id?: string
  label?: string
}) {
  const [text, setText] = useState<string | null>(null)
  const shown = text ?? (value === null ? "" : String(value).replace(".", ","))
  return (
    <Input
      id={id}
      aria-label={label}
      inputMode="decimal"
      value={shown}
      onChange={(e) => {
        setText(e.target.value)
        const v = parsePrice(e.target.value)
        if (v !== null || e.target.value.trim() === "") onChange(v)
      }}
      onBlur={() => setText(null)}
    />
  )
}

const HOURS = [0, 6, 12, 18, 24]

/** A day of a profile as a bar of 24 hours: a slot to pick, a limit to drag. */
function Frieze({
  profile,
  selected,
  onSelect,
  onChange,
  currency,
}: {
  profile: DayProfile
  selected: number | null
  onSelect: (i: number) => void
  onChange: (slots: PriceSlot[]) => void
  currency: string
}) {
  const bar = useRef<HTMLDivElement>(null)
  const color = profile.color ?? PROFILE_COLORS[3]
  const prices = profile.slots.map((s) => s.price)
  const low = Math.min(...prices)
  const high = Math.max(...prices)
  // The dearer the slot, the stronger its colour.
  // A light colour stays faint, so that the text over it can be read.
  const light = color === PROFILE_COLORS[1] ? 0.55 : 1
  const strength = (p: number) =>
    Math.round(
      (high === low ? 32 : 14 + ((p - low) / (high - low)) * 30) * light
    )
  const at = (clientX: number) => {
    const r = bar.current!.getBoundingClientRect()
    return ((clientX - r.left) / r.width) * DAY
  }
  return (
    <div className="grid gap-1">
      <div
        ref={bar}
        className="relative h-12 touch-none rounded-xl bg-muted select-none"
      >
        <div className="absolute inset-0 overflow-hidden rounded-xl">
          {profile.slots.map((s, i) => {
            const start = minutes(s.start)
            const end = slotEnd(profile.slots, i)
            return (
              <button
                key={i}
                type="button"
                onClick={() => onSelect(i)}
                aria-pressed={selected === i}
                aria-label={t("{name}, de {from} à {to}, {price}", {
                  name: s.name || t("Plage"),
                  from: s.start,
                  to: clock(end),
                  price: money(s.price, currency),
                })}
                className={cn(
                  "absolute inset-y-0 flex flex-col justify-center overflow-hidden border-r border-background px-2 text-left text-xs",
                  selected === i &&
                    "z-[1] ring-2 ring-primary outline-none ring-inset"
                )}
                style={{
                  left: `${(start / DAY) * 100}%`,
                  width: `${((end - start) / DAY) * 100}%`,
                  background: `color-mix(in oklch, ${color} ${strength(s.price)}%, transparent)`,
                }}
              >
                <b className="truncate">{s.name}</b>
                <span className="truncate text-muted-foreground tabular-nums">
                  {money(s.price, currency)}
                </span>
              </button>
            )
          })}
        </div>
        {profile.slots.slice(1).map((s, j) => {
          const i = j + 1
          return (
            <span
              key={i}
              role="slider"
              tabIndex={0}
              aria-label={t("Début de « {name} »", { name: s.name })}
              aria-valuemin={0}
              aria-valuemax={DAY}
              aria-valuenow={minutes(s.start)}
              aria-valuetext={s.start}
              className="absolute -inset-y-1 z-[2] w-2 -translate-x-1/2 cursor-ew-resize rounded bg-foreground/85 focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none"
              style={{ left: `${(minutes(s.start) / DAY) * 100}%` }}
              onPointerDown={(e) => {
                e.currentTarget.setPointerCapture(e.pointerId)
              }}
              onPointerMove={(e) => {
                if (!e.currentTarget.hasPointerCapture(e.pointerId)) return
                onChange(moveStart(profile.slots, i, at(e.clientX)))
              }}
              onKeyDown={(e) => {
                const by =
                  e.key === "ArrowLeft" ? -30 : e.key === "ArrowRight" ? 30 : 0
                if (!by) return
                e.preventDefault()
                onChange(moveStart(profile.slots, i, minutes(s.start) + by))
              }}
            />
          )
        })}
      </div>
      <div className="relative h-4 text-[11px] text-muted-foreground">
        {HOURS.map((h) => (
          <span
            key={h}
            className="absolute whitespace-nowrap"
            style={{
              left: `${(h / 24) * 100}%`,
              transform: `translateX(${h === 0 ? 0 : h === 24 ? -100 : -50}%)`,
            }}
          >
            {h} h
          </span>
        ))}
      </div>
    </div>
  )
}

/** A profile of day: its name, its colour, its slots on a bar, the slot picked to change. */
export function ProfileEditor({
  profile,
  onChange,
  onRemove,
  currency,
}: {
  profile: DayProfile
  onChange: (p: DayProfile) => void
  /** `undefined`: it cannot be removed (the last one). */
  onRemove?: () => void
  currency: string
}) {
  const [selected, setSelected] = useState<number | null>(null)
  const slots = profile.slots
  const sel = selected !== null && selected < slots.length ? selected : null
  const setSlots = (next: PriceSlot[]) => onChange({ ...profile, slots: next })
  const setSlot = (i: number, patch: Partial<PriceSlot>) =>
    setSlots(slots.map((s, j) => (j === i ? { ...s, ...patch } : s)))
  const cut = sel !== null ? split(slots, sel) : null

  return (
    <div className="grid gap-3 rounded-2xl border p-4">
      <div className="flex flex-wrap items-center gap-3">
        <Input
          aria-label={t("Nom du profil")}
          value={profile.name}
          onChange={(e) => onChange({ ...profile, name: e.target.value })}
          className="h-9 w-48 font-medium"
        />
        <div
          className="flex items-center gap-1.5"
          role="group"
          aria-label={t("Couleur")}
        >
          {PROFILE_COLORS.map((c) => (
            <button
              key={c}
              type="button"
              aria-label={t("Couleur {c}", { c })}
              aria-pressed={profile.color === c}
              onClick={() => onChange({ ...profile, color: c })}
              className={cn(
                "size-6 rounded-full border",
                profile.color === c &&
                  "ring-2 ring-primary ring-offset-2 ring-offset-background"
              )}
              style={{ background: c }}
            />
          ))}
        </div>
        <span className="text-sm text-muted-foreground">
          {slots.length === 1
            ? t("1 plage")
            : t("{n} plages", { n: slots.length })}
        </span>
        {onRemove && (
          <Button
            variant="ghost"
            size="sm"
            className="ml-auto text-destructive"
            onClick={onRemove}
          >
            <Trash2 /> {t("Supprimer le profil")}
          </Button>
        )}
      </div>
      <Frieze
        profile={profile}
        selected={sel}
        onSelect={(i) => setSelected(sel === i ? null : i)}
        onChange={setSlots}
        currency={currency}
      />
      {sel !== null ? (
        <div className="grid gap-3 rounded-xl bg-muted/50 p-3 sm:grid-cols-[1fr_7rem_7rem_9rem_auto] sm:items-end">
          <div className="grid gap-1.5">
            <Label htmlFor={`${profile.id}-name`}>{t("Nom de la plage")}</Label>
            <Input
              id={`${profile.id}-name`}
              value={slots[sel].name}
              placeholder={t("HP, HC…")}
              onChange={(e) => setSlot(sel, { name: e.target.value })}
            />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor={`${profile.id}-from`}>{t("De")}</Label>
            <Input
              id={`${profile.id}-from`}
              type="time"
              step={1800}
              value={slots[sel].start}
              disabled={sel === 0}
              onChange={(e) =>
                e.target.value &&
                setSlots(moveStart(slots, sel, minutes(e.target.value)))
              }
            />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor={`${profile.id}-to`}>{t("À")}</Label>
            <Input
              id={`${profile.id}-to`}
              type={sel + 1 < slots.length ? "time" : "text"}
              step={1800}
              value={sel + 1 < slots.length ? slots[sel + 1].start : "24:00"}
              disabled={sel + 1 >= slots.length}
              onChange={(e) =>
                e.target.value &&
                setSlots(moveStart(slots, sel + 1, minutes(e.target.value)))
              }
            />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor={`${profile.id}-price`}>
              {t("Prix ({currency} / kWh)", { currency })}
            </Label>
            <PriceInput
              id={`${profile.id}-price`}
              value={slots[sel].price}
              onChange={(v) => v !== null && setSlot(sel, { price: v })}
            />
          </div>
          <div className="flex gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={!cut}
              onClick={() => cut && setSlots(cut)}
            >
              <Scissors /> {t("Couper")}
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={slots.length < 2}
              onClick={() => {
                setSlots(merge(slots, sel))
                setSelected(null)
              }}
            >
              <Merge /> {t("Fusionner")}
            </Button>
          </div>
        </div>
      ) : (
        <p className="text-sm text-muted-foreground">
          {t(
            "Touchez une plage pour son nom et son prix, glissez une limite pour la déplacer (par demi-heure)."
          )}
        </p>
      )}
    </div>
  )
}
