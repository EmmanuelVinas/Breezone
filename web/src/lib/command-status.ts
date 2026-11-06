import { useSyncExternalStore } from "react"

/**
 * Visual acknowledgement of zone commands: "pending" while the server applies it, "done" for a
 * moment once the WebServer confirmed it, "error" for a moment if it was refused.
 */
export type CommandStatus = "pending" | "done" | "error"

type Target = { installation_id: number; system_id: number; zone_id: number }

const statuses = new Map<string, CommandStatus>()
const timers = new Map<string, ReturnType<typeof setTimeout>>()
const listeners = new Set<() => void>()

const key = (z: Target) => `${z.installation_id}-${z.system_id}-${z.zone_id}`

function emit() {
  listeners.forEach((l) => l())
}

export function setCommandStatus(zones: Target[], status: CommandStatus) {
  for (const z of zones) {
    const k = key(z)
    clearTimeout(timers.get(k))
    statuses.set(k, status)
    if (status !== "pending") {
      timers.set(
        k,
        setTimeout(
          () => {
            statuses.delete(k)
            emit()
          },
          status === "done" ? 1500 : 3000
        )
      )
    }
  }
  emit()
}

export function useCommandStatus(zone: Target): CommandStatus | undefined {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => listeners.delete(l)
    },
    () => statuses.get(key(zone))
  )
}
