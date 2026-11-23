import { useRef, useState } from "react"
import { Link } from "react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { toast } from "sonner"
import {
  Merge,
  Moon,
  Plus,
  Scissors,
  Sun,
  Trash2,
  Workflow,
  Zap,
} from "lucide-react"
import type {
  ChargeDayKind,
  ChargeMode,
  ChargeProfileInput,
  ChargeProfileView,
  ChargeSlot,
  OffPeak,
} from "@/lib/api"
import { api, currentUser } from "@/lib/api"
import { errorText, number } from "@/lib/format"
import { t } from "@/lib/i18n"
import { useChosenInstallation } from "@/lib/selected-installation"
import { cn } from "@/lib/utils"
import { PageHeader } from "@/components/app-shell"
import { CalendarCard } from "@/components/electricity/electricity-tab"
import {
  clock,
  DAY,
  merge,
  minutes,
  moveStart,
  newProfileId,
  PROFILE_COLORS,
  slotEnd,
  split,
} from "@/components/electricity/model"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Skeleton } from "@/components/ui/skeleton"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

const SELECTED =
  "data-[state=on]:border-primary data-[state=on]:bg-primary data-[state=on]:text-primary-foreground"

const MODE_COLOR: Record<ChargeMode, string> = {
  max: "var(--primary)",
  solar: "var(--warning)",
  off: "var(--muted)",
}

const MODE_LABEL: Record<ChargeMode, string> = {
  max: t("Charger jusqu'à"),
  solar: t("Surplus solaire seulement"),
  off: t("Ne pas charger"),
}

/** What a slot does, in a few words. */
function slotText(s: ChargeSlot) {
  if (s.mode === "max")
    return s.kw
      ? t("jusqu'à {kw} kW", { kw: number(s.kw, 1) })
      : t("au maximum")
  return s.mode === "solar" ? t("surplus solaire") : t("pas de charge")
}

const every = (id: string) => Array.from({ length: 7 }, () => id)

/**
 * Slots from the off-peak hours of the contract (22:00 to 06:00 without any): `on` in them,
 * `off` elsewhere.
 */
function offPeakSlots(
  offpeak: OffPeak[],
  on: ChargeSlot,
  off: ChargeSlot
): ChargeSlot[] {
  const ranges = offpeak.length ? offpeak : [{ from: "22:00", to: "06:00" }]
  const half = Array.from({ length: 48 }, () => false)
  for (const r of ranges) {
    const from = minutes(r.from) / 30
    const to = minutes(r.to) / 30
    for (let i = from; i !== to; i = (i + 1) % 48) half[i] = true
  }
  const slots: ChargeSlot[] = []
  half.forEach((cheap, i) => {
    if (i > 0 && cheap === half[i - 1]) return
    slots.push({ ...(cheap ? on : off), start: clock(i * 30) })
  })
  return slots
}

const OFF: ChargeSlot = { start: "00:00", mode: "off" }

/** Profiles to start from. */
function templates(offpeak: OffPeak[]): [string, ChargeProfileInput][] {
  const night = offPeakSlots(offpeak, { start: "", mode: "max", kw: 7 }, OFF)
  return [
    [
      t("Heures creuses"),
      {
        name: t("Heures creuses"),
        profiles: [
          {
            id: "d",
            name: t("Tous les jours"),
            color: PROFILE_COLORS[3],
            slots: night,
          },
        ],
        calendar: { day_start: "00:00", weekdays: every("d"), colors: {} },
      },
    ],
    [
      t("Nuit bleue (Tempo)"),
      {
        name: t("Nuit bleue"),
        profiles: [
          {
            id: "blue",
            name: t("Jour bleu"),
            color: PROFILE_COLORS[0],
            slots: night,
          },
          {
            id: "other",
            name: t("Jours blanc et rouge"),
            color: PROFILE_COLORS[2],
            slots: [OFF],
          },
        ],
        calendar: {
          day_start: "06:00",
          weekdays: every("blue"),
          colors: { blue: "blue", white: "other", red: "other" },
        },
      },
    ],
    [
      t("Solaire"),
      {
        name: t("Solaire"),
        profiles: [
          {
            id: "d",
            name: t("Tous les jours"),
            color: PROFILE_COLORS[4],
            slots: [{ start: "00:00", mode: "solar" }],
          },
        ],
        calendar: { day_start: "00:00", weekdays: every("d"), colors: {} },
      },
    ],
  ]
}

/** What is wrong with a profile, as the server would say it (null: nothing). */
function problem(p: ChargeProfileInput): string | null {
  if (!p.name.trim()) return t("Donnez un nom au profil.")
  if (p.profiles.length === 0) return t("Ajoutez au moins un type de jour.")
  for (const k of p.profiles) {
    if (!k.name.trim()) return t("Chaque type de jour a besoin d'un nom.")
    if (k.slots.some((s) => s.kw != null && (s.kw <= 0 || s.kw > 50)))
      return t("« {name} » : une puissance de 0 à 50 kW.", { name: k.name })
  }
  return null
}

const HOURS = [0, 6, 12, 18, 24]

/** A kind of day as a bar of 24 hours: a slot to pick, a limit to drag. */
function Frieze({
  kind,
  selected,
  onSelect,
  onChange,
}: {
  kind: ChargeDayKind
  selected: number | null
  onSelect: (i: number) => void
  onChange: (slots: ChargeSlot[]) => void
}) {
  const bar = useRef<HTMLDivElement>(null)
  const at = (clientX: number) => {
    const r = bar.current!.getBoundingClientRect()
    return ((clientX - r.left) / r.width) * DAY
  }
  return (
    <div className="grid gap-1">
      <div
        ref={bar}
        className="relative h-12 touch-none rounded-xl bg-muted select-none"
      >
        <div className="absolute inset-0 overflow-hidden rounded-xl">
          {kind.slots.map((s, i) => {
            const start = minutes(s.start)
            const end = slotEnd(kind.slots, i)
            return (
              <button
                key={i}
                type="button"
                onClick={() => onSelect(i)}
                aria-pressed={selected === i}
                aria-label={t("De {from} à {to} : {what}", {
                  from: s.start,
                  to: clock(end),
                  what: slotText(s),
                })}
                className={cn(
                  "absolute inset-y-0 flex flex-col justify-center overflow-hidden border-r border-background px-2 text-left text-xs",
                  s.mode === "off"
                    ? "text-muted-foreground"
                    : "text-primary-foreground",
                  selected === i &&
                    "z-[1] ring-2 ring-foreground outline-none ring-inset"
                )}
                style={{
                  left: `${(start / DAY) * 100}%`,
                  width: `${((end - start) / DAY) * 100}%`,
                  background: MODE_COLOR[s.mode],
                }}
              >
                <b className="truncate">{slotText(s)}</b>
                <span className="truncate tabular-nums opacity-80">
                  {s.start} – {clock(end)}
                </span>
              </button>
            )
          })}
        </div>
        {kind.slots.slice(1).map((s, j) => {
          const i = j + 1
          return (
            <span
              key={i}
              role="slider"
              tabIndex={0}
              aria-label={t("Début de la plage de {time}", { time: s.start })}
              aria-valuemin={0}
              aria-valuemax={DAY}
              aria-valuenow={minutes(s.start)}
              aria-valuetext={s.start}
              className="absolute -inset-y-1 z-[2] w-2 -translate-x-1/2 cursor-ew-resize rounded bg-foreground/85 focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none"
              style={{ left: `${(minutes(s.start) / DAY) * 100}%` }}
              onPointerDown={(e) =>
                e.currentTarget.setPointerCapture(e.pointerId)
              }
              onPointerMove={(e) => {
                if (!e.currentTarget.hasPointerCapture(e.pointerId)) return
                onChange(moveStart(kind.slots, i, at(e.clientX)))
              }}
              onKeyDown={(e) => {
                const by =
                  e.key === "ArrowLeft" ? -30 : e.key === "ArrowRight" ? 30 : 0
                if (!by) return
                e.preventDefault()
                onChange(moveStart(kind.slots, i, minutes(s.start) + by))
              }}
            />
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

/** A power typed freely ("7,4"). */
function KwInput({
  value,
  onChange,
  id,
}: {
  value: number | null | undefined
  onChange: (v: number | null) => void
  id: string
}) {
  const [text, setText] = useState<string | null>(null)
  const shown = text ?? (value == null ? "" : String(value).replace(".", ","))
  return (
    <Input
      id={id}
      inputMode="decimal"
      placeholder={t("maximum")}
      value={shown}
      onChange={(e) => {
        setText(e.target.value)
        const raw = e.target.value.trim().replace(",", ".")
        const v = Number(raw)
        if (raw === "") onChange(null)
        else if (Number.isFinite(v) && v > 0) onChange(v)
      }}
      onBlur={() => setText(null)}
    />
  )
}

/** A kind of day: its name, its colour, its slots, the slot picked to change. */
function KindEditor({
  kind,
  onChange,
  onRemove,
  offpeak,
}: {
  kind: ChargeDayKind
  onChange: (k: ChargeDayKind) => void
  onRemove?: () => void
  offpeak: OffPeak[]
}) {
  const [selected, setSelected] = useState<number | null>(null)
  const slots = kind.slots
  const sel = selected !== null && selected < slots.length ? selected : null
  const setSlots = (next: ChargeSlot[]) => onChange({ ...kind, slots: next })
  const setSlot = (i: number, patch: Partial<ChargeSlot>) =>
    setSlots(slots.map((s, j) => (j === i ? { ...s, ...patch } : s)))
  const cut = sel !== null ? split(slots, sel) : null
  const charging = slots.find((s) => s.mode === "max")
  return (
    <div className="grid gap-3 rounded-2xl border p-4">
      <div className="flex flex-wrap items-center gap-3">
        <Input
          aria-label={t("Nom du type de jour")}
          value={kind.name}
          onChange={(e) => onChange({ ...kind, name: e.target.value })}
          className="h-9 w-48 font-medium"
        />
        <div
          className="flex items-center gap-1.5"
          role="group"
          aria-label={t("Couleur")}
        >
          {PROFILE_COLORS.map((c) => (
            <button
              key={c}
              type="button"
              aria-label={t("Couleur {c}", { c })}
              aria-pressed={kind.color === c}
              onClick={() => onChange({ ...kind, color: c })}
              className={cn(
                "size-6 rounded-full border",
                kind.color === c &&
                  "ring-2 ring-primary ring-offset-2 ring-offset-background"
              )}
              style={{ background: c }}
            />
          ))}
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            setSlots(
              offPeakSlots(
                offpeak,
                { start: "", mode: "max", kw: charging?.kw ?? 7 },
                OFF
              )
            )
            setSelected(null)
          }}
        >
          <Moon /> {t("Heures creuses du contrat")}
        </Button>
        {onRemove && (
          <Button
            variant="ghost"
            size="sm"
            className="ml-auto text-destructive"
            onClick={onRemove}
          >
            <Trash2 /> {t("Supprimer")}
          </Button>
        )}
      </div>
      <Frieze
        kind={kind}
        selected={sel}
        onSelect={(i) => setSelected(sel === i ? null : i)}
        onChange={setSlots}
      />
      {sel !== null ? (
        <div className="grid gap-3 rounded-xl bg-muted/50 p-3">
          <div className="grid gap-1.5">
            <Label>{t("Ce que fait la borne")}</Label>
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              value={slots[sel].mode}
              onValueChange={(v) =>
                v &&
                setSlot(sel, {
                  mode: v as ChargeMode,
                  kw: v === "max" ? (slots[sel].kw ?? 7) : undefined,
                  min_amps: undefined,
                  max_amps: undefined,
                })
              }
              className="flex-wrap justify-start"
            >
              {(["max", "solar", "off"] as ChargeMode[]).map((m) => (
                <ToggleGroupItem key={m} value={m} className={SELECTED}>
                  {MODE_LABEL[m]}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          </div>
          <div className="grid gap-3 sm:grid-cols-[8rem_7rem_7rem_auto] sm:items-end">
            {slots[sel].mode === "max" ? (
              <div className="grid gap-1.5">
                <Label htmlFor={`${kind.id}-kw`}>{t("Jusqu'à (kW)")}</Label>
                <KwInput
                  key={sel}
                  id={`${kind.id}-kw`}
                  value={slots[sel].kw}
                  onChange={(v) => setSlot(sel, { kw: v ?? undefined })}
                />
              </div>
            ) : (
              <span />
            )}
            <div className="grid gap-1.5">
              <Label htmlFor={`${kind.id}-from`}>{t("De")}</Label>
              <Input
                id={`${kind.id}-from`}
                type="time"
                step={1800}
                value={slots[sel].start}
                disabled={sel === 0}
                onChange={(e) =>
                  e.target.value &&
                  setSlots(moveStart(slots, sel, minutes(e.target.value)))
                }
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor={`${kind.id}-to`}>{t("À")}</Label>
              <Input
                id={`${kind.id}-to`}
                type={sel + 1 < slots.length ? "time" : "text"}
                step={1800}
                value={sel + 1 < slots.length ? slots[sel + 1].start : "24:00"}
                disabled={sel + 1 >= slots.length}
                onChange={(e) =>
                  e.target.value &&
                  setSlots(moveStart(slots, sel + 1, minutes(e.target.value)))
                }
              />
            </div>
            <div className="flex gap-2">
              <Button
                variant="outline"
                size="sm"
                disabled={!cut}
                onClick={() => cut && setSlots(cut)}
              >
                <Scissors /> {t("Couper")}
              </Button>
              <Button
                variant="outline"
                size="sm"
                disabled={slots.length < 2}
                onClick={() => {
                  setSlots(merge(slots, sel))
                  setSelected(null)
                }}
              >
                <Merge /> {t("Fusionner")}
              </Button>
            </div>
          </div>
          {slots[sel].mode === "max" && (
            <p className="text-xs text-muted-foreground">
              {t(
                "Quel que soit le soleil. Une borne qui règle seule son courant (délestage) garde sa propre régulation."
              )}
            </p>
          )}
        </div>
      ) : (
        <p className="text-sm text-muted-foreground">
          {t(
            "Touchez une plage pour choisir ce que fait la borne, glissez une limite pour la déplacer (par demi-heure)."
          )}
        </p>
      )}
    </div>
  )
}

/** A profile being written: its name, its kinds of day, its calendar, and who follows it. */
function ProfileForm({
  initial,
  saved,
  offpeak,
  onDone,
}: {
  initial: ChargeProfileInput
  /** `null`: a new profile. */
  saved: ChargeProfileView | null
  offpeak: OffPeak[]
  onDone: (id: number | null) => void
}) {
  const qc = useQueryClient()
  const [draft, setDraft] = useState(initial)
  const issue = problem(draft)
  const changed = JSON.stringify(draft) !== JSON.stringify(initial)
  const done = (p: ChargeProfileView | null) => {
    qc.invalidateQueries({ queryKey: ["charge-profiles"] })
    qc.invalidateQueries({ queryKey: ["charging"] })
    onDone(p?.id ?? null)
  }
  const save = useMutation({
    mutationFn: () =>
      saved
        ? api.saveChargeProfile(saved.id, draft)
        : api.createChargeProfile(draft),
    onSuccess: (p) => {
      toast.success(t("Profil enregistré"))
      done(p)
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const remove = useMutation({
    mutationFn: () => api.deleteChargeProfile(saved!.id),
    onSuccess: () => done(null),
    onError: (e) => toast.error(errorText(e)),
  })
  const setKinds = (profiles: ChargeDayKind[]) =>
    setDraft({ ...draft, profiles })
  return (
    <div className="grid gap-4">
      <Card className="rounded-3xl">
        <CardHeader>
          <div className="flex flex-wrap items-center justify-between gap-3">
            <Input
              aria-label={t("Nom du profil")}
              value={draft.name}
              onChange={(e) => setDraft({ ...draft, name: e.target.value })}
              className="h-10 max-w-xs text-lg font-semibold"
            />
            <div className="flex gap-2">
              {saved && (
                <Button
                  variant="ghost"
                  className="text-destructive"
                  disabled={remove.isPending}
                  onClick={() => remove.mutate()}
                >
                  <Trash2 /> {t("Supprimer le profil")}
                </Button>
              )}
              <Button
                disabled={!!issue || save.isPending || (!!saved && !changed)}
                onClick={() => save.mutate()}
              >
                {t("Enregistrer")}
              </Button>
            </div>
          </div>
          <CardDescription>
            {issue ??
              t(
                "Les types de jour disent ce que fait la borne heure par heure ; le calendrier, quel type suit chaque jour."
              )}
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-3">
          {draft.profiles.map((k, i) => (
            <KindEditor
              key={k.id}
              kind={k}
              offpeak={offpeak}
              onChange={(next) =>
                setKinds(draft.profiles.map((o, j) => (j === i ? next : o)))
              }
              onRemove={
                draft.profiles.length > 1
                  ? () => {
                      const rest = draft.profiles.filter((_, j) => j !== i)
                      const swap = (id?: string) =>
                        id === k.id ? rest[0].id : id
                      const cal = draft.calendar
                      setDraft({
                        ...draft,
                        profiles: rest,
                        calendar: {
                          ...cal,
                          weekdays: cal.weekdays.map((w) => swap(w)!),
                          colors: {
                            blue: swap(cal.colors.blue),
                            white: swap(cal.colors.white),
                            red: swap(cal.colors.red),
                          },
                        },
                      })
                    }
                  : undefined
              }
            />
          ))}
          <div className="flex flex-wrap items-center justify-between gap-3">
            <Button
              variant="outline"
              size="sm"
              disabled={draft.profiles.length >= 10}
              onClick={() =>
                setKinds([
                  ...draft.profiles,
                  {
                    id: newProfileId(draft.profiles),
                    name: t("Type {n}", { n: draft.profiles.length + 1 }),
                    color:
                      PROFILE_COLORS[
                        draft.profiles.length % PROFILE_COLORS.length
                      ],
                    slots: [OFF],
                  },
                ])
              }
            >
              <Plus /> {t("Ajouter un type de jour")}
            </Button>
            <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
              {(["max", "solar", "off"] as ChargeMode[]).map((m) => (
                <span key={m} className="inline-flex items-center gap-1.5">
                  <span
                    className="size-2.5 rounded-[3px] ring-1 ring-border"
                    style={{ background: MODE_COLOR[m] }}
                  />
                  {MODE_LABEL[m]}
                </span>
              ))}
            </div>
          </div>
        </CardContent>
      </Card>
      <div className="grid gap-4">
        <CalendarCard
          tariff={draft}
          dayStartText={t("Le jour du profil commence à")}
          onChange={(calendar) => setDraft({ ...draft, calendar })}
        />
        <Card className="rounded-3xl">
          <CardHeader>
            <CardTitle>{t("Utilisé par")}</CardTitle>
            <CardDescription>
              {t(
                "Sans couleur Tempo connue, un jour suit son jour de la semaine."
              )}
            </CardDescription>
          </CardHeader>
          <CardContent className="grid gap-2 text-sm">
            {saved?.automations.map((name) => (
              <Link
                key={name}
                to="/programmations"
                className="flex items-center gap-2 rounded-2xl bg-muted/60 px-3 py-2.5 hover:bg-muted"
              >
                <Workflow className="size-4 text-muted-foreground" />
                {t("Automatisme « {name} »", { name })}
              </Link>
            ))}
            {saved?.used_by.length ? (
              saved.used_by.map((u) => (
                <Link
                  key={u.device_id}
                  to="/recharge"
                  className="flex items-center gap-2 rounded-2xl bg-muted/60 px-3 py-2.5 hover:bg-muted"
                >
                  <Zap className="size-4 text-primary" />
                  {u.name}
                </Link>
              ))
            ) : (
              <p className="text-muted-foreground">
                {t("Aucune borne : choisissez ce profil sur la page Recharge.")}
              </p>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  )
}

/** Recharge › Profiles: the charging profiles of the home, written by the administrators. */
export function ChargeProfilesPage() {
  const admin = currentUser()?.role === "admin"
  const profiles = useQuery({
    queryKey: ["charge-profiles"],
    queryFn: api.chargeProfiles,
  })
  const installation = useChosenInstallation()
  const tariff = useQuery({
    queryKey: ["tariff", installation?.id],
    queryFn: () => api.tariff(installation!.id),
    enabled: !!installation,
  })
  const offpeak = tariff.data?.contract?.offpeak ?? []
  /** The profile shown: its id, "new" with a template, or the first one. */
  const [shown, setShown] = useState<
    { id: number } | { template: ChargeProfileInput } | null
  >(null)
  const list = profiles.data ?? []
  const current =
    shown && "template" in shown
      ? null
      : (list.find((p) => p.id === (shown as { id: number } | null)?.id) ??
        list[0] ??
        null)
  const initial: ChargeProfileInput | null =
    shown && "template" in shown
      ? shown.template
      : current
        ? {
            name: current.name,
            profiles: current.profiles,
            calendar: current.calendar,
          }
        : null
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
          "Le plan de la semaine d'une borne : quand charger, jusqu'à quelle puissance, ou sur le surplus solaire."
        )}
      />
      {!admin ? (
        <p className="text-sm text-muted-foreground">
          {t("Seuls les administrateurs écrivent les profils de recharge.")}
        </p>
      ) : profiles.isLoading ? (
        <Skeleton className="h-72 w-full rounded-3xl" />
      ) : (
        <div className="grid gap-4">
          <div className="flex flex-wrap items-center gap-2">
            {list.map((p) => (
              <Button
                key={p.id}
                size="sm"
                variant={current?.id === p.id ? "default" : "outline"}
                onClick={() => setShown({ id: p.id })}
              >
                {p.name}
                {p.used_by.length > 0 && (
                  <span className="opacity-70">· {t("actif")}</span>
                )}
              </Button>
            ))}
            <span className="mx-1 text-sm text-muted-foreground">
              {t("Nouveau :")}
            </span>
            {templates(offpeak).map(([label, tpl]) => (
              <Button
                key={label}
                size="sm"
                variant="outline"
                onClick={() => setShown({ template: tpl })}
              >
                {tpl.profiles.some((k) =>
                  k.slots.some((s) => s.mode === "solar")
                ) ? (
                  <Sun />
                ) : (
                  <Plus />
                )}
                {label}
              </Button>
            ))}
          </div>
          {initial ? (
            <ProfileForm
              key={current ? `p${current.id}` : JSON.stringify(initial)}
              initial={initial}
              saved={current}
              offpeak={offpeak}
              onDone={(id) => setShown(id === null ? null : { id })}
            />
          ) : (
            <Card className="rounded-3xl">
              <CardContent className="py-10 text-center text-sm text-muted-foreground">
                {t(
                  "Aucun profil pour l'instant : partez d'un modèle ci-dessus."
                )}
              </CardContent>
            </Card>
          )}
        </div>
      )}
    </>
  )
}
