import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Minus, Plus } from "lucide-react"
import { toast } from "sonner"
import type { DeviceView, TraitField, TraitInfo, TraitWidget } from "@/lib/api"
import { api } from "@/lib/api"
import { errorText } from "@/lib/format"
import { fieldOf, fieldText, traitOf } from "@/lib/generic-device"
import { lang, t } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Slider } from "@/components/ui/slider"
import { Switch } from "@/components/ui/switch"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

/** The widgets this client knows; any other shows its field as a value. */
const KNOWN = new Set([
  "value",
  "toggle",
  "slider",
  "stepper",
  "gauge",
  "enum",
  "button",
  "color",
])

type Piece = {
  trait: TraitInfo
  widget: TraitWidget
  field?: TraitField
}

/**
 * The pieces of a device's card from the descriptors of its traits (`ui.card`): the controls of
 * a kind of device this version of the web does not know, as the server describes them.
 */
export function cardPieces(
  d: Pick<DeviceView, "traits">,
  catalog: TraitInfo[] | undefined
): Piece[] {
  const out: Piece[] = []
  for (const id of d.traits) {
    const trait = traitOf(catalog, id)
    for (const widget of trait?.ui?.card ?? []) {
      // Pictures and charts belong to a detail, not to a line of a card.
      if (widget.widget === "picture" || widget.widget === "chart") continue
      const field = trait!.state.find((f) => f.name === widget.field)
      out.push({ trait: trait!, widget, field })
    }
  }
  return out
}

function label(field: TraitField | undefined, fallback: string) {
  return field?.label[lang()] ?? fallback
}

/** One piece: a value to read, or a control that sends the action of its trait. */
function PieceControl({ d, piece }: { d: DeviceView; piece: Piece }) {
  const qc = useQueryClient()
  const { trait, widget } = piece
  const state = (d.state ?? {}) as Record<string, unknown>
  const field = piece.field && fieldOf(piece.field, state)
  const value = widget.field ? state[widget.field] : undefined
  const act = useMutation({
    mutationFn: (params: Record<string, unknown>) =>
      api.act(d.id, trait.trait, widget.action ?? "set", params),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["devices"] }),
    onError: (e) => toast.error(errorText(e)),
  })
  const send = (v: unknown) =>
    act.mutate(widget.param ? { [widget.param]: v } : (widget.params ?? {}))
  const disabled = !d.can_control || !d.available || act.isPending
  const kind =
    KNOWN.has(widget.widget) && widget.action ? widget.widget : "value"
  const name = label(field, widget.field ?? widget.action ?? "")
  const [draft, setDraft] = useState<number | null>(null)

  switch (kind) {
    case "toggle":
      return (
        <label className="flex items-center justify-between gap-3 text-sm">
          <span className="text-muted-foreground">{name}</span>
          <Switch
            checked={value === true}
            disabled={disabled}
            loading={act.isPending}
            onCheckedChange={(v) => send(v)}
            aria-label={name}
          />
        </label>
      )
    case "slider": {
      const min = field?.min ?? 0
      const max = field?.max ?? 100
      const shown = draft ?? (typeof value === "number" ? value : min)
      return (
        <div className="grid gap-1.5 text-sm">
          <div className="flex justify-between gap-2">
            <span className="text-muted-foreground">{name}</span>
            <span>{field ? fieldText(field, shown) : shown}</span>
          </div>
          <Slider
            min={min}
            max={max}
            step={
              field?.step || (field?.type === "integer" ? 1 : (max - min) / 100)
            }
            value={[shown]}
            disabled={disabled}
            onValueChange={([v]) => setDraft(v)}
            onValueCommit={([v]) => {
              setDraft(null)
              send(v)
            }}
            aria-label={name}
          />
        </div>
      )
    }
    case "stepper": {
      const step = field?.step || (field?.type === "integer" ? 1 : 0.5)
      const current = typeof value === "number" ? value : (field?.min ?? 0)
      const clamp = (v: number) =>
        Math.min(field?.max ?? Infinity, Math.max(field?.min ?? -Infinity, v))
      return (
        <div className="flex items-center justify-between gap-3 text-sm">
          <span className="text-muted-foreground">{name}</span>
          <div className="flex items-center gap-2">
            <Button
              size="icon"
              variant="outline"
              className="size-8"
              disabled={disabled}
              onClick={() => send(clamp(current - step))}
              aria-label={t("Moins")}
            >
              <Minus />
            </Button>
            <span className="min-w-14 text-center font-medium">
              {field ? fieldText(field, current) : current}
            </span>
            <Button
              size="icon"
              variant="outline"
              className="size-8"
              disabled={disabled}
              onClick={() => send(clamp(current + step))}
              aria-label={t("Plus")}
            >
              <Plus />
            </Button>
          </div>
        </div>
      )
    }
    case "enum":
      return (
        <div className="grid gap-1.5 text-sm">
          <span className="text-muted-foreground">{name}</span>
          <ToggleGroup
            type="single"
            variant="outline"
            value={typeof value === "string" ? value : ""}
            onValueChange={(v) => v && send(v)}
            disabled={disabled}
            className="flex-wrap justify-start"
          >
            {(field?.values ?? []).map((v) => (
              <ToggleGroupItem
                key={v}
                value={v}
                className="data-[state=on]:border-primary data-[state=on]:bg-primary data-[state=on]:text-primary-foreground"
              >
                {field ? fieldText(field, v) : v}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </div>
      )
    case "button": {
      if (widget.when && state[widget.when] !== true) return null
      const action = trait.actions.find((a) => a.name === widget.action)
      return (
        <Button
          size="sm"
          variant="outline"
          disabled={disabled}
          onClick={() => act.mutate(widget.params ?? {})}
        >
          {action?.label[lang()] ?? widget.action}
        </Button>
      )
    }
    case "color":
      return (
        <label className="flex items-center justify-between gap-3 text-sm">
          <span className="text-muted-foreground">{name}</span>
          <input
            type="color"
            className="h-8 w-12 cursor-pointer rounded-md border bg-transparent"
            value={typeof value === "string" ? value : "#ffffff"}
            disabled={disabled}
            onChange={(e) => send(e.target.value)}
            aria-label={name}
          />
        </label>
      )
    default: {
      const text = field ? fieldText(field, value) : null
      if (text === null) return null
      return (
        <div
          className={cn(
            "flex items-baseline justify-between gap-3 text-sm",
            widget.widget === "gauge" && "items-center"
          )}
        >
          <span className="text-muted-foreground">{name}</span>
          <span
            className={cn(
              "font-medium",
              widget.widget === "gauge" && "text-2xl font-semibold"
            )}
          >
            {text}
          </span>
        </div>
      )
    }
  }
}

/**
 * The controls of a device as its traits describe them, for a kind this version does not know (a
 * module installed from a library may bring one): nothing when its traits describe none.
 */
export function GenericControls({ d }: { d: DeviceView }) {
  const catalog = useQuery({
    queryKey: ["traits"],
    queryFn: api.traits,
    staleTime: Infinity,
  })
  const pieces = cardPieces(d, catalog.data)
  if (!pieces.length) return null
  return (
    <div className="grid gap-3">
      {pieces.map((p, i) => (
        <PieceControl
          key={`${p.trait.trait}-${p.widget.widget}-${p.widget.field ?? p.widget.action}-${i}`}
          d={d}
          piece={p}
        />
      ))}
    </div>
  )
}
