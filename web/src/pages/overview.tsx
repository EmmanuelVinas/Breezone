import { useMemo, useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  closestCenter,
  DndContext,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
} from "@dnd-kit/core"
import type { DragEndEvent } from "@dnd-kit/core"
import {
  arrayMove,
  rectSortingStrategy,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
} from "@dnd-kit/sortable"
import { CSS } from "@dnd-kit/utilities"
import { GripVertical, Pencil, Plus, Settings2, X } from "lucide-react"
import { toast } from "sonner"
import type {
  DashboardBlock,
  DashboardLayout,
  DashboardSize,
  DashboardView,
} from "@/lib/api"
import { api } from "@/lib/api"
import { errorText } from "@/lib/format"
import { t, tn } from "@/lib/i18n"
import {
  useChosenInstallation,
  useModulesHere,
} from "@/lib/selected-installation"
import { cn } from "@/lib/utils"
import { PageHeader } from "@/components/app-shell"
import { BlockView, OverviewContext } from "@/components/overview/blocks"
import type { OverviewData } from "@/components/overview/blocks"
import { generate, SIZE_LABEL } from "@/components/overview/model"
import { BlockDialog } from "@/components/overview/settings"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

const SPAN: Record<DashboardSize, string> = {
  quarter: "lg:col-span-1",
  half: "sm:col-span-2 lg:col-span-2",
  full: "sm:col-span-2 lg:col-span-4",
}

/** A block, with its handles while the overview is being edited. */
function Slot({
  block,
  editing,
  onSize,
  onSettings,
  onRemove,
}: {
  block: DashboardBlock
  editing: boolean
  onSize: (s: DashboardSize) => void
  onSettings: () => void
  onRemove: () => void
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: block.id, disabled: !editing })
  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      className={cn(
        "relative min-w-0",
        SPAN[block.size as DashboardSize] ?? SPAN.half,
        isDragging && "z-10 opacity-80",
        editing &&
          "rounded-3xl ring-2 ring-primary/40 ring-offset-2 ring-offset-background"
      )}
    >
      {editing && (
        <div className="absolute -top-4 right-4 z-10 flex items-center gap-1 rounded-full border bg-background/95 p-1 shadow-sm">
          <button
            type="button"
            className="cursor-grab rounded-full p-1 text-muted-foreground hover:text-foreground active:cursor-grabbing"
            aria-label={t("Déplacer")}
            {...attributes}
            {...listeners}
          >
            <GripVertical className="size-4" />
          </button>
          <ToggleGroup
            type="single"
            size="sm"
            value={block.size}
            onValueChange={(v) => v && onSize(v as DashboardSize)}
          >
            {(Object.keys(SIZE_LABEL) as DashboardSize[]).map((s) => (
              <ToggleGroupItem key={s} value={s} className="h-7 px-2 text-xs">
                {s === "full" ? "1" : SIZE_LABEL[s]}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
          <Button
            variant="ghost"
            size="icon"
            className="size-7"
            onClick={onSettings}
            aria-label={t("Réglages du bloc")}
          >
            <Settings2 />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="size-7"
            onClick={onRemove}
            aria-label={t("Retirer")}
          >
            <X />
          </Button>
        </div>
      )}
      <div
        className={cn("h-full", editing && "pointer-events-none select-none")}
      >
        <BlockView block={block} editing={editing} />
      </div>
    </div>
  )
}

export function OverviewPage() {
  const qc = useQueryClient()
  const installation = useChosenInstallation()
  const modulesHere = useModulesHere()
  const id = installation?.id
  const dashboard = useQuery({
    queryKey: ["dashboard", id],
    queryFn: () => api.dashboard(id!),
    enabled: id !== undefined,
  })
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

  const [by, setBy] = useState<"room" | "type">("room")
  const [draft, setDraft] = useState<DashboardBlock[] | null>(null)
  const [dialog, setDialog] = useState<{ block?: DashboardBlock } | null>(null)
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates })
  )

  // The devices of this installation, and those of the modules of this server.
  const mine = useMemo(
    () =>
      (devices.data ?? []).filter((d) =>
        d.installation_id === null ? modulesHere : d.installation_id === id
      ),
    [devices.data, id, modulesHere]
  )
  const generated = useMemo(
    () =>
      generate(by, {
        devices: mine,
        rooms: rooms.data ?? [],
        roles: live.data?.roles ?? {},
        surplus: (surplus.data?.loads.length ?? 0) > 0,
        scenes: (routines.data?.length ?? 0) > 0,
      }),
    [by, mine, rooms.data, live.data?.roles, surplus.data, routines.data]
  )
  const view = dashboard.data
  const blocks = draft ?? view?.layout?.blocks ?? generated.blocks
  const editing = draft !== null

  const done = (v: DashboardView, message: string) => {
    qc.setQueryData(["dashboard", id], v)
    setDraft(null)
    toast.success(message)
  }
  const layout = (): DashboardLayout => ({ version: 1, blocks: draft ?? [] })
  const saveMine = useMutation({
    mutationFn: () => api.saveDashboard(id!, layout()),
    onSuccess: (v) => done(v, t("Vue enregistrée pour vous")),
    onError: (e) => toast.error(errorText(e)),
  })
  const saveDefault = useMutation({
    mutationFn: () => api.saveDefaultDashboard(id!, layout()),
    onSuccess: (v) => done(v, t("Vue enregistrée pour tous")),
    onError: (e) => toast.error(errorText(e)),
  })
  const reset = useMutation({
    mutationFn: () => api.resetDashboard(id!),
    onSuccess: (v) => done(v, t("Vue par défaut rétablie")),
    onError: (e) => toast.error(errorText(e)),
  })

  const change = (blockId: string, patch: Partial<DashboardBlock>) =>
    setDraft((d) =>
      (d ?? []).map((b) => (b.id === blockId ? { ...b, ...patch } : b))
    )
  const onDragEnd = (e: DragEndEvent) => {
    if (!e.over || e.active.id === e.over.id) return
    setDraft((d) => {
      const list = d ?? []
      const from = list.findIndex((b) => b.id === e.active.id)
      const to = list.findIndex((b) => b.id === e.over!.id)
      return arrayMove(list, from, to)
    })
  }

  if (!installation)
    return (
      <PageHeader
        title={t("Vue d'ensemble")}
        description={t(
          "Ajoutez une installation pour composer sa vue d'ensemble."
        )}
      />
    )

  const data: OverviewData = {
    installation,
    devices: mine,
    rooms: rooms.data ?? [],
    live: live.data,
    today: today.data,
    surplus: surplus.data,
    routines: routines.data ?? [],
    traits: traits.data ?? [],
    charted: charted.data ?? [],
  }
  const busy = saveMine.isPending || saveDefault.isPending || reset.isPending
  const source =
    view?.source === "mine"
      ? t("Votre vue")
      : view?.source === "installation"
        ? t("Vue de l'installation")
        : t("Vue proposée")

  return (
    <OverviewContext.Provider value={data}>
      <PageHeader
        title={installation.name}
        description={[
          installation.location,
          tn(data.rooms.length, "{n} pièce", "{n} pièces"),
          tn(mine.length, "{n} appareil", "{n} appareils"),
          source,
        ]
          .filter(Boolean)
          .join(" · ")}
        actions={
          editing ? (
            <div className="flex flex-wrap gap-2">
              <Button variant="outline" size="sm" onClick={() => setDialog({})}>
                <Plus /> {t("Ajouter un bloc")}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setDraft(null)}
                disabled={busy}
              >
                {t("Annuler")}
              </Button>
              {view?.can_set_default && (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => saveDefault.mutate()}
                  disabled={busy}
                >
                  {t("Enregistrer pour tous")}
                </Button>
              )}
              <Button
                size="sm"
                onClick={() => saveMine.mutate()}
                disabled={busy}
              >
                {t("Enregistrer pour moi")}
              </Button>
            </div>
          ) : (
            <div className="flex flex-wrap items-center gap-2">
              {view?.source === "generated" && (
                <ToggleGroup
                  type="single"
                  variant="outline"
                  size="sm"
                  value={by}
                  onValueChange={(v) => v && setBy(v as "room" | "type")}
                >
                  <ToggleGroupItem value="room">
                    {t("Par pièce")}
                  </ToggleGroupItem>
                  <ToggleGroupItem value="type">
                    {t("Par type")}
                  </ToggleGroupItem>
                </ToggleGroup>
              )}
              {view?.source === "mine" && view.has_default && (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => reset.mutate()}
                  disabled={busy}
                >
                  {t("Revenir à la vue de l'installation")}
                </Button>
              )}
              <Button
                variant="outline"
                size="sm"
                onClick={() => setDraft(blocks)}
              >
                <Pencil /> {t("Modifier")}
              </Button>
            </div>
          )
        }
      />
      {dashboard.isLoading || devices.isLoading ? (
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
          {[0, 1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-32 rounded-3xl" />
          ))}
        </div>
      ) : (
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragEnd={onDragEnd}
        >
          <SortableContext
            items={blocks.map((b) => b.id)}
            strategy={rectSortingStrategy}
          >
            <div className="grid grid-flow-dense gap-4 sm:grid-cols-2 lg:grid-cols-4">
              {blocks.map((b) => (
                <Slot
                  key={b.id}
                  block={b}
                  editing={editing}
                  onSize={(size) => change(b.id, { size })}
                  onSettings={() => setDialog({ block: b })}
                  onRemove={() =>
                    setDraft((d) => (d ?? []).filter((x) => x.id !== b.id))
                  }
                />
              ))}
            </div>
          </SortableContext>
        </DndContext>
      )}
      {editing && blocks.length === 0 && (
        <p className="rounded-3xl border border-dashed p-10 text-center text-sm text-muted-foreground">
          {t("Ajoutez un bloc pour composer la vue.")}
        </p>
      )}
      {dialog && (
        <BlockDialog
          key={dialog.block?.id ?? "new"}
          initial={dialog.block}
          home={data}
          onClose={() => setDialog(null)}
          onDone={(b) => {
            setDraft((d) =>
              dialog.block
                ? (d ?? []).map((x) => (x.id === b.id ? b : x))
                : [...(d ?? []), b]
            )
            setDialog(null)
          }}
        />
      )}
    </OverviewContext.Provider>
  )
}
