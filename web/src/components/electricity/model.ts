import type { DayProfile, PriceSlot, Tariff } from "@/lib/api"
import { locale, t } from "@/lib/i18n"

/** Minutes since midnight of "HH:MM". */
export const minutes = (hhmm: string) => {
  const [h, m] = hhmm.split(":").map(Number)
  return h * 60 + m
}

/** "HH:MM" of minutes since midnight (24:00 for the end of the day). */
export const clock = (min: number) =>
  `${String(Math.floor(min / 60)).padStart(2, "0")}:${String(min % 60).padStart(2, "0")}`

/** Where the price may be moved to: half hours. */
export const STEP = 30
export const DAY = 24 * 60

/** The end of a slot: the start of the next one, or midnight. */
export const slotEnd = (slots: PriceSlot[], i: number) =>
  i + 1 < slots.length ? minutes(slots[i + 1].start) : DAY

/** A price per kWh as typed ("0,1609" or "0.1609"); `null` when it is not one. */
export function parsePrice(text: string): number | null {
  const v = Number(text.trim().replace(",", "."))
  return text.trim() !== "" && Number.isFinite(v) && v >= 0 ? v : null
}

export function money(v: number, currency: string, digits = 4) {
  return v.toLocaleString(locale(), {
    style: "currency",
    currency,
    minimumFractionDigits: 2,
    maximumFractionDigits: digits,
  })
}

/** Colours offered for a profile (Tempo's first). */
export const PROFILE_COLORS = [
  "#3b82f6",
  "#e5e7eb",
  "#ef4444",
  "#14b8a6",
  "#f59e0b",
  "#a855f7",
]

/** The slot cut in two at its middle (on a half hour), when it is long enough. */
export function split(slots: PriceSlot[], i: number): PriceSlot[] | null {
  const start = minutes(slots[i].start)
  const end = slotEnd(slots, i)
  if (end - start < 2 * STEP) return null
  const middle = start + Math.round((end - start) / 2 / STEP) * STEP
  const copy = { ...slots[i], start: clock(middle) }
  return [...slots.slice(0, i + 1), copy, ...slots.slice(i + 1)]
}

/** The slot given to the one before it (the first one to the next). */
export function merge(slots: PriceSlot[], i: number): PriceSlot[] {
  if (slots.length < 2) return slots
  if (i === 0) return [{ ...slots[1], start: "00:00" }, ...slots.slice(2)]
  return slots.filter((_, j) => j !== i)
}

/** The start of slot `i` (not the first) moved, between its neighbours. */
export function moveStart(
  slots: PriceSlot[],
  i: number,
  to: number
): PriceSlot[] {
  const low = minutes(slots[i - 1].start) + STEP
  const high = slotEnd(slots, i) - STEP
  const at = Math.min(high, Math.max(low, Math.round(to / STEP) * STEP))
  return slots.map((s, j) => (j === i ? { ...s, start: clock(at) } : s))
}

const slot = (start: string, name: string, price: number): PriceSlot => ({
  start,
  name,
  price,
})

const profile = (
  id: string,
  name: string,
  color: string,
  slots: PriceSlot[]
): DayProfile => ({ id, name, color, slots })

const hcHp = (hc: number, hp: number) => [
  slot("00:00", "HC", hc),
  slot("06:00", "HP", hp),
  slot("22:00", "HC", hc),
]

const every = (id: string) => Array.from({ length: 7 }, () => id)

/** The offers a contract may start from; prices are indicative, to check on the bill. */
export type Offer = "base" | "hc" | "tempo" | "custom"

export const OFFER_LABEL: Record<Offer, string> = {
  base: t("Base"),
  hc: t("Heures creuses"),
  tempo: "Tempo",
  custom: t("Personnalisée"),
}

export function template(offer: Offer): Tariff {
  switch (offer) {
    case "base":
      return {
        subscription: 16,
        export_price: null,
        profiles: [
          profile("normal", t("Tous les jours"), PROFILE_COLORS[3], [
            slot("00:00", t("Base"), 0.2016),
          ]),
        ],
        calendar: { day_start: "00:00", weekdays: every("normal"), colors: {} },
      }
    case "hc":
      return {
        subscription: 16.5,
        export_price: null,
        profiles: [
          profile(
            "normal",
            t("Tous les jours"),
            PROFILE_COLORS[3],
            hcHp(0.1696, 0.2146)
          ),
        ],
        calendar: { day_start: "00:00", weekdays: every("normal"), colors: {} },
      }
    case "tempo":
      return {
        subscription: 16.2,
        export_price: null,
        profiles: [
          profile(
            "blue",
            t("Jour bleu"),
            PROFILE_COLORS[0],
            hcHp(0.1288, 0.1552)
          ),
          profile(
            "white",
            t("Jour blanc"),
            PROFILE_COLORS[1],
            hcHp(0.1447, 0.1792)
          ),
          profile(
            "red",
            t("Jour rouge"),
            PROFILE_COLORS[2],
            hcHp(0.1518, 0.6586)
          ),
        ],
        calendar: {
          day_start: "06:00",
          weekdays: every("blue"),
          colors: { blue: "blue", white: "white", red: "red" },
        },
      }
    case "custom":
      return {
        subscription: 0,
        export_price: null,
        profiles: [
          profile("normal", t("Normal"), PROFILE_COLORS[3], [
            slot("00:00", t("Base"), 0.2),
          ]),
        ],
        calendar: { day_start: "00:00", weekdays: every("normal"), colors: {} },
      }
  }
}

/** What is wrong with a tariff, as the server would say it (null: nothing). */
export function problem(tariff: Tariff): string | null {
  if (tariff.profiles.length === 0) return t("Ajoutez au moins un profil.")
  for (const p of tariff.profiles) {
    if (!p.name.trim()) return t("Chaque profil a besoin d'un nom.")
    if (p.slots.some((s) => s.price > 10))
      return t("« {name} » : un prix au-delà de 10 par kWh.", { name: p.name })
  }
  return null
}

/** A new id for a profile, unlike the others. */
export function newProfileId(profiles: DayProfile[]) {
  let i = profiles.length + 1
  while (profiles.some((p) => p.id === `p${i}`)) i++
  return `p${i}`
}

/** Today, as a local "YYYY-MM-DD". */
export function today() {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`
}
