import { useState, type ReactNode } from "react"
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
  DashboardPage,
  DashboardSize,
  DashboardView,
} from "@/lib/api"
import { api } from "@/lib/api"
import { errorText } from "@/lib/format"
import { t } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import { PageHeader } from "@/components/app-shell"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { BlockView } from "./blocks"
import { OverviewContext } from "./context"
import type { OverviewData } from "./context"
import { SIZE_LABEL } from "./model"
import { BlockDialog } from "./settings"

const SPAN: Record<DashboardSize, string> = {
  quarter: "lg:col-span-1",
  half: "sm:col-span-2 lg:col-span-2",
  full: "sm:col-span-2 lg:col-span-4",
}

/** A block, with its handles while the page is being edited. */
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
        // On a phone, one block per row: an empty cell would only leave a gap.
        block.type === "spacer" && !editing && "hidden sm:block",
        editing &&
          "rounded-3xl outline-2 outline-offset-4 outline-primary/60 outline-dashed"
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

/**
 * A page of an installation as a dashboard of blocks: the layout the user made, else the one its
 * administrators set, else the one composed from the home (`generated`). "Modifier" lets one
 * arrange it, for oneself or (administrators) for everyone.
 */
export function Board({
  page,
  title,
  description,
  data,
  generated,
  shows,
  actions,
  loading,
  aside,
  intro,
}: {
  page: DashboardPage
  title: string
  /** Under the title; the source of the layout is added to it. */
  description?: (string | null | undefined)[]
  data: OverviewData
  generated: DashboardBlock[]
  /** Out of the edit mode, whether a block has something to show here. */
  shows?: (b: DashboardBlock) => boolean
  /** Shown beside "Modifier" out of the edit mode. */
  actions?: (source: DashboardView["source"] | undefined) => ReactNode
  loading?: boolean
  /** Beside the blocks on a large screen (the detail of the device chosen), kept while arranging. */
  aside?: ReactNode
  /** Between the title and the blocks (the installation of the climate page, its modes). */
  intro?: ReactNode
}) {
  const qc = useQueryClient()
  // Without an installation there is nowhere to keep a layout: the page as it comes.
  const id = data.installation?.id ?? 0
  const key = ["dashboard", id, page]
  const dashboard = useQuery({
    queryKey: key,
    queryFn: () => api.dashboard(id, page),
    enabled: data.installation !== undefined,
  })
  const [draft, setDraft] = useState<DashboardBlock[] | null>(null)
  const [dialog, setDialog] = useState<{ block?: DashboardBlock } | null>(null)
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates })
  )
  const view = dashboard.data
  const editing = draft !== null
  const blocks = (draft ?? view?.layout?.blocks ?? generated).filter(
    (b) => editing || !shows || shows(b)
  )

  const done = (v: DashboardView, message: string) => {
    qc.setQueryData(key, v)
    setDraft(null)
    toast.success(message)
  }
  const layout = (): DashboardLayout => ({ version: 1, blocks: draft ?? [] })
  const saveMine = useMutation({
    mutationFn: () => api.saveDashboard(id, layout(), page),
    onSuccess: (v) => done(v, t("Vue enregistrée pour vous")),
    onError: (e) => toast.error(errorText(e)),
  })
  const saveDefault = useMutation({
    mutationFn: () => api.saveDefaultDashboard(id, layout(), page),
    onSuccess: (v) => done(v, t("Vue enregistrée pour tous")),
    onError: (e) => toast.error(errorText(e)),
  })
  const reset = useMutation({
    mutationFn: () => api.resetDashboard(id, page),
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
        title={title}
        description={[...(description ?? []), source]
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
              {actions?.(view?.source)}
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
              {data.installation && (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setDraft(blocks)}
                >
                  <Pencil /> {t("Modifier")}
                </Button>
              )}
            </div>
          )
        }
      />
      {intro}
      {(dashboard.isLoading && data.installation) || loading ? (
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
            <div
              className={cn(
                aside &&
                  "grid items-start gap-5 lg:grid-cols-[minmax(0,1fr)_340px]"
              )}
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
              {aside}
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
          page={page}
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
