import { useState } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
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
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable"
import { CSS } from "@dnd-kit/utilities"
import { GripVertical } from "lucide-react"
import { toast } from "sonner"
import type { Automation } from "@/lib/api"
import { api } from "@/lib/api"
import { errorText } from "@/lib/format"
import { t } from "@/lib/i18n"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

function Row({ a, rank }: { a: Automation; rank: number }) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: a.id })
  return (
    <li
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      className={`flex items-center gap-3 border-b bg-background py-2.5 last:border-b-0 ${isDragging ? "relative z-10 opacity-80" : ""}`}
    >
      <button
        type="button"
        className="cursor-grab touch-none text-muted-foreground"
        aria-label={t("Déplacer « {name} »", { name: a.name })}
        {...attributes}
        {...listeners}
      >
        <GripVertical className="size-4" />
      </button>
      <span className="grid size-7 shrink-0 place-items-center rounded-lg bg-muted text-xs text-muted-foreground">
        {rank}
      </span>
      <div className="grid min-w-0 flex-1">
        <span className="truncate text-sm font-medium">{a.name}</span>
        <span className="truncate text-xs text-muted-foreground">
          {a.summary.actions.join(" · ")}
        </span>
      </div>
    </li>
  )
}

/**
 * Ranks the automations by dragging them: the solar surplus goes to them in this order, and the
 * higher one wins a device two of them drive at the same time.
 */
export function PrioritiesDialog({
  automations,
  onClose,
}: {
  automations: Automation[]
  onClose: () => void
}) {
  const qc = useQueryClient()
  const [list, setList] = useState(() =>
    [...automations].sort((a, b) => a.priority - b.priority)
  )
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    })
  )
  const save = useMutation({
    mutationFn: () => api.orderAutomations(list.map((a) => a.id)),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["automations"] })
      qc.invalidateQueries({ queryKey: ["surplus"] })
      onClose()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const onDragEnd = (e: DragEndEvent) => {
    if (!e.over || e.active.id === e.over.id) return
    setList((l) => {
      const from = l.findIndex((a) => a.id === e.active.id)
      const to = l.findIndex((a) => a.id === e.over!.id)
      return arrayMove(l, from, to)
    })
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("Priorités")}</DialogTitle>
          <DialogDescription>
            {t(
              "Faites glisser pour ranger. Le surplus solaire va d'abord au premier ; quand deux automatismes pilotent le même appareil en même temps, le plus haut l'emporte."
            )}
          </DialogDescription>
        </DialogHeader>
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragEnd={onDragEnd}
        >
          <SortableContext
            items={list.map((a) => a.id)}
            strategy={verticalListSortingStrategy}
          >
            <ol className="grid">
              {list.map((a, i) => (
                <Row key={a.id} a={a} rank={i + 1} />
              ))}
            </ol>
          </SortableContext>
        </DndContext>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("Annuler")}
          </Button>
          <Button onClick={() => save.mutate()} disabled={save.isPending}>
            {t("Enregistrer")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
