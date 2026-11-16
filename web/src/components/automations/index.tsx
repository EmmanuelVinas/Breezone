import { useState } from "react"
import { useQuery } from "@tanstack/react-query"
import { BookOpen, Plus } from "lucide-react"
import type {
  Automation,
  AutomationCondition,
  AutomationInput,
} from "@/lib/api"
import { api, currentUser, isDeviceAction } from "@/lib/api"
import { t } from "@/lib/i18n"
import { useChosenInstallations } from "@/lib/selected-installation"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { AutomationEditor, inputOf } from "./editor"
import type { Home } from "./fields"
import { AutomationCard, AutomationsBar, UnreadableAutomations } from "./list"
import { RecipesDialog, blank } from "./recipes"

export function AutomationsSection() {
  const automations = useQuery({
    queryKey: ["automations"],
    queryFn: api.automations,
    // What the rules see now (the measure, the conditions) follows the home.
    refetchInterval: 30_000,
  })
  const devices = useQuery({ queryKey: ["devices"], queryFn: api.devices })
  const traits = useQuery({
    queryKey: ["traits"],
    queryFn: api.traits,
    staleTime: Infinity,
  })
  const routines = useQuery({ queryKey: ["routines"], queryFn: api.routines })
  const live = useQuery({ queryKey: ["energy-live"], queryFn: api.energyLive })
  const { installations, ids } = useChosenInstallations()
  const admin = currentUser()?.role === "admin"
  const [editing, setEditing] = useState<{
    id?: number
    initial: AutomationInput
    nonce: number
  } | null>(null)
  const [choosing, setChoosing] = useState(false)

  const editable = new Set(
    installations
      .filter((i) => i.controllable && i.rights.routines)
      .map((i) => i.id)
  )
  // The zones of the installation chosen, and the devices of the modules of this server.
  const shown = (devices.data ?? []).filter(
    (d) => d.installation_id === null || ids.has(d.installation_id)
  )
  const drivable = new Set(
    shown
      .filter((d) =>
        d.installation_id === null ? admin : editable.has(d.installation_id)
      )
      .map((d) => d.id)
  )
  const home: Home = {
    devices: shown,
    drivable,
    traits: traits.data ?? [],
    scenes: (routines.data ?? []).filter((r) =>
      r.actions.every((x) =>
        isDeviceAction(x)
          ? drivable.has(x.device_id)
          : editable.has(x.installation_id)
      )
    ),
    roles: live.data?.roles ?? {
      grid: false,
      solar: false,
      consumption: false,
    },
  }
  const installationOf = new Map(
    (devices.data ?? []).map((d) => [d.id, d.installation_id])
  )
  const deviceIds = (c: AutomationCondition): number[] =>
    c.kind === "group"
      ? c.conditions.flatMap(deviceIds)
      : c.kind === "state"
        ? [c.device_id]
        : c.kind === "measure" && c.measure.kind === "device"
          ? [c.measure.device_id]
          : []
  const steps = (a: Automation) => [
    ...a.actions,
    ...(a.else ?? []),
    ...(a.over ?? []),
  ]
  const used = (a: Automation) => [
    ...steps(a).flatMap((s) => (s.kind === "device" ? [s.device_id] : [])),
    ...a.conditions.flatMap(deviceIds),
    ...(a.triggers ?? []).flatMap((w) =>
      w.kind === "state"
        ? [w.device_id]
        : w.kind === "measure" && w.measure.kind === "device"
          ? [w.measure.device_id]
          : []
    ),
  ]
  // The rules of the installation chosen: those that touch it, and those of the modules alone.
  const list = (automations.data ?? []).filter((a) => {
    const zones = used(a).flatMap((id) => installationOf.get(id) ?? [])
    return zones.length === 0 || zones.some((i) => ids.has(i))
  })
  const canEdit = (a: Automation) =>
    steps(a).every((s) =>
      s.kind === "device"
        ? drivable.has(s.device_id)
        : s.kind === "scene"
          ? home.scenes.some((r) => r.id === s.routine_id)
          : true
    )
  const ready = devices.isSuccess && traits.isSuccess
  const canCreate = ready && (drivable.size > 0 || home.scenes.length > 0)
  const open = (initial: AutomationInput, id?: number) =>
    setEditing((e) => ({ id, initial, nonce: (e?.nonce ?? 0) + 1 }))

  return (
    <section className="grid gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-lg font-semibold">{t("Automatismes")}</h2>
          <p className="text-sm text-muted-foreground">
            {t(
              "Quand quelque chose se produit, Breezone agit tout seul : surplus solaire, température, heure, soleil."
            )}
          </p>
        </div>
        {canCreate && (
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" onClick={() => setChoosing(true)}>
              <BookOpen /> {t("Recettes")}
            </Button>
            <Button onClick={() => open(blank(home))}>
              <Plus /> {t("Nouvel automatisme")}
            </Button>
          </div>
        )}
      </div>
      <AutomationsBar admin={admin} />
      {admin && <UnreadableAutomations />}
      {automations.isLoading ? (
        <div className="grid gap-4 md:grid-cols-2">
          <Skeleton className="h-40 rounded-3xl" />
          <Skeleton className="h-40 rounded-3xl" />
        </div>
      ) : list.length ? (
        <div className="grid gap-4 md:grid-cols-2">
          {list.map((a) => (
            <AutomationCard
              key={a.id}
              automation={a}
              onEdit={
                ready && canEdit(a) ? () => open(inputOf(a), a.id) : undefined
              }
              onDuplicate={
                ready && canEdit(a)
                  ? () =>
                      open({
                        ...inputOf(a),
                        name: t("{name} (copie)", { name: a.name }),
                      })
                  : undefined
              }
            />
          ))}
        </div>
      ) : (
        <p className="rounded-3xl border border-dashed p-6 text-center text-sm text-muted-foreground">
          {t(
            "Aucun automatisme. Partez d'une recette, ou par exemple : quand l'injection au réseau dépasse 500 W pendant 5 min, si le salon est sous 18 °C de novembre à mars, alors allumer le salon à 21°."
          )}
        </p>
      )}
      {choosing && (
        <RecipesDialog
          home={home}
          onClose={() => setChoosing(false)}
          onPick={(rule) => {
            setChoosing(false)
            open(rule)
          }}
        />
      )}
      {editing && (
        <AutomationEditor
          key={editing.nonce}
          home={home}
          id={editing.id}
          initial={editing.initial}
          onClose={() => setEditing(null)}
        />
      )}
    </section>
  )
}

export {
  DeviceActionEditor,
  describeDeviceAction,
  drivableDevices,
  newDeviceAction,
} from "./steps"
