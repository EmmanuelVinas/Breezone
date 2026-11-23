import { t } from "@/lib/i18n"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import type { Home } from "./fields"

const SELECTED =
  "data-[state=on]:border-primary data-[state=on]:bg-primary data-[state=on]:text-primary-foreground"

/** A stretch of the tariff, as pills: a kind of day and/or a slot ("any" for either). */
export function TariffPick({
  home,
  profile,
  slot,
  withSlot = true,
  onChange,
}: {
  home: Home
  profile?: string | null
  slot?: string | null
  /** Tomorrow: a kind of day only. */
  withSlot?: boolean
  onChange: (p: { profile: string | null; slot: string | null }) => void
}) {
  const tariff = home.tariff
  if (!tariff) return null
  const pick = (
    value: string,
    items: [string, string, string | null][],
    set: (v: string | null) => void,
    label: string
  ) => (
    <ToggleGroup
      type="single"
      variant="outline"
      size="sm"
      value={value}
      onValueChange={(v) => v && set(v === "*" ? null : v)}
      className="flex-wrap justify-start"
      aria-label={label}
    >
      {items.map(([id, name, color]) => (
        <ToggleGroupItem key={id} value={id} className={SELECTED}>
          {color && (
            <span
              className="size-2.5 rounded-full ring-1 ring-border"
              style={{ background: color }}
            />
          )}
          {name}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  )
  return (
    <div className="grid w-full gap-2 lg:w-auto">
      {pick(
        profile ?? "*",
        [
          ["*", t("tout jour"), null],
          ...tariff.profiles.map(
            (p) => [p.id, p.name, p.color] as [string, string, string | null]
          ),
        ],
        (v) => onChange({ profile: v, slot: withSlot ? (slot ?? null) : null }),
        t("Type de jour")
      )}
      {withSlot &&
        tariff.slots.length > 0 &&
        pick(
          slot ?? "*",
          [
            ["*", t("toute la journée"), null],
            ...tariff.slots.map(
              (s) => [s, s, null] as [string, string, string | null]
            ),
          ],
          (v) => onChange({ profile: profile ?? null, slot: v }),
          t("Plage")
        )}
    </div>
  )
}

/** A first stretch: the first slot of the tariff, else its first kind of day. */
export function firstStretch(home: Home) {
  const slot = home.tariff?.slots[0] ?? null
  return { profile: slot ? null : (home.tariff?.profiles[0]?.id ?? null), slot }
}
