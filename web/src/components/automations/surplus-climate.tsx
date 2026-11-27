import { useState } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Loader2 } from "lucide-react"
import { toast } from "sonner"
import type { AutomationInput, AutomationStep, DeviceView } from "@/lib/api"
import { api } from "@/lib/api"
import { errorText } from "@/lib/format"
import { t } from "@/lib/i18n"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import type { Home } from "./fields"
import { MAX_ACTIONS } from "./fields"
import { blank } from "./recipes"

/** What the recipe asks: the rooms, the temperatures wanted, when it heats or cools. */
type Setup = {
  zones: number[]
  heat: boolean
  heatTo: number
  /** Heats only when it is colder outside. */
  heatBelow: number
  cool: boolean
  coolTo: number
  /** Cools only when it is warmer outside. */
  coolAbove: number
  /** What a room is reckoned to draw (W): it starts once the surplus is that much. */
  power: number
}

/**
 * The rules of the recipe: while it is cold (or warm) outside, the rooms chosen follow the solar
 * surplus, heating (or cooling) to their temperature when there is enough of it, in the order of
 * the priorities; once it is no longer so, they are put back as they were. The mode of a system is
 * left as it is (it is the whole system's): a room heats only while its system heats.
 */
export function surplusClimateRules(
  home: Home,
  zones: DeviceView[],
  s: Setup
): AutomationInput[] {
  const chosen = zones.filter((z) => s.zones.includes(z.id))
  const groups: DeviceView[][] = []
  for (let i = 0; i < chosen.length; i += MAX_ACTIONS)
    groups.push(chosen.slice(i, i + MAX_ACTIONS))
  const rule = (
    name: string,
    group: DeviceView[],
    mode: "heat" | "cool",
    op: "below" | "above",
    outdoor: number,
    setpoint: number
  ): AutomationInput => ({
    ...blank(home),
    name,
    triggers: [
      {
        kind: "measure",
        measure: { kind: "outdoor" },
        op,
        value: outdoor,
        for_minutes: 0,
      },
    ],
    match: "all",
    conditions: [],
    actions: group.map((z): AutomationStep => ({
      kind: "surplus",
      device_id: z.id,
      enabled: true,
      power: s.power,
      min_on_minutes: 30,
      mode,
      setpoint,
    })),
    over: group.map((z): AutomationStep => ({
      kind: "surplus",
      device_id: z.id,
      enabled: false,
    })),
  })
  // Several rules: each says which rooms it heats.
  const part = (name: string, i: number) =>
    groups.length > 1
      ? `${name} · ${groups[i].map((z) => z.name).join(", ")}`
      : name
  return groups.flatMap((group, i) => [
    ...(s.heat
      ? [
          rule(
            part(t("Surplus solaire : chauffer la maison"), i),
            group,
            "heat",
            "below",
            s.heatBelow,
            s.heatTo
          ),
        ]
      : []),
    ...(s.cool
      ? [
          rule(
            part(t("Surplus solaire : rafraîchir la maison"), i),
            group,
            "cool",
            "above",
            s.coolAbove,
            s.coolTo
          ),
        ]
      : []),
  ])
}

function Degrees({
  id,
  label,
  value,
  onChange,
  step = 0.5,
  unit = "°C",
}: {
  id: string
  label: string
  value: number
  onChange: (v: number) => void
  step?: number
  unit?: string
}) {
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      <div className="flex items-center gap-2">
        <Input
          id={id}
          type="number"
          step={step}
          value={value}
          className="w-28"
          onChange={(e) => onChange(Number(e.target.value))}
        />
        <span className="text-sm text-muted-foreground">{unit}</span>
      </div>
    </div>
  )
}

/** The form of the recipe, then its rules made at once. */
export function SurplusClimateSetup({
  home,
  zones,
  onDone,
  onBack,
}: {
  home: Home
  zones: DeviceView[]
  onDone: () => void
  onBack: () => void
}) {
  const qc = useQueryClient()
  const [s, setS] = useState<Setup>({
    zones: zones.map((z) => z.id),
    heat: true,
    heatTo: 21,
    heatBelow: 15,
    cool: true,
    coolTo: 25,
    coolAbove: 25,
    power: 1500,
  })
  const set = (patch: Partial<Setup>) => setS({ ...s, ...patch })
  const rules = surplusClimateRules(home, zones, s)
  const create = useMutation({
    mutationFn: async () => {
      for (const r of rules) await api.createAutomation(r)
    },
    onSuccess: () => {
      toast.success(
        rules.length > 1
          ? t("{n} automatismes créés", { n: rules.length })
          : t("Automatisme créé")
      )
      qc.invalidateQueries({ queryKey: ["automations"] })
      onDone()
    },
    onError: (e) => {
      toast.error(errorText(e))
      qc.invalidateQueries({ queryKey: ["automations"] })
    },
  })
  return (
    <div className="grid gap-5">
      <div className="grid gap-2">
        <Label>{t("Pièces à chauffer ou rafraîchir")}</Label>
        <div className="grid gap-2 sm:grid-cols-2">
          {zones.map((z) => (
            <label key={z.id} className="flex items-center gap-2 text-sm">
              <Checkbox
                checked={s.zones.includes(z.id)}
                onCheckedChange={(v) =>
                  set({
                    zones:
                      v === true
                        ? [...s.zones, z.id]
                        : s.zones.filter((x) => x !== z.id),
                  })
                }
              />
              {z.name}
            </label>
          ))}
        </div>
      </div>
      <div className="grid gap-3 rounded-2xl border p-3">
        <label className="flex items-center gap-2 font-medium">
          <Checkbox
            checked={s.heat}
            onCheckedChange={(v) => set({ heat: v === true })}
          />
          {t("Chauffer quand il fait froid dehors")}
        </label>
        {s.heat && (
          <div className="flex flex-wrap gap-4">
            <Degrees
              id="heat-to"
              label={t("Jusqu'à")}
              value={s.heatTo}
              onChange={(v) => set({ heatTo: v })}
            />
            <Degrees
              id="heat-below"
              label={t("Si la température extérieure est sous")}
              value={s.heatBelow}
              onChange={(v) => set({ heatBelow: v })}
            />
          </div>
        )}
      </div>
      <div className="grid gap-3 rounded-2xl border p-3">
        <label className="flex items-center gap-2 font-medium">
          <Checkbox
            checked={s.cool}
            onCheckedChange={(v) => set({ cool: v === true })}
          />
          {t("Rafraîchir quand il fait chaud dehors")}
        </label>
        {s.cool && (
          <div className="flex flex-wrap gap-4">
            <Degrees
              id="cool-to"
              label={t("Jusqu'à")}
              value={s.coolTo}
              onChange={(v) => set({ coolTo: v })}
            />
            <Degrees
              id="cool-above"
              label={t("Si la température extérieure dépasse")}
              value={s.coolAbove}
              onChange={(v) => set({ coolAbove: v })}
            />
          </div>
        )}
      </div>
      <Degrees
        id="power"
        label={t("Puissance estimée d'une pièce")}
        value={s.power}
        step={100}
        unit="W"
        onChange={(v) => set({ power: v })}
      />
      <p className="text-sm text-muted-foreground">
        {t(
          "Chaque pièce démarre quand le surplus suffit depuis 1 min et s'arrête quand il manque depuis 3 min, dans l'ordre des priorités des automatismes. Le mode de la climatisation (chaud ou froid) reste celui que vous avez choisi : une pièce ne chauffe que si son système est en mode chaud. Ensuite, elle revient comme avant, sauf si quelqu'un l'a changée entre-temps."
        )}
      </p>
      <div className="flex flex-wrap gap-2">
        <Button
          disabled={rules.length === 0 || create.isPending}
          onClick={() => create.mutate()}
        >
          {create.isPending && <Loader2 className="animate-spin" />}
          {rules.length > 1
            ? t("Créer les {n} automatismes", { n: rules.length })
            : t("Créer l'automatisme")}
        </Button>
        <Button variant="ghost" onClick={onBack}>
          {t("Retour aux recettes")}
        </Button>
      </div>
    </div>
  )
}
