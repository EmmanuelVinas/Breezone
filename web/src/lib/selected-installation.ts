import { useSyncExternalStore } from "react"
import { useQuery } from "@tanstack/react-query"
import { api } from "@/lib/api"

/** The installation chosen in « Mes installations », kept in this browser. */
const KEY = "breezone.last_installation"
const EVENT = "breezone:installation"

function read(): number | null {
  try {
    const v = Number(localStorage.getItem(KEY))
    return Number.isFinite(v) && v > 0 ? v : null
  } catch {
    return null
  }
}

export function rememberInstallation(id: number) {
  try {
    if (localStorage.getItem(KEY) === String(id)) return
    localStorage.setItem(KEY, String(id))
  } catch {
    /* storage blocked: the choice lasts this page only */
  }
  window.dispatchEvent(new Event(EVENT))
}

function subscribe(changed: () => void) {
  window.addEventListener(EVENT, changed)
  window.addEventListener("storage", changed)
  return () => {
    window.removeEventListener(EVENT, changed)
    window.removeEventListener("storage", changed)
  }
}

/** The id of the installation chosen, followed as it changes (other tabs too). */
export function useSelectedInstallationId() {
  return useSyncExternalStore(subscribe, read, () => null)
}

export const lastInstallationId = read

/** The installation chosen, else the first one (null without any). */
export function useChosenInstallation() {
  const id = useSelectedInstallationId()
  const list = useQuery({
    queryKey: ["status"],
    queryFn: api.status,
    staleTime: 60_000,
  }).data
  return list?.find((x) => x.id === id) ?? list?.[0] ?? null
}

/**
 * The modules (lights, shutters, energy…) are this server's: they go with its own installations.
 * False when the installation chosen is reached through another Breezone server, while this one
 * has installations of its own.
 */
export function useModulesHere() {
  const list = useQuery({
    queryKey: ["status"],
    queryFn: api.status,
    staleTime: 60_000,
  }).data
  const chosen = useChosenInstallation()
  return !(chosen?.access === "node" && list?.some((x) => x.access === "local"))
}
