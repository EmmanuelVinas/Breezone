import { useState } from "react"
import { Link } from "react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { toast } from "sonner"
import { Blinds, ChevronDown, ChevronUp, Square } from "lucide-react"
import type { DeviceView, Room } from "@/lib/api"
import { api, currentUser } from "@/lib/api"
import type { CoverState } from "@/lib/covers"
import { coverKind, coverState, coverText, isCover, isOpen } from "@/lib/covers"
import { t, tn } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import { PageHeader } from "@/components/app-shell"
import { RoomSelect } from "@/components/room-select"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { Slider } from "@/components/ui/slider"

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e))

type Command = { id: number; action: "set" | "stop"; position?: number }

function useCommand() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (cmds: Command[]) =>
      Promise.all(
        cmds.map((c) =>
          api.act(
            c.id,
            "cover.position",
            c.action,
            c.action === "set" ? { position: c.position } : {}
          )
        )
      ),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["devices"] }),
    onError: (e) => toast.error(errorText(e)),
  })
}

/** A window with its shutter down to what is closed: the sky shows above the slats. */
function Window({ s, className }: { s: CoverState; className?: string }) {
  const open = Math.min(100, Math.max(0, s.position ?? 0))
  return (
    <div
      className={cn(
        "relative overflow-hidden rounded-xl border-4 border-muted bg-gradient-to-b from-sky-400/70 via-sky-300/40 to-emerald-700/50",
        className
      )}
    >
      <div
        className="absolute inset-x-0 top-0 bg-[repeating-linear-gradient(180deg,var(--color-slate-300)_0_5px,var(--color-slate-400)_5px_7px)] transition-[height] duration-700 dark:bg-[repeating-linear-gradient(180deg,var(--color-slate-400)_0_5px,var(--color-slate-500)_5px_7px)]"
        style={{ height: `${100 - open}%` }}
      />
    </div>
  )
}

function Controls({
  device,
  command,
  disabled,
  size = "icon",
}: {
  device: DeviceView
  command: ReturnType<typeof useCommand>
  disabled: boolean
  size?: "icon" | "default"
}) {
  const labels = size === "default"
  return (
    <div className={cn("flex gap-2", labels ? "w-full" : "justify-center")}>
      <Button
        variant="outline"
        size={size}
        className={cn(labels && "flex-1")}
        disabled={disabled}
        aria-label={t("Ouvrir")}
        onClick={(e) => {
          e.stopPropagation()
          command.mutate([{ id: device.id, action: "set", position: 100 }])
        }}
      >
        <ChevronUp /> {labels && t("Ouvrir")}
      </Button>
      <Button
        variant="outline"
        size={size}
        className={cn(labels && "flex-1")}
        disabled={disabled}
        aria-label={t("Stop")}
        onClick={(e) => {
          e.stopPropagation()
          command.mutate([{ id: device.id, action: "stop" }])
        }}
      >
        <Square className="fill-current" /> {labels && t("Stop")}
      </Button>
      <Button
        variant="outline"
        size={size}
        className={cn(labels && "flex-1")}
        disabled={disabled}
        aria-label={t("Fermer")}
        onClick={(e) => {
          e.stopPropagation()
          command.mutate([{ id: device.id, action: "set", position: 0 }])
        }}
      >
        <ChevronDown /> {labels && t("Fermer")}
      </Button>
    </div>
  )
}

function ShutterCard({
  device,
  selected,
  onSelect,
  canDrive,
}: {
  device: DeviceView
  selected: boolean
  onSelect: () => void
  canDrive: boolean
}) {
  const command = useCommand()
  const s = coverState(device)
  const moving = s.moving === "opening" || s.moving === "closing"
  return (
    <Card
      className={cn(
        "cursor-pointer gap-3 rounded-3xl py-4 transition-shadow",
        selected && "ring-2 ring-primary"
      )}
      onClick={onSelect}
    >
      <CardContent className="grid gap-3 px-4">
        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0">
            <b className="block truncate">{device.name}</b>
            <span
              className={cn(
                "text-xs text-muted-foreground",
                moving && "text-primary"
              )}
            >
              {device.available ? coverText(s) : t("Indisponible")}
            </span>
          </div>
          <span className="shrink-0 text-xs text-muted-foreground">
            {coverKind(device)}
          </span>
        </div>
        <Window s={s} className="mx-auto aspect-[4/3] w-28" />
        <Controls
          device={device}
          command={command}
          disabled={!canDrive || !device.available || command.isPending}
        />
      </CardContent>
    </Card>
  )
}

function Detail({
  device,
  rooms,
  canDrive,
  admin,
}: {
  device: DeviceView
  rooms: Room[]
  canDrive: boolean
  admin: boolean
}) {
  const command = useCommand()
  const s = coverState(device)
  const [draft, setDraft] = useState<number | null>(null)
  const position = draft ?? s.position ?? 0
  const room = rooms.find((r) => r.id === device.room_id)
  const disabled = !canDrive || !device.available || command.isPending
  return (
    <Card className="rounded-3xl lg:sticky lg:top-4">
      <CardHeader>
        <CardTitle className="truncate">{device.name}</CardTitle>
        <CardDescription>
          {[room?.name, coverKind(device)].filter(Boolean).join(" · ")}
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-5">
        <Window s={{ position }} className="aspect-[4/3] w-full" />
        <div className="grid gap-2">
          <div className="flex items-baseline justify-between text-sm">
            <span className="text-muted-foreground">{t("Position")}</span>
            <span className="tabular font-medium">
              {draft !== null
                ? t("Ouvert à {n} %", { n: draft })
                : device.available
                  ? coverText(s)
                  : t("Indisponible")}
            </span>
          </div>
          <Slider
            min={0}
            max={100}
            step={1}
            value={[position]}
            disabled={disabled}
            onValueChange={([v]) => setDraft(v)}
            onValueCommit={([v]) =>
              command.mutate([{ id: device.id, action: "set", position: v }], {
                onSettled: () => setDraft(null),
              })
            }
            aria-label={t("Ouverture de {name}", { name: device.name })}
          />
          <div className="flex justify-between text-xs text-muted-foreground">
            <span>{t("Fermé")}</span>
            <span>{t("Ouvert")}</span>
          </div>
        </div>
        <Controls
          device={device}
          command={command}
          disabled={disabled}
          size="default"
        />
        {admin && (
          <div className="grid gap-1.5">
            <span className="text-xs text-muted-foreground">{t("Pièce")}</span>
            <RoomSelect
              deviceId={device.id}
              roomId={device.room_id}
              rooms={rooms}
            />
          </div>
        )}
      </CardContent>
    </Card>
  )
}

export function ShuttersPage() {
  const admin = currentUser()?.role === "admin"
  const devices = useQuery({
    queryKey: ["devices"],
    queryFn: api.devices,
    // Faster while one moves: its position shows as it goes.
    refetchInterval: (q) =>
      (q.state.data ?? []).some(
        (d) =>
          isCover(d) &&
          coverState(d).moving &&
          coverState(d).moving !== "stopped"
      )
        ? 1_500
        : 5_000,
  })
  const rooms = useQuery({ queryKey: ["rooms"], queryFn: api.rooms })
  const [selected, setSelected] = useState<number | null>(null)
  const command = useCommand()
  const all = (devices.data ?? []).filter(isCover)
  const open = all.filter(isOpen)
  const usable = all.filter((d) => d.available)
  const groups: { room: Room | null; covers: DeviceView[] }[] = [
    ...(rooms.data ?? []).map((r) => ({
      room: r,
      covers: all.filter((d) => d.room_id === r.id),
    })),
    { room: null, covers: all.filter((d) => d.room_id === null) },
  ].filter((g) => g.covers.length > 0)
  const shown = all.find((d) => d.id === selected) ?? all[0]
  const every = (list: DeviceView[], position: number) =>
    command.mutate(
      list
        .filter((d) => d.available)
        .map((d) => ({ id: d.id, action: "set" as const, position }))
    )

  return (
    <>
      <PageHeader
        title={t("Volets")}
        description={
          all.length
            ? t("{open} ouvert(s) sur {total}", {
                open: open.length,
                total: all.length,
              })
            : undefined
        }
        actions={
          all.length > 0 && (
            <div className="flex gap-2">
              <Button
                variant="outline"
                size="sm"
                disabled={!admin || command.isPending || usable.length === 0}
                onClick={() => every(all, 100)}
              >
                <ChevronUp /> {t("Tout ouvrir")}
              </Button>
              <Button
                variant="outline"
                size="sm"
                disabled={!admin || command.isPending || usable.length === 0}
                onClick={() => every(all, 0)}
              >
                <ChevronDown /> {t("Tout fermer")}
              </Button>
            </div>
          )
        }
      />
      {devices.isLoading ? (
        <Skeleton className="h-72 w-full rounded-3xl" />
      ) : all.length === 0 ? (
        <Card className="rounded-3xl">
          <CardContent className="flex flex-col items-center gap-3 py-12 text-center">
            <span className="grid size-12 place-items-center rounded-2xl bg-primary/15 text-primary">
              <Blinds className="size-6" />
            </span>
            <div className="font-heading text-lg font-semibold">
              {t("Aucun volet")}
            </div>
            <p className="max-w-md text-sm text-muted-foreground">
              {t(
                "Appairez votre passerelle Legrand ou VELUX dans Réglages › Modules, avec son code HomeKit."
              )}
            </p>
            {admin && (
              <Button asChild>
                <Link to="/settings?tab=modules">
                  {t("Ouvrir Réglages › Modules")}
                </Link>
              </Button>
            )}
          </CardContent>
        </Card>
      ) : (
        <div className="grid items-start gap-5 lg:grid-cols-[minmax(0,1fr)_340px]">
          <div className="grid gap-6">
            {groups.map((g) => {
              const opened = g.covers.filter(isOpen)
              return (
                <section key={g.room?.id ?? "none"} className="grid gap-3">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="flex items-baseline gap-2">
                      <h2 className="font-heading text-lg font-semibold">
                        {g.room?.name ?? t("Sans pièce")}
                      </h2>
                      <span className="text-sm text-muted-foreground">
                        {tn(g.covers.length, "{n} volet", "{n} volets")} ·{" "}
                        {tn(opened.length, "{n} ouvert", "{n} ouverts")}
                      </span>
                    </div>
                    <div className="flex gap-1">
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={!admin || command.isPending}
                        onClick={() => every(g.covers, 100)}
                      >
                        <ChevronUp /> {t("Tout ouvrir")}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={!admin || command.isPending}
                        onClick={() => every(g.covers, 0)}
                      >
                        <ChevronDown /> {t("Tout fermer")}
                      </Button>
                    </div>
                  </div>
                  <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
                    {g.covers.map((d) => (
                      <ShutterCard
                        key={d.id}
                        device={d}
                        selected={shown?.id === d.id}
                        onSelect={() => setSelected(d.id)}
                        canDrive={admin}
                      />
                    ))}
                  </div>
                </section>
              )
            })}
          </div>
          {shown && (
            <Detail
              key={shown.id}
              device={shown}
              rooms={rooms.data ?? []}
              canDrive={admin}
              admin={admin}
            />
          )}
        </div>
      )}
    </>
  )
}
