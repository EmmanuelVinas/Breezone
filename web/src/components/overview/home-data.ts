import { useMemo } from "react"
import { useQuery } from "@tanstack/react-query"
import { api } from "@/lib/api"
import { usePrice } from "@/components/electricity/hooks"
import {
  useChosenInstallation,
  useModulesHere,
} from "@/lib/selected-installation"
import type { OverviewData } from "./context"

const NO_ROLES = {}

/**
 * What the blocks of a page read about the installation chosen: its devices and those of the
 * modules of this server, the energy measured here, the surplus steered here, the price.
 */
export function useHomeData() {
  const installation = useChosenInstallation()
  const modulesHere = useModulesHere()
  const id = installation?.id
  const devices = useQuery({
    queryKey: ["devices"],
    queryFn: api.devices,
    refetchInterval: 10_000,
  })
  const rooms = useQuery({ queryKey: ["rooms"], queryFn: api.rooms })
  const live = useQuery({
    queryKey: ["energy-live"],
    queryFn: api.energyLive,
    refetchInterval: 10_000,
  })
  const today = useQuery({
    queryKey: ["energy-day", "today"],
    queryFn: () => api.energyDay(),
    refetchInterval: 5 * 60_000,
  })
  const surplus = useQuery({
    queryKey: ["surplus"],
    queryFn: api.surplus,
    refetchInterval: 10_000,
  })
  const routines = useQuery({ queryKey: ["routines"], queryFn: api.routines })
  const charted = useQuery({
    queryKey: ["charted"],
    queryFn: api.charted,
    staleTime: 5 * 60_000,
  })
  const traits = useQuery({
    queryKey: ["traits"],
    queryFn: api.traits,
    staleTime: Infinity,
  })
  const price = usePrice()
  // What Enedis measured, when a Linky account is linked (empty otherwise).
  const linky = useQuery({
    queryKey: ["linky", 30],
    queryFn: () => api.linky(30),
    staleTime: 30 * 60_000,
    retry: false,
  })

  // The devices of this installation, and those of the modules of this server.
  const mine = useMemo(
    () =>
      (devices.data ?? []).filter((d) =>
        d.installation_id === null ? modulesHere : d.installation_id === id
      ),
    [devices.data, id, modulesHere]
  )
  // The energy of the home is measured by modules: an installation without them has none.
  const energyHere = mine.some((d) => d.role !== null)
  const roles = energyHere ? live.data?.roles : NO_ROLES
  // The price of the electricity of this installation (its own contract).
  const priced = !!price.data?.now
  const surplusHere = (surplus.data?.loads ?? []).some((l) =>
    mine.some((d) => d.id === l.device_id)
  )
  const data: OverviewData | null = installation
    ? {
        installation,
        devices: mine,
        rooms: rooms.data ?? [],
        live: energyHere ? live.data : undefined,
        today: energyHere ? today.data : undefined,
        surplus: surplusHere ? surplus.data : undefined,
        routines: routines.data ?? [],
        traits: traits.data ?? [],
        charted: charted.data ?? [],
        priced,
        energy:
          modulesHere && Object.values(live.data?.roles ?? {}).some(Boolean),
        linky: modulesHere && !!linky.data?.prm,
      }
    : null
  return {
    installation,
    data,
    mine,
    roles,
    energyHere,
    surplusHere,
    priced,
    /** The energy is still being read: what depends on it is not judged yet. */
    energyLoading: energyHere && live.isLoading,
    loading: devices.isLoading,
    scenes: (routines.data?.length ?? 0) > 0,
  }
}
