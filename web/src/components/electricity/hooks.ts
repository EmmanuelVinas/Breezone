import { useQuery } from "@tanstack/react-query"
import { api } from "@/lib/api"
import { useChosenInstallation } from "@/lib/selected-installation"

/** The price of electricity now at the installation chosen (none without one). */
export function usePrice() {
  const id = useChosenInstallation()?.id
  return useQuery({
    queryKey: ["price", id],
    queryFn: () => api.price(id!, 24),
    enabled: id !== undefined,
    refetchInterval: 60_000,
  })
}

/** The contract and the prices of the installation chosen. */
export function useTariff() {
  const id = useChosenInstallation()?.id
  return useQuery({
    queryKey: ["tariff", id],
    queryFn: () => api.tariff(id!),
    enabled: id !== undefined,
    staleTime: 5 * 60_000,
  })
}
