import { useQuery } from "@tanstack/react-query"
import type { DeviceGrant, DeviceView } from "@/lib/api"
import { api } from "@/lib/api"
import { deviceType, DeviceTypeIcon, typeOrder } from "@/lib/device-icons"
import { t, tn } from "@/lib/i18n"
import { RoomIcon } from "@/lib/room-icons"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"

type Level = "none" | DeviceGrant["permission"]

function LevelPick({
  value,
  onChange,
  label,
}: {
  value: Level
  onChange: (v: Level) => void
  label: string
}) {
  return (
    <Select value={value} onValueChange={(v) => onChange(v as Level)}>
      <SelectTrigger className="w-40" aria-label={label}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="none">{t("Aucun accès")}</SelectItem>
        <SelectItem value="read">{t("Lecture seule")}</SelectItem>
        <SelectItem value="control">{t("Pilotage")}</SelectItem>
      </SelectContent>
    </Select>
  )
}

function Row({
  icon,
  label,
  sub,
  value,
  onChange,
}: {
  icon?: React.ReactNode
  label: string
  sub?: string
  value: Level
  onChange: (v: Level) => void
}) {
  return (
    <div className="flex flex-wrap items-center gap-3 rounded-xl border p-2.5 text-sm">
      {icon}
      <span className="min-w-0 flex-1">
        {label}
        {sub && (
          <span className="block text-xs text-muted-foreground">{sub}</span>
        )}
      </span>
      <LevelPick value={value} onChange={onChange} label={label} />
    </div>
  )
}

/**
 * What a user may see and drive among the devices of the modules (those of no installation): all
 * of them, by room, by type or one by one. The most a grant gives wins.
 */
export function DeviceAccessEditor({
  value,
  onChange,
}: {
  value: DeviceGrant[]
  onChange: (v: DeviceGrant[]) => void
}) {
  const devices = useQuery({ queryKey: ["devices"], queryFn: api.devices })
  const rooms = useQuery({ queryKey: ["rooms"], queryFn: api.rooms })
  const modules = (devices.data ?? []).filter((d) => d.installation_id === null)
  const level = (scope: DeviceGrant["scope"], ref = ""): Level =>
    value.find((g) => g.scope === scope && g.ref === ref)?.permission ?? "none"
  const set = (scope: DeviceGrant["scope"], ref: string, l: Level) =>
    onChange([
      ...value.filter((g) => !(g.scope === scope && g.ref === ref)),
      ...(l === "none" ? [] : [{ scope, ref, permission: l }]),
    ])

  if (modules.length === 0)
    return (
      <p className="text-sm text-muted-foreground">
        {t("Aucun appareil de module sur ce serveur.")}
      </p>
    )

  const types = [
    ...new Map(modules.map((d) => [deviceType(d).id, deviceType(d)])).values(),
  ].sort((a, b) => typeOrder(a) - typeOrder(b))
  const usedRooms = (rooms.data ?? []).filter((r) =>
    modules.some((d) => d.room_id === r.id)
  )
  const count = (pick: (d: DeviceView) => boolean) => {
    const n = modules.filter(pick).length
    return tn(n, "{n} appareil", "{n} appareils")
  }
  const sorted = [...modules].sort(
    (a, b) =>
      typeOrder(deviceType(a)) - typeOrder(deviceType(b)) ||
      a.name.localeCompare(b.name)
  )

  return (
    <div className="grid gap-4">
      <Row
        label={t("Tous les appareils des modules")}
        sub={count(() => true)}
        value={level("all")}
        onChange={(l) => set("all", "", l)}
      />
      <div className="grid gap-2">
        <div className="text-xs font-medium text-muted-foreground">
          {t("Par type")}
        </div>
        {types.map((ty) => (
          <Row
            key={ty.id}
            icon={
              <DeviceTypeIcon
                of={ty}
                className="size-4 text-muted-foreground"
                aria-hidden
              />
            }
            label={ty.label}
            sub={count((d) => deviceType(d).id === ty.id)}
            value={level("type", ty.id)}
            onChange={(l) => set("type", ty.id, l)}
          />
        ))}
      </div>
      {usedRooms.length > 0 && (
        <div className="grid gap-2">
          <div className="text-xs font-medium text-muted-foreground">
            {t("Par pièce")}
          </div>
          {usedRooms.map((r) => (
            <Row
              key={r.id}
              icon={
                <RoomIcon
                  room={r}
                  className="size-4 text-muted-foreground"
                  aria-hidden
                />
              }
              label={r.name}
              sub={count((d) => d.room_id === r.id)}
              value={level("room", String(r.id))}
              onChange={(l) => set("room", String(r.id), l)}
            />
          ))}
        </div>
      )}
      <details className="grid gap-2">
        <summary className="cursor-pointer text-xs font-medium text-muted-foreground">
          {t("Appareil par appareil")}
        </summary>
        <div className="mt-2 grid gap-2">
          {sorted.map((d) => (
            <Row
              key={d.id}
              icon={
                <DeviceTypeIcon
                  of={deviceType(d)}
                  className="size-4 text-muted-foreground"
                  aria-hidden
                />
              }
              label={d.name}
              sub={deviceType(d).label}
              value={level("device", String(d.id))}
              onChange={(l) => set("device", String(d.id), l)}
            />
          ))}
        </div>
      </details>
      <p className="text-xs text-muted-foreground">
        {t(
          "Le plus large l'emporte : « Lumières en pilotage » et « Salon en lecture seule » laissent piloter les lumières du salon. Les noms, les pièces et les réglages des appareils restent aux administrateurs. L'énergie de la maison est visible avec les types Énergie ou Solaire."
        )}
      </p>
    </div>
  )
}

/** "Tous les appareils en pilotage", "2 types · 1 pièce"… */
export function deviceAccessSummary(grants: DeviceGrant[]) {
  if (grants.length === 0) return t("aucun appareil de module")
  const all = grants.find((g) => g.scope === "all")
  const parts = [
    ...(all
      ? [
          all.permission === "control"
            ? t("tous les appareils en pilotage")
            : t("tous les appareils en lecture"),
        ]
      : []),
    ...(["type", "room", "device"] as const).flatMap((s) => {
      const n = grants.filter((g) => g.scope === s).length
      if (!n) return []
      return [
        s === "type"
          ? tn(n, "{n} type", "{n} types")
          : s === "room"
            ? tn(n, "{n} pièce", "{n} pièces")
            : tn(n, "{n} appareil", "{n} appareils"),
      ]
    }),
  ]
  return parts.join(" · ")
}
