import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { ArrowDown, ArrowUp, House, Loader2, Plus, Trash2 } from "lucide-react"
import { toast } from "sonner"
import type { DeviceView, Room } from "@/lib/api"
import { api } from "@/lib/api"
import { t, tn } from "@/lib/i18n"
import { Confirm } from "@/components/confirm"
import { RoomSelect } from "@/components/room-select"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Skeleton } from "@/components/ui/skeleton"
import { Switch } from "@/components/ui/switch"

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e))

/** A room of the house: renamed in place, moved up or down, deleted. */
function RoomRow({
  room,
  index,
  rooms,
  count,
}: {
  room: Room
  index: number
  rooms: Room[]
  count: number
}) {
  const qc = useQueryClient()
  const [draft, setDraft] = useState<string | null>(null)
  const refresh = () => {
    qc.invalidateQueries({ queryKey: ["rooms"] })
    qc.invalidateQueries({ queryKey: ["devices"] })
  }
  const rename = useMutation({
    mutationFn: (name: string) =>
      api.updateRoom(room.id, { name, position: index }),
    onSuccess: refresh,
    onError: (e) => toast.error(errorText(e)),
  })
  // Positions follow the order shown, both rooms swapped.
  const move = useMutation({
    mutationFn: async (to: number) => {
      const order = [...rooms]
      ;[order[index], order[to]] = [order[to], order[index]]
      await Promise.all(
        order.map((r, i) => api.updateRoom(r.id, { name: r.name, position: i }))
      )
    },
    onSuccess: refresh,
    onError: (e) => toast.error(errorText(e)),
  })
  const remove = useMutation({
    mutationFn: () => api.deleteRoom(room.id),
    onSuccess: () => {
      toast.success(t("« {name} » supprimée", { name: room.name }))
      refresh()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  return (
    <div className="flex items-center gap-2 rounded-2xl border p-2 pl-3">
      <House className="size-4 shrink-0 text-muted-foreground" />
      <Input
        value={draft ?? room.name}
        maxLength={60}
        aria-label={t("Nom de la pièce")}
        className="h-8 min-w-0 flex-1 font-medium"
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => {
          if (draft !== null && draft.trim() && draft.trim() !== room.name)
            rename.mutate(draft.trim())
          setDraft(null)
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur()
        }}
      />
      <span className="hidden shrink-0 text-xs text-muted-foreground sm:inline">
        {tn(count, "{n} objet", "{n} objets")}
      </span>
      <Button
        variant="ghost"
        size="icon"
        disabled={index === 0 || move.isPending}
        aria-label={t("Monter dans la liste")}
        onClick={() => move.mutate(index - 1)}
      >
        <ArrowUp />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        disabled={index === rooms.length - 1 || move.isPending}
        aria-label={t("Descendre dans la liste")}
        onClick={() => move.mutate(index + 1)}
      >
        <ArrowDown />
      </Button>
      <Confirm
        title={t("Supprimer « {name} » ?", { name: room.name })}
        description={t(
          "Ses objets restent, sans pièce : vous pourrez les ranger ailleurs."
        )}
        onConfirm={() => remove.mutate()}
      >
        <Button variant="ghost" size="icon" aria-label={t("Supprimer")}>
          <Trash2 />
        </Button>
      </Confirm>
    </div>
  )
}

function NewRoom({ position }: { position: number }) {
  const qc = useQueryClient()
  const [name, setName] = useState("")
  const create = useMutation({
    mutationFn: () => api.createRoom({ name: name.trim(), position }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["rooms"] })
      setName("")
    },
    onError: (e) => toast.error(errorText(e)),
  })
  return (
    <form
      className="flex gap-2"
      onSubmit={(e) => {
        e.preventDefault()
        if (name.trim()) create.mutate()
      }}
    >
      <Input
        value={name}
        maxLength={60}
        placeholder={t("Salon, Chambre parents…")}
        aria-label={t("Nom de la pièce")}
        onChange={(e) => setName(e.target.value)}
      />
      <Button type="submit" disabled={!name.trim() || create.isPending}>
        <Plus /> {t("Ajouter")}
      </Button>
    </form>
  )
}

/** The objects of one module (an Airzone installation, a Hue bridge…), each with its room. */
function ModuleObjects({
  title,
  devices,
  rooms,
  hueSource,
}: {
  title: string
  devices: DeviceView[]
  rooms: Room[]
  hueSource?: number
}) {
  const qc = useQueryClient()
  const take = useMutation({
    mutationFn: () => api.takeRooms(hueSource!),
    onSuccess: (r) => {
      qc.invalidateQueries({ queryKey: ["devices"] })
      qc.invalidateQueries({ queryKey: ["rooms"] })
      toast.success(
        r.placed === 0 && r.created.length === 0
          ? t("Toutes les lumières ont déjà une pièce")
          : t("{placed} lumière(s) rangée(s), {created} pièce(s) créée(s)", {
              placed: r.placed,
              created: r.created.length,
            })
      )
    },
    onError: (e) => toast.error(errorText(e)),
  })
  return (
    <section className="grid gap-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="font-heading font-semibold">{title}</h3>
        {hueSource !== undefined && (
          <Button
            variant="outline"
            size="sm"
            disabled={take.isPending}
            onClick={() => take.mutate()}
          >
            {take.isPending ? <Loader2 className="animate-spin" /> : <House />}
            {t("Reprendre les pièces Hue")}
          </Button>
        )}
      </div>
      {devices.map((d) => (
        <div
          key={d.id}
          className="flex flex-wrap items-center justify-between gap-2 rounded-2xl border p-2 pl-3"
        >
          <span className="min-w-0 truncate text-sm font-medium">{d.name}</span>
          <RoomSelect
            deviceId={d.id}
            roomId={d.room_id}
            rooms={rooms}
            className="w-48"
          />
        </div>
      ))}
    </section>
  )
}

/** Settings › Rooms: the rooms of the house, and the room of each object of each module. */
export function RoomsTab() {
  const rooms = useQuery({ queryKey: ["rooms"], queryFn: api.rooms })
  const devices = useQuery({ queryKey: ["devices"], queryFn: api.devices })
  const sources = useQuery({ queryKey: ["sources"], queryFn: api.sources })
  const installations = useQuery({
    queryKey: ["installations"],
    queryFn: api.installations,
  })
  const [unplaced, setUnplaced] = useState(false)
  if (rooms.isLoading || devices.isLoading)
    return <Skeleton className="h-72 w-full rounded-3xl" />
  const list = rooms.data ?? []
  const all = (devices.data ?? []).filter((d) => !unplaced || !d.room_id)
  // One group per Airzone installation and per module, in the order they come.
  const groups: {
    key: string
    title: string
    devices: DeviceView[]
    hue?: number
  }[] = []
  for (const d of all) {
    const key = d.source_id
      ? `s${d.source_id}`
      : `i${d.installation_id ?? "none"}`
    let g = groups.find((x) => x.key === key)
    if (!g) {
      const source = sources.data?.find((s) => s.id === d.source_id)
      const inst = installations.data?.find((i) => i.id === d.installation_id)
      g = {
        key,
        title: source
          ? source.name
          : inst
            ? t("Airzone · {name}", { name: inst.name })
            : "Airzone",
        devices: [],
        hue: source?.driver === "hue" ? source.id : undefined,
      }
      groups.push(g)
    }
    g.devices.push(d)
  }
  const counts = new Map<number, number>()
  for (const d of devices.data ?? [])
    if (d.room_id) counts.set(d.room_id, (counts.get(d.room_id) ?? 0) + 1)

  return (
    <div className="grid items-start gap-6 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]">
      <Card className="rounded-3xl">
        <CardHeader>
          <CardTitle>{t("Pièces de la maison")}</CardTitle>
          <CardDescription>
            {t(
              "Les mêmes pièces pour tous les modules : climatisation, lumières, volets… L'ordre est celui des pages."
            )}
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-2">
          {list.length === 0 && (
            <p className="text-sm text-muted-foreground">
              {t("Aucune pièce pour l'instant.")}
            </p>
          )}
          {list.map((r, i) => (
            <RoomRow
              key={r.id}
              room={r}
              index={i}
              rooms={list}
              count={counts.get(r.id) ?? 0}
            />
          ))}
          <NewRoom position={list.length} />
        </CardContent>
      </Card>
      <Card className="rounded-3xl">
        <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-3">
          <div className="grid gap-1.5">
            <CardTitle>{t("Objets des modules")}</CardTitle>
            <CardDescription>
              {t(
                "Chaque module a ses propres objets, et parfois ses propres pièces : rangez-les dans celles de la maison."
              )}
            </CardDescription>
          </div>
          <label className="flex items-center gap-2 text-sm text-muted-foreground">
            <Switch checked={unplaced} onCheckedChange={setUnplaced} />
            {t("Sans pièce seulement")}
          </label>
        </CardHeader>
        <CardContent className="grid gap-6">
          {groups.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              {unplaced
                ? t("Tous les objets ont une pièce.")
                : t("Aucun objet pour l'instant : ajoutez un module.")}
            </p>
          ) : (
            groups.map((g) => (
              <ModuleObjects
                key={g.key}
                title={g.title}
                devices={g.devices}
                rooms={list}
                hueSource={g.hue}
              />
            ))
          )}
        </CardContent>
      </Card>
    </div>
  )
}
