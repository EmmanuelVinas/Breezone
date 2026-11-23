import { useState } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Plus, Trash2 } from "lucide-react"
import { toast } from "sonner"
import type {
  Automation,
  AutomationCondition,
  AutomationInput,
  AutomationStep,
  AutomationTrigger,
} from "@/lib/api"
import { api } from "@/lib/api"
import { t } from "@/lib/i18n"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { ConditionList, Connector, MatchToggle, leavesOf } from "./conditions"
import type { Home, Option } from "./fields"
import { MAX_CONDITIONS, MAX_TRIGGERS, Pick } from "./fields"
import {
  ELSE_TONE,
  IF_TONE,
  RemoveButton,
  Section,
  StepList,
  THEN_TONE,
  WHEN_TONE,
} from "./steps"
import { ROW, TriggerEditor, newTrigger } from "./triggers"

export function conditionComplete(c: AutomationCondition): boolean {
  if (c.kind === "group")
    return c.conditions.length >= 2 && c.conditions.every(conditionComplete)
  if (c.kind === "measure") return Number.isFinite(c.value)
  if (c.kind === "days") return c.days.length > 0
  return true
}

/** Every number of the rule is filled in. */
export function complete(d: AutomationInput) {
  const triggersOk = d.triggers.every(
    (w) => w.kind !== "measure" || Number.isFinite(w.value)
  )
  const stepsOk = [...d.actions, ...d.else, ...d.over].every(
    (s) =>
      (s.kind !== "wait_until" || conditionComplete(s.condition)) &&
      (s.kind !== "notify" || s.message.trim().length > 0)
  )
  return (
    Boolean(d.name.trim()) &&
    d.triggers.length > 0 &&
    triggersOk &&
    d.conditions.every(conditionComplete) &&
    d.actions.length > 0 &&
    stepsOk
  )
}

export function inputOf(a: Automation): AutomationInput {
  return {
    name: a.name,
    enabled: a.enabled,
    triggers: a.triggers,
    match: a.match ?? "all",
    conditions: a.conditions,
    actions: a.actions,
    else: a.else ?? [],
    mode: a.mode ?? "single",
    over: a.over ?? [],
  }
}

/** A trigger that holds for a while: it can tell when it is over. */
export const lasts = (w: AutomationTrigger) =>
  w.kind === "measure" ||
  w.kind === "state" ||
  w.kind === "facade" ||
  w.kind === "presence" ||
  w.kind === "tariff" ||
  w.kind === "module"

export type OverMode = "none" | "restore" | "steps"

export function overModeOf(over: AutomationStep[]): OverMode {
  if (over.length === 0) return "none"
  if (over.length === 1 && over[0].kind === "restore") return "restore"
  return "steps"
}

export function AutomationEditor({
  home,
  id,
  initial,
  onClose,
}: {
  home: Home
  /** The rule changed; none for a new one. */
  id?: number
  initial: AutomationInput
  onClose: () => void
}) {
  const qc = useQueryClient()
  const [draft, setDraft] = useState<AutomationInput>(initial)
  const [withElse, setWithElse] = useState(initial.else.length > 0)
  const [overMode, setOverMode] = useState<OverMode>(overModeOf(initial.over))
  const set = (patch: Partial<AutomationInput>) =>
    setDraft((d) => ({ ...d, ...patch }))
  const save = useMutation({
    mutationFn: () => {
      const over: AutomationStep[] = !lasting
        ? []
        : overMode === "restore"
          ? [{ kind: "restore" }]
          : overMode === "steps"
            ? draft.over
            : []
      const input = { ...draft, else: withElse ? draft.else : [], over }
      return id ? api.updateAutomation(id, input) : api.createAutomation(input)
    },
    onSuccess: () => {
      toast.success(id ? t("Automatisme modifié") : t("Automatisme créé"))
      qc.invalidateQueries({ queryKey: ["automations"] })
      onClose()
    },
    onError: (e) => toast.error(e instanceof Error ? e.message : String(e)),
  })

  const lasting = draft.triggers.some(lasts)
  const drivesDevice = draft.actions.some(
    (s) => s.kind === "device" || s.kind === "group"
  )
  const all = [
    ...draft.actions,
    ...(withElse ? draft.else : []),
    ...(lasting && overMode === "steps" ? draft.over : []),
  ]
  const waits = all.some((s) => s.kind === "wait" || s.kind === "wait_until")
  const drivesZone = all.some(
    (s) => s.kind === "device" && s.trait === "climate.zone"
  )
  const room = MAX_CONDITIONS - leavesOf(draft.conditions)

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90svh] overflow-x-hidden overflow-y-auto sm:max-w-[min(72rem,95vw)]">
        <DialogHeader>
          <DialogTitle>
            {id ? t("Modifier l'automatisme") : t("Nouvel automatisme")}
          </DialogTitle>
          <DialogDescription>
            {t(
              "Une règle se lit comme une phrase : quand quelque chose se produit, si certaines choses sont vraies, alors Breezone agit. Tout se passe sur votre serveur."
            )}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-5">
          <div className="grid gap-2">
            <Label htmlFor="an">{t("Nom")}</Label>
            <Input
              id="an"
              value={draft.name}
              placeholder={t("ex. Surplus solaire : chauffer le salon")}
              onChange={(e) => set({ name: e.target.value })}
            />
          </div>

          <Section
            badge={t("Quand")}
            tone={WHEN_TONE}
            hint={t("Ce qui déclenche la règle ; un seul suffit.")}
          >
            {draft.triggers.map((w, i) => (
              <div key={i} className="grid gap-2">
                {i > 0 && <Connector text={t("ou")} />}
                <div className={ROW}>
                  <TriggerEditor
                    home={home}
                    when={w}
                    onChange={(next) =>
                      set({
                        triggers: draft.triggers.map((x, j) =>
                          j === i ? next : x
                        ),
                      })
                    }
                  />
                  {draft.triggers.length > 1 && (
                    <RemoveButton
                      onClick={() =>
                        set({
                          triggers: draft.triggers.filter((_, j) => j !== i),
                        })
                      }
                    />
                  )}
                </div>
              </div>
            ))}
            <div className="flex flex-wrap items-center gap-2">
              {draft.triggers.length < MAX_TRIGGERS && (
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() =>
                    set({
                      triggers: [...draft.triggers, newTrigger("time", home)!],
                    })
                  }
                >
                  <Plus /> {t("Ajouter un déclencheur (ou)")}
                </Button>
              )}
            </div>
            <p className="text-xs text-muted-foreground">
              {t(
                "Pour « et », ajoutez une condition dans Si : la règle part quand le déclencheur et ses conditions sont vrais ensemble."
              )}
            </p>
            {draft.triggers.some(
              (w) =>
                w.kind === "measure" ||
                w.kind === "state" ||
                w.kind === "facade"
            ) && (
              <p className="text-xs text-muted-foreground">
                {t(
                  "Une mesure, un état ou le soleil sur une façade déclenche une fois, puis attend que ce ne soit plus vrai avant de pouvoir déclencher de nouveau."
                )}
              </p>
            )}
          </Section>

          <Section
            badge={t("Si")}
            tone={IF_TONE}
            hint={t("Facultatif : ce qui doit être vrai à ce moment.")}
          >
            {draft.conditions.length > 1 && (
              <div>
                <MatchToggle
                  value={draft.match}
                  onChange={(match) => set({ match })}
                />
              </div>
            )}
            <ConditionList
              home={home}
              conditions={draft.conditions}
              match={draft.match}
              onChange={(conditions) => set({ conditions })}
              room={room}
            />
          </Section>

          <Section
            badge={t("Alors")}
            tone={THEN_TONE}
            hint={t("Ce que Breezone fait, dans l'ordre.")}
          >
            <StepList
              home={home}
              steps={draft.actions}
              onChange={(actions) => set({ actions })}
            />
          </Section>

          {!lasting && draft.triggers.length > 0 && (
            <p className="text-xs text-muted-foreground">
              {t(
                "Pas d'« Ensuite » ici : une heure, un lever ou un coucher du soleil est un instant, il ne cesse pas d'être vrai. Pour revenir en arrière, créez une autre règle au moment voulu."
              )}
            </p>
          )}

          {lasting && (
            <Section
              badge={t("Ensuite")}
              tone={THEN_TONE}
              hint={t(
                "Quand ce n'est plus vrai : le déclencheur ne tient plus (repassé de l'autre côté du seuil avec une marge, aussi longtemps qu'il a fallu pour démarrer). Les étapes de « Alors » encore en cours s'arrêtent."
              )}
            >
              <Pick
                value={overMode}
                onChange={(v) => setOverMode(v as OverMode)}
                options={[
                  ["none", t("Ne rien faire")],
                  ...(drivesDevice
                    ? [["restore", t("Remettre comme avant")] as Option]
                    : []),
                  ["steps", t("Faire…")],
                ]}
                className="w-full lg:w-72"
              />
              {overMode === "restore" && (
                <p className="text-xs text-muted-foreground">
                  {t(
                    "Chaque appareil reprend son réglage d'avant, sauf si quelqu'un l'a changé entre-temps."
                  )}
                </p>
              )}
              {overMode === "steps" && (
                <StepList
                  home={home}
                  steps={draft.over}
                  before={draft.actions}
                  onChange={(over) => set({ over })}
                />
              )}
            </Section>
          )}

          {withElse ? (
            <Section
              badge={t("Sinon")}
              tone={ELSE_TONE}
              hint={t(
                "Ce que Breezone fait quand les conditions sont fausses."
              )}
            >
              <StepList
                home={home}
                steps={draft.else}
                onChange={(steps) => set({ else: steps })}
              />
              <div>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => setWithElse(false)}
                >
                  <Trash2 /> {t("Retirer « Sinon »")}
                </Button>
              </div>
            </Section>
          ) : (
            draft.conditions.length > 0 && (
              <div>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => setWithElse(true)}
                >
                  <Plus /> {t("Ajouter « Sinon »")}
                </Button>
              </div>
            )
          )}

          {waits && (
            <div className="grid gap-2">
              <Label>
                {t("Si la règle se redéclenche pendant qu'elle attend")}
              </Label>
              <Pick
                value={draft.mode}
                onChange={(v) => set({ mode: v as "single" | "restart" })}
                options={[
                  ["single", t("Ignorer le nouveau déclenchement")],
                  ["restart", t("Tout recommencer")],
                ]}
                className="w-full lg:w-72"
              />
            </div>
          )}
          {drivesZone && (
            <p className="text-xs text-muted-foreground">
              {t(
                "Cette règle pilote une zone de climatisation : vérifiez qu'une programmation Airzone ne fait pas le contraire au même moment."
              )}
            </p>
          )}
        </div>
        <DialogFooter>
          <Button
            onClick={() => save.mutate()}
            disabled={save.isPending || !complete(draft)}
          >
            {t("Enregistrer")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ---------------------------------------------------------------------------------------
// Recipes: ready-made rules, built from the devices of the home
// ---------------------------------------------------------------------------------------
