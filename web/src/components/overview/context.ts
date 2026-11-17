import { createContext, useContext } from "react"
import type {
  Charted,
  DeviceView,
  EnergyDay,
  EnergyLive,
  Installation,
  Room,
  Routine,
  SurplusView,
  TraitInfo,
} from "@/lib/api"

/** What the blocks read: fetched once by the page. */
export type OverviewData = {
  installation: Installation
  devices: DeviceView[]
  rooms: Room[]
  live: EnergyLive | undefined
  today: EnergyDay | undefined
  surplus: SurplusView | undefined
  routines: Routine[]
  traits: TraitInfo[]
  /** The devices with a history, and its fields. */
  charted: Charted[]
}

export const OverviewContext = createContext<OverviewData | null>(null)

export function useOverview() {
  const data = useContext(OverviewContext)
  if (!data) throw new Error("overview data missing")
  return data
}
