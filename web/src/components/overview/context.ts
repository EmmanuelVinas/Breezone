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
  /** This server has a tariff of electricity, shown here (the price block). */
  priced: boolean
  /** This server measures energy: the parts of the energy page have something to show. */
  energy: boolean
  /** A Linky account gives what Enedis measured. */
  linky?: boolean
}

export const OverviewContext = createContext<OverviewData | null>(null)

export function useOverview() {
  const data = useContext(OverviewContext)
  if (!data) throw new Error("overview data missing")
  return data
}
