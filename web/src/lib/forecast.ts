import { useQuery } from "@tanstack/react-query"
import type { DeviceView } from "@/lib/api"
import { api } from "@/lib/api"
import { stateNumber } from "@/lib/format"

export const isSolarForecast = (d: DeviceView) =>
  d.traits.includes("solar.forecast@1")

/**
 * The value of a forecast curve at `ts`, between its points; 0 outside them and over a night (two
 * points more than 6 hours apart), as the server reads it.
 */
export function forecastAt(points: [number, number][], ts: number) {
  const i = points.findIndex(([t]) => t >= ts)
  if (i < 0) return 0
  if (i === 0) return points[0][0] === ts ? points[0][1] : 0
  const [t0, v0] = points[i - 1]
  const [t1, v1] = points[i]
  if (t1 - t0 > 6 * 3600) return 0
  return v0 + ((v1 - v0) * (ts - t0)) / (t1 - t0)
}

/** The solar forecast of the home (its first device that forecasts the sun) and its curve. */
export function useSolarForecast() {
  const devices = useQuery({ queryKey: ["devices"], queryFn: api.devices })
  const device = (devices.data ?? []).find(isSolarForecast)
  const curve = useQuery({
    queryKey: ["device-forecast", device?.id],
    queryFn: () => api.deviceForecast(device!.id),
    enabled: device !== undefined,
    // The server asks Forecast.Solar again every 30 minutes.
    refetchInterval: 600_000,
    staleTime: 300_000,
  })
  return {
    loading: devices.isLoading,
    /** When it was last read: "now" for a chart (the devices are read again every few seconds). */
    readAt: Math.max(devices.dataUpdatedAt, curve.dataUpdatedAt) / 1000,
    device,
    power: curve.data?.series.power ?? [],
    today: device ? stateNumber(device, "energy_today") : null,
    remaining: device ? stateNumber(device, "energy_remaining") : null,
    tomorrow: device ? stateNumber(device, "energy_tomorrow") : null,
  }
}
