import { useMutation, useQueryClient } from "@tanstack/react-query"
import { toast } from "sonner"
import type { Installation, Zone, ZoneCommand } from "@/lib/api"
import { api } from "@/lib/api"
import { setCommandStatus } from "@/lib/command-status"

type Target = Pick<Zone, "installation_id" | "system_id" | "zone_id">

/** Applies a command optimistically, then sends it to the server. */
export function useZoneControl() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ zone, cmd }: { zone: Target; cmd: ZoneCommand }) =>
      api.control(zone, cmd),
    onMutate: async ({ zone, cmd }) => {
      setCommandStatus([zone], "pending")
      await qc.cancelQueries({ queryKey: ["status"] })
      const previous = qc.getQueryData<Installation[]>(["status"])
      qc.setQueryData<Installation[]>(["status"], (old) =>
        (old ?? []).map((inst) =>
          inst.id !== zone.installation_id
            ? inst
            : {
                ...inst,
                zones: inst.zones.map((z) => {
                  const sameSystem = z.system_id === zone.system_id
                  const same = sameSystem && z.zone_id === zone.zone_id
                  if (!same && !(sameSystem && cmd.mode)) return z
                  // The mode is system-wide: every zone of the system follows.
                  if (!same) return { ...z, mode: cmd.mode ?? z.mode }
                  return { ...z, ...stripUndefined(cmd) } as Zone
                }),
              }
        )
      )
      return { previous }
    },
    onSuccess: (_zone, { zone }) => setCommandStatus([zone], "done"),
    onError: (err, { zone }, ctx) => {
      setCommandStatus([zone], "error")
      if (ctx?.previous) qc.setQueryData(["status"], ctx.previous)
      toast.error("Commande refusée", {
        description: err instanceof Error ? err.message : String(err),
      })
    },
  })
}

function stripUndefined(cmd: ZoneCommand) {
  return Object.fromEntries(
    Object.entries(cmd).filter(([, v]) => v !== undefined)
  )
}

/** Switches a set of zones on/off at once (e.g. a whole floor), with a confirmation message. */
export function useGroupPower() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({
      zones,
      on,
    }: {
      zones: Zone[]
      on: boolean
      label?: string
    }) =>
      api.batch(
        zones.map((z) => ({
          installation_id: z.installation_id,
          system_id: z.system_id,
          zone_id: z.zone_id,
          on,
        }))
      ),
    onMutate: async ({ zones, on }) => {
      setCommandStatus(zones, "pending")
      const keys = new Set(
        zones.map((z) => `${z.installation_id}-${z.system_id}-${z.zone_id}`)
      )
      qc.setQueryData<Installation[]>(["status"], (old) =>
        (old ?? []).map((inst) => ({
          ...inst,
          zones: inst.zones.map((z) =>
            keys.has(`${z.installation_id}-${z.system_id}-${z.zone_id}`)
              ? { ...z, on }
              : z
          ),
        }))
      )
    },
    onSuccess: (res, { zones, on, label }) => {
      zones.forEach((z, i) =>
        setCommandStatus([z], res.results[i]?.ok === false ? "error" : "done")
      )
      const failed = res.results.filter((r) => !r.ok).length
      const ok = zones.length - failed
      const what = on ? "allumée" : "éteinte"
      if (failed)
        toast.warning(
          `${label ? `${label} : ` : ""}${ok} zone(s) ${what}(s), ${failed} sans réponse`
        )
      else
        toast.success(
          `${label ? `${label} : ` : ""}${ok} zone${ok > 1 ? "s" : ""} ${what}${ok > 1 ? "s" : ""}`
        )
    },
    onError: (err, { zones }) => {
      setCommandStatus(zones, "error")
      toast.error("Échec", {
        description: err instanceof Error ? err.message : String(err),
      })
      qc.invalidateQueries({ queryKey: ["status"] })
    },
  })
}
