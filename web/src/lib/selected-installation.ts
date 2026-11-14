import { useSyncExternalStore } from "react"

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
