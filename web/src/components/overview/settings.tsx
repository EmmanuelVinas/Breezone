import { useState } from "react"
import type {
  DashboardBlock,
  DeviceView,
  Room,
  Routine,
  TraitInfo,
} from "@/lib/api"
import { deviceType, typeOrder } from "@/lib/device-icons"
import { t } from "@/lib/i18n"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import type { BlockType, EnergyFlow } from "./model"
import {
  BLOCK_LABEL,
  block as newBlock,
  chartFields,
  FLOW_LABEL,
  shown,
} from "./model"

type Home = {
  devices: DeviceView[]
  rooms: Room[]
  routines: Routine[]
  traits: TraitInfo[]
}

function Field({
  label,
  children,
}: {
  label: string
  children: React.ReactNode
}) {
  return (
    <div className="grid gap-2">
      <Label>{label}</Label>
      {children}
    </div>
  )
}

function Pick({
  value,
  onChange,
  options,
  placeholder,
}: {
  value: string | undefined
  onChange: (v: string) => void
  options: [string, string][]
  placeholder?: string
}) {
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger className="w-full">
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent>
        {options.map(([v, label]) => (
          <SelectItem key={v} value={v}>
            {label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

/** Whether a block has what it needs to show something. */
export function complete(b: DashboardBlock) {
  const c = b.config
  if (b.type === "device") return typeof c.device_id === "number"
  if (b.type === "chart")
    return typeof c.device_id === "number" && typeof c.field === "string"
  if (b.type === "group") return "room_id" in c || "type" in c
  return true
}

/** The settings of one block: what it shows. */
function Settings({
  block,
  home,
  onChange,
}: {
  block: DashboardBlock
  home: Home
  onChange: (b: DashboardBlock) => void
}) {
  const c = block.config
  const set = (patch: Record<string, unknown>) =>
    onChange({ ...block, config: { ...c, ...patch } })
  const devices = home.devices.filter(shown)
  const deviceOptions = devices.map((d): [string, string] => [
    String(d.id),
    `${d.name} · ${deviceType(d).label}`,
  ])
  switch (block.type as BlockType) {
    case "energy":
      return (
        <Field label={t("Mesure")}>
          <Pick
            value={String(c.flow ?? "solar")}
            onChange={(flow) => set({ flow })}
            options={(Object.keys(FLOW_LABEL) as EnergyFlow[]).map((f) => [
              f,
              FLOW_LABEL[f],
            ])}
          />
        </Field>
      )
    case "group": {
      const types = [
        ...new Map(
          devices.map((d) => [deviceType(d).id, deviceType(d)])
        ).values(),
      ].sort((a, b) => typeOrder(a) - typeOrder(b))
      const key =
        c.type !== undefined
          ? `type:${c.type}`
          : "room_id" in c
            ? `room:${c.room_id ?? "none"}`
            : undefined
      const hidden = new Set((c.hidden as number[] | undefined) ?? [])
      const members = devices.filter((d) =>
        c.type !== undefined
          ? deviceType(d).id === c.type
          : "room_id" in c && d.room_id === (c.room_id ?? null)
      )
      return (
        <>
          <Field label={t("Ce que la carte rassemble")}>
            <Pick
              value={key}
              placeholder={t("Choisir")}
              onChange={(k) => {
                const [kind, v] = k.split(":")
                onChange({
                  ...block,
                  config:
                    kind === "type"
                      ? { type: v }
                      : { room_id: v === "none" ? null : Number(v) },
                })
              }}
              options={[
                ...home.rooms.map((r): [string, string] => [
                  `room:${r.id}`,
                  r.name,
                ]),
                ["room:none", t("Sans pièce")],
                ...types.map((ty): [string, string] => [
                  `type:${ty.id}`,
                  ty.label,
                ]),
              ]}
            />
          </Field>
          {members.length > 0 && (
            <Field label={t("Appareils montrés")}>
              <div className="grid gap-2">
                {members.map((d) => (
                  <label key={d.id} className="flex items-center gap-2 text-sm">
                    <Checkbox
                      checked={!hidden.has(d.id)}
                      onCheckedChange={(v) => {
                        const next = new Set(hidden)
                        if (v) next.delete(d.id)
                        else next.add(d.id)
                        set({ hidden: [...next] })
                      }}
                    />
                    {d.name}
                  </label>
                ))}
              </div>
            </Field>
          )}
        </>
      )
    }
    case "device":
      return (
        <Field label={t("Appareil")}>
          <Pick
            value={c.device_id !== undefined ? String(c.device_id) : undefined}
            placeholder={t("Choisir")}
            onChange={(v) => set({ device_id: Number(v) })}
            options={deviceOptions}
          />
        </Field>
      )
    case "chart": {
      const d = home.devices.find((x) => x.id === c.device_id)
      const fields = d ? chartFields(d, home.traits) : []
      return (
        <>
          <Field label={t("Appareil")}>
            <Pick
              value={
                c.device_id !== undefined ? String(c.device_id) : undefined
              }
              placeholder={t("Choisir")}
              onChange={(v) => {
                const next = home.devices.find((x) => x.id === Number(v))
                const first = next
                  ? chartFields(next, home.traits)[0]
                  : undefined
                set({ device_id: Number(v), field: first?.name })
              }}
              options={home.devices
                .filter((x) => chartFields(x, home.traits).length > 0)
                .map((x): [string, string] => [String(x.id), x.name])}
            />
          </Field>
          {fields.length > 0 && (
            <Field label={t("Mesure")}>
              <Pick
                value={typeof c.field === "string" ? c.field : undefined}
                onChange={(field) => set({ field })}
                options={fields.map((f) => [f.name, f.label])}
              />
            </Field>
          )}
          <Field label={t("Période")}>
            <Pick
              value={String(c.hours ?? 24)}
              onChange={(v) => set({ hours: Number(v) })}
              options={[
                ["24", t("24 heures")],
                ["168", t("7 jours")],
              ]}
            />
          </Field>
        </>
      )
    }
    case "scenes": {
      const ids = new Set((c.routine_ids as number[] | undefined) ?? [])
      return (
        <Field label={t("Scènes montrées (aucune cochée : toutes)")}>
          <div className="grid gap-2">
            {home.routines.map((r) => (
              <label key={r.id} className="flex items-center gap-2 text-sm">
                <Checkbox
                  checked={ids.has(r.id)}
                  onCheckedChange={(v) => {
                    const next = new Set(ids)
                    if (v) next.add(r.id)
                    else next.delete(r.id)
                    set({ routine_ids: [...next] })
                  }}
                />
                {r.name}
              </label>
            ))}
          </div>
        </Field>
      )
    }
    default:
      return (
        <p className="text-sm text-muted-foreground">
          {t("Ce bloc n'a pas de réglage.")}
        </p>
      )
  }
}

/** Sets up a block: a new one (its type first), or one already on the overview. */
export function BlockDialog({
  initial,
  home,
  onDone,
  onClose,
}: {
  initial?: DashboardBlock
  home: Home
  onDone: (b: DashboardBlock) => void
  onClose: () => void
}) {
  const [draft, setDraft] = useState<DashboardBlock | undefined>(initial)
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90svh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {initial ? t("Réglages du bloc") : t("Ajouter un bloc")}
          </DialogTitle>
        </DialogHeader>
        <div className="grid gap-4">
          {!initial && (
            <Field label={t("Type de bloc")}>
              <Pick
                value={draft?.type}
                placeholder={t("Choisir")}
                onChange={(type) => setDraft(newBlock(type as BlockType))}
                options={(Object.keys(BLOCK_LABEL) as BlockType[]).map((b) => [
                  b,
                  BLOCK_LABEL[b],
                ])}
              />
            </Field>
          )}
          {draft && <Settings block={draft} home={home} onChange={setDraft} />}
        </div>
        <DialogFooter>
          <Button
            disabled={!draft || !complete(draft)}
            onClick={() => draft && onDone(draft)}
          >
            {initial ? t("Appliquer") : t("Ajouter")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
