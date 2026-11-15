import type { DeviceView } from "@/lib/api"
import { t } from "@/lib/i18n"

/** The state of a shutter, a blind, a roof window or a door, as `cover.position@1` describes it. */
export type CoverState = {
  position?: number
  target?: number
  moving?: "opening" | "closing" | "stopped"
  stoppable?: boolean
}

export const isCover = (d: DeviceView) => d.traits.includes("cover.position@1")

export const coverState = (d: DeviceView) => (d.state ?? {}) as CoverState

/** Unknown counts as stoppable: only a driver that knows it cannot stop says so (a HomeKit
 * shutter without « hold position », like Legrand's). */
export const isStoppable = (d: DeviceView) => coverState(d).stoppable !== false

/** What the device is: its driver says it (a HomeKit shutter, a VELUX window…). */
export function coverKind(d: DeviceView) {
  if (d.kind === "window") return t("Fenêtre de toit")
  if (d.kind === "door") return t("Porte")
  return t("Volet roulant")
}

export function coverText(s: CoverState) {
  if (s.moving === "opening")
    return typeof s.target === "number"
      ? t("S'ouvre… {n} %", { n: s.target })
      : t("S'ouvre…")
  if (s.moving === "closing")
    return typeof s.target === "number"
      ? t("Se ferme… {n} %", { n: s.target })
      : t("Se ferme…")
  if (typeof s.position !== "number") return "–"
  if (s.position <= 0) return t("Fermé")
  if (s.position >= 100) return t("Ouvert")
  return t("Ouvert à {n} %", { n: s.position })
}

export const isOpen = (d: DeviceView) =>
  d.available && (coverState(d).position ?? 0) > 0

/** About 25 s from closed to open: gateways like Legrand's only tell the position at the end of a
 * move, so the move in between is estimated. */
const MS_PER_PERCENT = 250
/** After the estimated end, how long the hardware has to confirm before what it says shows again. */
const GRACE_MS = 20_000

type Motion = { from: number; to: number; since: number; heard: boolean }
const motions = new Map<number, Motion>()
const targets = new Map<number, number | undefined>()

const travel = (m: Motion) => Math.abs(m.to - m.from) * MS_PER_PERCENT

function estimate(m: Motion, now: number) {
  const done = travel(m) ? Math.min(1, (now - m.since) / travel(m)) : 1
  return m.from + (m.to - m.from) * done
}

/** A command just sent: the shutter shows moving at once, from where it is shown. */
export function startMotion(d: DeviceView, to: number) {
  const from = coverView(d).position ?? 0
  if (from === to) motions.delete(d.id)
  else motions.set(d.id, { from, to, since: Date.now(), heard: false })
}

export const stopMotion = (d: DeviceView) => motions.delete(d.id)

/** The state to show: the hardware's, or, while a move goes on, where the shutter should be by
 * now. Ends when the hardware reaches the target, says it stopped short, or stays silent too long. */
export function coverView(d: DeviceView): CoverState {
  const s = coverState(d)
  const now = Date.now()
  const position = s.position ?? 0
  const known = targets.has(d.id)
  const previous = targets.get(d.id)
  targets.set(d.id, s.target)
  const moving = s.moving === "opening" || s.moving === "closing"
  let m = motions.get(d.id)
  // A move started elsewhere (a switch, a scene, the phone): followed too, but not a target that
  // was already there when the page opened and never reached.
  if (
    typeof s.target === "number" &&
    s.target !== position &&
    m?.to !== s.target &&
    (moving || (known && previous !== s.target))
  ) {
    m = {
      from: m ? estimate(m, now) : position,
      to: s.target,
      since: now,
      heard: moving,
    }
    motions.set(d.id, m)
  }
  if (!m) return s
  if (moving) m.heard = true
  const stale = now - m.since > travel(m) + GRACE_MS
  if (
    position === m.to ||
    (m.heard && !moving && s.target === m.to) ||
    (stale && !moving)
  ) {
    motions.delete(d.id)
    return s
  }
  // A device that tells its position on the way is believed when it is further along.
  const timed = estimate(m, now)
  const up = m.to > m.from
  const at = up
    ? Math.max(timed, Math.min(position, m.to))
    : Math.min(timed, Math.max(position, m.to))
  return {
    ...s,
    position: Math.round(at),
    target: m.to,
    moving: up ? "opening" : "closing",
  }
}

export const isMoving = (d: DeviceView) => {
  const m = coverView(d).moving
  return m === "opening" || m === "closing"
}
