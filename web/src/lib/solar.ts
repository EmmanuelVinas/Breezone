import type { DeviceView } from "@/lib/api"

export const isInverter = (d: DeviceView | undefined) =>
  !!d?.traits.includes("solar.inverter@1")

/**
 * How the energy page counts a solar inverter, as the server does: through the meter that says it
 * measures it, else by default unless a production measure that says nothing may measure it too;
 * an inverter given the role of a production always counts.
 */
export function inverterCounting(
  inverter: DeviceView,
  devices: DeviceView[]
):
  | { counted: true; by?: DeviceView; forced: boolean }
  | { counted: false; overlap: DeviceView } {
  const by = devices.find(
    (d) => d.id !== inverter.id && d.measures.device_id === inverter.id
  )
  if (by) return { counted: true, by, forced: false }
  if (inverter.role === "solar") return { counted: true, forced: true }
  const overlap = devices.find(
    (d) =>
      d.id !== inverter.id &&
      !isInverter(d) &&
      d.role === "solar" &&
      d.measures.device_id === null
  )
  return overlap
    ? { counted: false, overlap }
    : { counted: true, forced: false }
}
