import { useState } from "react"
import { Link, useNavigate } from "react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { toast } from "sonner"
import { ExternalLink, Plug, Plus, Sun, Trash2 } from "lucide-react"
import type {
  ChargeBar,
  ChargeProfileInput,
  ChargeProfileView,
  ChargeRulePlan,
} from "@/lib/api"
import { api, currentUser } from "@/lib/api"
import { errorText, number } from "@/lib/format"
import { t } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import { PageHeader } from "@/components/app-shell"
import { PROFILE_COLORS } from "@/components/electricity/model"
import { effectColor, effectText } from "@/lib/charge-effects"
import { Badge } from "@/components/ui/badge"
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
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

const SELECTED =
  "data-[state=on]:border-primary data-[state=on]:bg-primary data-[state=on]:text-primary-foreground"

/** The legend of the friezes: each profile, the surplus, no charge. */
export function EffectLegend({ profiles }: { profiles: ChargeProfileView[] }) {
  const items: [string, string][] = [
    ...profiles.map(
      (p) => [p.name, p.color ?? "var(--primary)"] as [string, string]
    ),
    [t("Surplus solaire"), "var(--warning)"],
    [t("Ne pas charger"), "var(--muted)"],
  ]
  return (
    <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
      {items.map(([label, color]) => (
        <span key={label} className="inline-flex items-center gap-1.5">
          <span
            className="size-2.5 rounded-[3px] ring-1 ring-border"
            style={{ background: color }}
          />
          {label}
        </span>
      ))}
    </div>
  )
}

const HOURS = [0, 6, 12, 18, 24]
const DAY = 24 * 60

function hm(m: number) {
  return `${Math.floor(m / 60)} h${m % 60 ? ` ${String(m % 60).padStart(2, "0")}` : ""}`
}

/** A frieze of an automation: read only; a stretch opens the automation. */
function RuleBar({
  bar,
  profiles,
  onOpen,
}: {
  bar: ChargeBar
  profiles: ChargeProfileView[]
  onOpen: () => void
}) {
  return (
    <div className="grid gap-1">
      <div className="relative h-11 overflow-hidden rounded-xl bg-muted/40">
        {bar.segments.map((s, i) => {
          const off =
            !s.effect || s.effect.mode === "off" || s.effect.mode === "free"
          return (
            <button
              key={i}
              type="button"
              onClick={onOpen}
              title={t("Ouvrir l'automatisme")}
              className={cn(
                "absolute inset-y-0 flex cursor-pointer flex-col justify-center overflow-hidden border-r border-background px-2 text-left text-[11px] transition-[filter] hover:brightness-125 focus-visible:brightness-125 focus-visible:outline-none",
                off ? "text-muted-foreground" : "text-primary-foreground"
              )}
              style={{
                left: `${(s.start / DAY) * 100}%`,
                width: `${((s.end - s.start) / DAY) * 100}%`,
                background: effectColor(s.effect, profiles),
              }}
            >
              <b className="truncate">{effectText(s.effect, profiles)}</b>
              {s.effect && (
                <span className="truncate tabular-nums opacity-80">
                  {hm(s.start)} – {hm(s.end)}
                </span>
              )}
            </button>
          )
        })}
      </div>
      <div className="relative h-4 text-[11px] text-muted-foreground">
        {HOURS.map((h) => (
          <span
            key={h}
            className="absolute whitespace-nowrap"
            style={{
              left: `${(h / 24) * 100}%`,
              transform: `translateX(${h === 0 ? 0 : h === 24 ? -100 : -50}%)`,
            }}
          >
            {h} h
          </span>
        ))}
      </div>
    </div>
  )
}

/** An automation in one sentence, from its summary. */
function sentence(r: ChargeRulePlan) {
  const s = {
    ...r.summary,
    when: r.summary.when.replace(/^(quand|when|cuando)\s+/i, ""),
  }
  const conditions = s.conditions.join(` ${s.joined} `)
  const parts = [
    conditions
      ? t("Quand {when}, si {conditions} : {then}", {
          when: s.when,
          conditions,
          then: s.actions.join(", "),
        })
      : t("Quand {when} : {then}", {
          when: s.when,
          then: s.actions.join(", "),
        }),
  ]
  if (s.else.length) parts.push(t("sinon {else}", { else: s.else.join(", ") }))
  if (s.over.length)
    parts.push(t("ensuite {over}", { over: s.over.join(", ") }))
  return parts.join(" ; ") + "."
}

function barLabel(b: ChargeBar) {
  if (b.others) return t("Autres jours")
  return b.label || t("Tous les jours")
}

/** The automations that drive a charger, as friezes. */
function RulePlans({ profiles }: { profiles: ChargeProfileView[] }) {
  const navigate = useNavigate()
  const plans = useQuery({
    queryKey: ["charge-automations"],
    queryFn: api.chargeAutomations,
  })
  const list = plans.data ?? []
  const placed = list.filter((r) => r.placed)
  const loose = list.filter((r) => !r.placed)
  const open = (id: number) => navigate(`/programmations?automation=${id}`)
  const several =
    new Set(list.flatMap((r) => r.devices.map((d) => d.id))).size > 1
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div>
            <CardTitle>{t("Ce que prévoient les automatismes")}</CardTitle>
            <CardDescription>
              {t(
                "Rappel : ils se règlent dans Automatismes. Touchez une plage pour ouvrir le sien."
              )}
            </CardDescription>
          </div>
          <Link
            to="/programmations"
            className="text-sm text-muted-foreground hover:underline"
          >
            {t("Automatismes")} ›
          </Link>
        </div>
      </CardHeader>
      <CardContent className="grid gap-3">
        {plans.isLoading ? (
          <Skeleton className="h-32 w-full rounded-2xl" />
        ) : list.length === 0 ? (
          <p className="rounded-2xl border border-dashed p-6 text-center text-sm text-muted-foreground">
            {t(
              "Aucun automatisme ne pilote encore la borne : partez d'une recette ci-dessus."
            )}
          </p>
        ) : (
          <>
            {placed.map((r) => (
              <div
                key={r.id}
                className={cn(
                  "grid gap-2 rounded-2xl border p-4",
                  (!r.enabled || r.paused) && "opacity-60"
                )}
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-semibold">{r.name}</span>
                    {r.paused ? (
                      <Badge variant="secondary">
                        {t("en pause : l'appli de la borne décide")}
                      </Badge>
                    ) : r.enabled ? (
                      <Badge className="bg-success/15 text-success">
                        {t("actif")}
                      </Badge>
                    ) : (
                      <Badge variant="secondary">{t("désactivé")}</Badge>
                    )}
                  </div>
                  <Button variant="ghost" size="sm" onClick={() => open(r.id)}>
                    {t("Ouvrir")} <ExternalLink />
                  </Button>
                </div>
                <p className="text-sm text-muted-foreground">{sentence(r)}</p>
                {r.bars.map((b, i) => (
                  <div
                    key={i}
                    className="grid items-center gap-2 sm:grid-cols-[10rem_1fr]"
                  >
                    <span className="flex items-center gap-1.5 text-sm">
                      {b.color && (
                        <span
                          className="size-2.5 rounded-full"
                          style={{ background: b.color }}
                        />
                      )}
                      {barLabel(b)}
                      {several && (
                        <span className="text-muted-foreground">
                          · {r.devices.find((d) => d.id === b.device_id)?.name}
                        </span>
                      )}
                    </span>
                    <RuleBar
                      bar={b}
                      profiles={profiles}
                      onOpen={() => open(r.id)}
                    />
                  </div>
                ))}
                {r.assumed && (
                  <p className="text-xs text-muted-foreground">
                    {t(
                      "Certaines conditions ne se savent qu'au moment venu (présence, mesure, appareil) : la frise les suppose vraies."
                    )}
                  </p>
                )}
              </div>
            ))}
            {loose.length > 0 && (
              <div className="grid gap-2">
                <div className="text-xs font-semibold tracking-wide text-muted-foreground uppercase">
                  {t("Sans heure fixe")}
                </div>
                {loose.map((r) => (
                  <button
                    key={r.id}
                    type="button"
                    onClick={() => open(r.id)}
                    className={cn(
                      "flex items-center gap-3 rounded-2xl bg-muted/60 px-3 py-2.5 text-left hover:bg-muted",
                      (!r.enabled || r.paused) && "opacity-60"
                    )}
                  >
                    <Plug className="size-4 shrink-0 text-muted-foreground" />
                    <span className="grid">
                      <span className="text-sm font-medium">{r.name}</span>
                      <span className="text-xs text-muted-foreground">
                        {sentence(r)}
                      </span>
                    </span>
                  </button>
                ))}
              </div>
            )}
            <EffectLegend profiles={profiles} />
            <p className="text-xs text-muted-foreground">
              {t(
                "Sur la même heure, le dernier automatisme déclenché l'emporte. Si la programmation de l'appli d'une borne est allumée, ses automatismes sont en pause."
              )}
            </p>
          </>
        )}
      </CardContent>
    </Card>
  )
}

/** A power typed freely ("7,4"). */
function KwInput({
  value,
  onChange,
  label,
}: {
  value: number | null
  onChange: (v: number | null) => void
  label: string
}) {
  const [text, setText] = useState<string | null>(null)
  const shown = text ?? (value == null ? "" : String(value).replace(".", ","))
  return (
    <div className="relative w-24">
      <Input
        aria-label={label}
        inputMode="decimal"
        value={shown}
        className="pr-9 text-right"
        onChange={(e) => {
          setText(e.target.value)
          const v = Number(e.target.value.trim().replace(",", "."))
          if (Number.isFinite(v) && v > 0) onChange(v)
        }}
        onBlur={() => setText(null)}
      />
      <span className="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 text-sm text-muted-foreground">
        kW
      </span>
    </div>
  )
}

type Draft = ChargeProfileInput & { id?: number; key: string }

const draftOf = (p: ChargeProfileView): Draft => ({
  id: p.id,
  key: `p${p.id}`,
  name: p.name,
  kw: p.kw,
  color: p.color,
})

/** The profiles: a name, a colour, a power each; written by the administrators. */
function ProfilesCard({
  profiles,
  admin,
}: {
  profiles: ChargeProfileView[]
  admin: boolean
}) {
  const qc = useQueryClient()
  const saved = profiles.map(draftOf)
  const [drafts, setDrafts] = useState<Draft[]>(saved)
  const [seen, setSeen] = useState(profiles)
  if (seen !== profiles) {
    setSeen(profiles)
    setDrafts(saved)
  }
  const issue = drafts.find(
    (d) => !d.name.trim() || (d.kw != null && (d.kw <= 0 || d.kw > 50))
  )
  const changed = JSON.stringify(drafts) !== JSON.stringify(saved)
  const save = useMutation({
    mutationFn: async () => {
      const body = (d: Draft): ChargeProfileInput => ({
        name: d.name.trim(),
        kw: d.kw,
        color: d.color,
      })
      for (const p of saved)
        if (!drafts.some((d) => d.id === p.id))
          await api.deleteChargeProfile(p.id!)
      for (const d of drafts) {
        const before = saved.find((p) => p.id === d.id)
        if (!d.id) await api.createChargeProfile(body(d))
        else if (JSON.stringify(before) !== JSON.stringify(d))
          await api.saveChargeProfile(d.id, body(d))
      }
    },
    onSuccess: () => toast.success(t("Profils enregistrés")),
    onError: (e) => toast.error(errorText(e)),
    onSettled: () => {
      qc.invalidateQueries({ queryKey: ["charge-profiles"] })
      qc.invalidateQueries({ queryKey: ["charge-automations"] })
      qc.invalidateQueries({ queryKey: ["charging"] })
    },
  })
  const set = (i: number, patch: Partial<Draft>) =>
    setDrafts(drafts.map((d, j) => (j === i ? { ...d, ...patch } : d)))
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <div className="flex flex-wrap items-center justify-between gap-3">
          <CardTitle>{t("Profils")}</CardTitle>
          {admin && (
            <div className="flex gap-2">
              <Button
                variant="outline"
                size="sm"
                disabled={drafts.length >= 20}
                onClick={() =>
                  setDrafts([
                    ...drafts,
                    {
                      key: `n${Date.now()}`,
                      name: t("Profil {n}", { n: drafts.length + 1 }),
                      kw: 7,
                      color:
                        PROFILE_COLORS[
                          (drafts.length + 3) % PROFILE_COLORS.length
                        ],
                    },
                  ])
                }
              >
                <Plus /> {t("Ajouter un profil")}
              </Button>
              <Button
                size="sm"
                disabled={!changed || !!issue || save.isPending}
                onClick={() => save.mutate()}
              >
                {t("Enregistrer")}
              </Button>
            </div>
          )}
        </div>
        {issue && (
          <CardDescription className="text-destructive">
            {!issue.name.trim()
              ? t("Chaque profil a besoin d'un nom.")
              : t("« {name} » : une puissance de 0 à 50 kW.", {
                  name: issue.name,
                })}
          </CardDescription>
        )}
      </CardHeader>
      <CardContent className="grid">
        {drafts.length === 0 && (
          <p className="py-4 text-sm text-muted-foreground">
            {t(
              "Aucun profil pour l'instant : ajoutez-en un, par exemple « Nuit 7 kW »."
            )}
          </p>
        )}
        {drafts.map((d, i) => {
          const view = profiles.find((p) => p.id === d.id)
          return (
            <div
              key={d.key}
              className="grid items-center gap-3 border-t py-3 first:border-t-0 md:grid-cols-[auto_minmax(10rem,1fr)_auto_minmax(8rem,1fr)_auto]"
            >
              <div
                className="flex items-center gap-1"
                role="group"
                aria-label={t("Couleur")}
              >
                {PROFILE_COLORS.map((c) => (
                  <button
                    key={c}
                    type="button"
                    disabled={!admin}
                    aria-label={t("Couleur {c}", { c })}
                    aria-pressed={d.color === c}
                    onClick={() => set(i, { color: c })}
                    className={cn(
                      "size-4 rounded-full border",
                      d.color === c &&
                        "ring-2 ring-primary ring-offset-2 ring-offset-background",
                      !admin && d.color !== c && "hidden"
                    )}
                    style={{ background: c }}
                  />
                ))}
              </div>
              <Input
                aria-label={t("Nom du profil")}
                value={d.name}
                disabled={!admin}
                onChange={(e) => set(i, { name: e.target.value })}
              />
              <div className="flex items-center gap-2">
                <ToggleGroup
                  type="single"
                  variant="outline"
                  size="sm"
                  disabled={!admin}
                  value={d.kw == null ? "max" : "kw"}
                  onValueChange={(v) =>
                    v && set(i, { kw: v === "max" ? null : (view?.kw ?? 7) })
                  }
                >
                  <ToggleGroupItem value="kw" className={SELECTED}>
                    {t("Jusqu'à")}
                  </ToggleGroupItem>
                  <ToggleGroupItem value="max" className={SELECTED}>
                    {t("Maximum")}
                  </ToggleGroupItem>
                </ToggleGroup>
                {d.kw != null ? (
                  <KwInput
                    label={t("Puissance de {name}", { name: d.name })}
                    value={d.kw}
                    onChange={(kw) => set(i, { kw })}
                  />
                ) : (
                  <span className="w-24 text-sm text-muted-foreground">
                    {t("de la borne")}
                  </span>
                )}
              </div>
              <span className="text-xs text-muted-foreground">
                {view?.automations.length
                  ? view.automations.map((a) => a.name).join(" · ")
                  : t("Aucun automatisme")}
              </span>
              {admin ? (
                <Button
                  variant="ghost"
                  size="icon"
                  className="justify-self-end text-destructive"
                  aria-label={t("Supprimer {name}", { name: d.name })}
                  onClick={() => setDrafts(drafts.filter((_, j) => j !== i))}
                >
                  <Trash2 />
                </Button>
              ) : (
                <span />
              )}
            </div>
          )
        })}
        <p className="pt-2 text-xs text-muted-foreground">
          {t(
            "La couleur sert aux frises. Une borne qui ne peut pas tant tire son maximum ; une borne en mode dynamique garde son délestage."
          )}
        </p>
      </CardContent>
    </Card>
  )
}

/** Recharge › Profiles: the powers a charger may follow, and what the automations plan. */
export function ChargeProfilesPage() {
  const admin = currentUser()?.role === "admin"
  const profiles = useQuery({
    queryKey: ["charge-profiles"],
    queryFn: api.chargeProfiles,
  })
  const recipes: [string, string, boolean][] = [
    ["charge-offpeak", t("Heures creuses"), false],
    ["charge-blue-night", t("Nuit bleue (Tempo)"), false],
    ["charge-solar", t("Solaire"), true],
  ]
  return (
    <>
      <div className="mb-1 text-sm text-muted-foreground">
        <Link to="/recharge" className="hover:underline">
          {t("Recharge")}
        </Link>{" "}
        › {t("Profils")}
      </div>
      <PageHeader
        title={t("Profils de recharge")}
        description={t(
          "Ce que la borne peut tirer au plus. Quand elle charge, ce sont les automatismes qui le disent."
        )}
      />
      <div className="grid gap-4">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-sm text-muted-foreground">
            {t("Nouvel automatisme :")}
          </span>
          {recipes.map(([id, label, sun]) => (
            <Button key={id} size="sm" variant="outline" asChild>
              <Link to={`/programmations?recette=${id}`}>
                {sun ? <Sun /> : <Plus />}
                {label}
              </Link>
            </Button>
          ))}
        </div>
        {profiles.isLoading ? (
          <Skeleton className="h-48 w-full rounded-3xl" />
        ) : (
          <>
            <ProfilesCard profiles={profiles.data ?? []} admin={admin} />
            <RulePlans profiles={profiles.data ?? []} />
          </>
        )}
      </div>
    </>
  )
}

/** "jusqu'à 7 kW" or "au maximum" (for the texts of other screens). */
export function profilePower(p: { kw: number | null }) {
  return p.kw != null
    ? t("jusqu'à {kw} kW", { kw: number(p.kw, 1) })
    : t("au maximum")
}
