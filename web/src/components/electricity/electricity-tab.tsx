import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  CalendarPlus,
  Euro,
  History,
  Plus,
  Save,
  Trash2,
  Undo2,
} from "lucide-react"
import { toast } from "sonner"
import type {
  Contract,
  DayProfile,
  PriceView,
  Tariff,
  TariffCalendar,
  TariffView,
} from "@/lib/api"
import { api } from "@/lib/api"
import { DAY_LABEL, errorText, time } from "@/lib/format"
import { locale, t } from "@/lib/i18n"
import { Confirm } from "@/components/confirm"
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
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import { Switch } from "@/components/ui/switch"
import {
  money,
  newProfileId,
  OFFER_LABEL,
  PROFILE_COLORS,
  problem,
  template,
  today,
  type Offer,
} from "./model"
import { PriceInput, ProfileEditor } from "./profile-editor"

const COUNTRIES: [string, string][] = [
  ["FR", t("France")],
  ["BE", t("Belgique")],
  ["CH", t("Suisse")],
  ["ES", t("Espagne")],
  ["LU", t("Luxembourg")],
]
const POWERS = [3, 6, 9, 12, 15, 18, 24, 30, 36]
const OFFERS: Offer[] = ["base", "hc", "tempo", "custom"]
const COLORS = [
  ["blue", t("Bleu")],
  ["white", t("Blanc")],
  ["red", t("Rouge")],
] as const

function dayText(day: string) {
  return new Date(`${day}T12:00:00`).toLocaleDateString(locale(), {
    day: "numeric",
    month: "long",
    year: "numeric",
  })
}

const offerOf = (c: Contract | null): Offer =>
  OFFERS.includes(c?.offer as Offer) ? (c!.offer as Offer) : "custom"

function Field({
  label,
  htmlFor,
  children,
}: {
  label: string
  htmlFor?: string
  children: React.ReactNode
}) {
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={htmlFor}>{label}</Label>
      {children}
    </div>
  )
}

/** The price now, and the stretches of the next 24 hours as a bar. */
function PriceNow({ price, tariff }: { price: PriceView; tariff?: Tariff }) {
  const now = price.now
  if (!now) return null
  const color = (id: string) =>
    tariff?.profiles.find((p) => p.id === id)?.color ?? PROFILE_COLORS[3]
  const periods = price.periods
  const first = periods[0]?.start ?? 0
  const span = (periods.at(-1)?.end ?? first + 1) - first
  const prices = periods.map((p) => p.price)
  const low = Math.min(...prices)
  const high = Math.max(...prices)
  const next = periods.find((p) => p.price !== now.price)
  return (
    <Card className="rounded-3xl">
      <CardContent className="grid gap-4">
        <div className="flex flex-wrap items-center gap-4">
          <span className="grid size-14 place-items-center rounded-2xl bg-primary/15 text-primary">
            <Euro className="size-7" aria-hidden />
          </span>
          <div>
            <div className="text-sm text-muted-foreground">
              {t("Prix maintenant")}
            </div>
            <div className="font-heading text-3xl font-semibold tabular-nums">
              {money(now.price, price.currency)}
              <span className="text-base font-normal text-muted-foreground">
                {" "}
                / kWh
              </span>
            </div>
            <div className="text-sm text-muted-foreground">
              {[now.slot, now.profile_name].filter(Boolean).join(" · ")}
              {next &&
                ` · ${t("{slot} à {time} ({price})", {
                  slot: next.slot || t("changement"),
                  time: time(next.start),
                  price: money(next.price, price.currency),
                })}`}
            </div>
          </div>
        </div>
        {periods.length > 0 && (
          <div className="grid gap-1">
            <div className="text-xs text-muted-foreground">
              {t("Les 24 prochaines heures")}
            </div>
            <div className="flex h-7 overflow-hidden rounded-lg">
              {periods.map((p) => (
                <span
                  key={p.start}
                  title={`${time(p.start)} · ${p.slot} · ${money(p.price, price.currency)}`}
                  className="border-r border-background"
                  style={{
                    flex: p.end - p.start,
                    background: `color-mix(in oklch, ${color(p.profile)} ${high === low ? 32 : 14 + Math.round(((p.price - low) / (high - low)) * 30)}%, transparent)`,
                  }}
                />
              ))}
            </div>
            <div className="flex justify-between text-[11px] text-muted-foreground">
              <span>{time(first)}</span>
              <span>{time(first + span / 2)}</span>
              <span>{time(first + span)}</span>
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  )
}

/** Which profile a day follows: by the day of the week, or by the colour announced. */
function CalendarCard({
  tariff,
  onChange,
}: {
  tariff: Tariff
  onChange: (c: TariffCalendar) => void
}) {
  const cal = tariff.calendar
  const byColor = Object.values(cal.colors).some(Boolean)
  const pick = (value: string, set: (v: string) => void, label: string) => (
    <Select value={value} onValueChange={set}>
      <SelectTrigger className="w-full" aria-label={label}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {tariff.profiles.map((p) => (
          <SelectItem key={p.id} value={p.id}>
            <span className="inline-flex items-center gap-2">
              <span
                className="size-2.5 rounded-full"
                style={{ background: p.color ?? PROFILE_COLORS[3] }}
              />
              {p.name}
            </span>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{t("Calendrier")}</CardTitle>
        <CardDescription>{t("Quel profil suit chaque jour")}</CardDescription>
      </CardHeader>
      <CardContent className="grid gap-5">
        <div className="flex items-center justify-between gap-4">
          <div>
            <Label htmlFor="day-start">
              {t("Le jour du tarif commence à")}
            </Label>
            <p className="text-sm text-muted-foreground">
              {t("06:00 pour Tempo : la nuit garde le profil de la veille.")}
            </p>
          </div>
          <Input
            id="day-start"
            type="time"
            step={1800}
            className="w-28"
            value={cal.day_start}
            onChange={(e) =>
              e.target.value && onChange({ ...cal, day_start: e.target.value })
            }
          />
        </div>
        <div className="grid gap-2">
          <Label>{t("Par jour de la semaine")}</Label>
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-4 lg:grid-cols-7">
            {DAY_LABEL.map((d, i) => (
              <div key={d} className="grid gap-1">
                <span className="text-xs text-muted-foreground">{d}</span>
                {pick(
                  cal.weekdays[i],
                  (v) =>
                    onChange({
                      ...cal,
                      weekdays: cal.weekdays.map((w, j) => (j === i ? v : w)),
                    }),
                  d
                )}
              </div>
            ))}
          </div>
        </div>
        <div className="grid gap-3">
          <div className="flex items-center justify-between gap-4">
            <div>
              <Label htmlFor="by-color">
                {t("Selon la couleur annoncée (Tempo)")}
              </Label>
              <p className="text-sm text-muted-foreground">
                {t(
                  "La couleur du jour l'emporte sur le jour de la semaine, dès qu'elle est connue."
                )}
              </p>
            </div>
            <Switch
              id="by-color"
              checked={byColor}
              onCheckedChange={(on) =>
                onChange({
                  ...cal,
                  colors: on
                    ? {
                        blue: tariff.profiles[0]?.id,
                        white: tariff.profiles[1]?.id ?? tariff.profiles[0]?.id,
                        red: tariff.profiles.at(-1)?.id,
                      }
                    : {},
                })
              }
            />
          </div>
          {byColor && (
            <div className="grid gap-2 sm:grid-cols-3">
              {COLORS.map(([key, label]) => (
                <Field key={key} label={t("Jour {color}", { color: label })}>
                  {pick(
                    cal.colors[key] ?? tariff.profiles[0].id,
                    (v) =>
                      onChange({ ...cal, colors: { ...cal.colors, [key]: v } }),
                    label
                  )}
                </Field>
              ))}
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  )
}

/** Settings › Electricity: the contract of the home and its prices, dated. */
export function ElectricityTab() {
  const qc = useQueryClient()
  const view = useQuery({ queryKey: ["tariff"], queryFn: api.tariff })
  const price = useQuery({
    queryKey: ["price"],
    queryFn: () => api.price(24),
    refetchInterval: 60_000,
  })
  // What is being changed: the contract, and the tariff from `from` on (null: as saved).
  const [contract, setContract] = useState<Contract | null>(null)
  const [edit, setEdit] = useState<{ from: string; tariff: Tariff } | null>(
    null
  )
  const [newFrom, setNewFrom] = useState(today())

  const saved = view.data
  const versions = saved?.versions ?? []
  const latest = versions.at(-1)
  const shown =
    edit ?? (latest ? { from: latest.valid_from, tariff: latest.tariff } : null)
  const c: Contract = contract ??
    saved?.contract ?? {
      country: "FR",
      supplier: null,
      offer: "custom",
      power_kva: 9,
      currency: "EUR",
    }
  const dirty = contract !== null || edit !== null
  // No offer stands out before a contract is set.
  const chosen = contract || saved?.contract ? offerOf(c) : null

  const done = (v: TariffView) => {
    qc.setQueryData(["tariff"], v)
    qc.invalidateQueries({ queryKey: ["price"] })
  }
  const save = useMutation({
    mutationFn: async () => {
      let v: TariffView | undefined
      if (contract) v = await api.saveContract(contract)
      if (edit) v = await api.saveTariff(edit.from, edit.tariff)
      return v
    },
    onSuccess: (v) => {
      if (v) done(v)
      setContract(null)
      setEdit(null)
      toast.success(t("Prix de l'électricité enregistrés"))
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const remove = useMutation({
    mutationFn: (from: string) => api.deleteTariff(from),
    onSuccess: (v) => {
      done(v)
      setEdit(null)
    },
    onError: (e) => toast.error(errorText(e)),
  })

  if (view.isLoading) return <Skeleton className="h-96 w-full rounded-3xl" />

  const setTariff = (tariff: Tariff) =>
    setEdit({ from: shown?.from ?? today(), tariff })
  const chooseOffer = (offer: Offer) => {
    setContract({
      ...c,
      offer,
      supplier: offer === "custom" ? c.supplier : (c.supplier ?? "EDF"),
    })
    setEdit({ from: shown?.from ?? today(), tariff: template(offer) })
  }
  const tariff = shown?.tariff
  const wrong = tariff ? problem(tariff) : null

  return (
    <div className="grid gap-6">
      {price.data && <PriceNow price={price.data} tariff={latest?.tariff} />}

      <Card className="rounded-3xl">
        <CardHeader>
          <CardTitle>{t("Contrat")}</CardTitle>
          <CardDescription>
            {t(
              "Un par serveur, comme le compteur que lisent ses modules. Prix TTC, comme sur la facture."
            )}
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-5">
          <div className="grid gap-4 sm:grid-cols-3">
            <Field label={t("Pays")}>
              <Select
                value={c.country ?? "FR"}
                onValueChange={(country) =>
                  setContract({
                    ...c,
                    country,
                    currency: country === "CH" ? "CHF" : "EUR",
                  })
                }
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {COUNTRIES.map(([code, name]) => (
                    <SelectItem key={code} value={code}>
                      {name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
            <Field label={t("Fournisseur")} htmlFor="supplier">
              <Input
                id="supplier"
                value={c.supplier ?? ""}
                placeholder="EDF, TotalEnergies…"
                onChange={(e) =>
                  setContract({ ...c, supplier: e.target.value || null })
                }
              />
            </Field>
            <Field label={t("Puissance souscrite")}>
              <Select
                value={c.power_kva ? String(c.power_kva) : undefined}
                onValueChange={(v) =>
                  setContract({ ...c, power_kva: Number(v) })
                }
              >
                <SelectTrigger className="w-full">
                  <SelectValue placeholder={t("Choisir")} />
                </SelectTrigger>
                <SelectContent>
                  {POWERS.map((p) => (
                    <SelectItem key={p} value={String(p)}>
                      {p} kVA
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
          </div>
          <div className="grid gap-2">
            <Label>{t("Offre")}</Label>
            <div className="flex flex-wrap gap-2">
              {OFFERS.map((o) => (
                <Button
                  key={o}
                  type="button"
                  variant={chosen === o ? "default" : "outline"}
                  aria-pressed={chosen === o}
                  onClick={() => chooseOffer(o)}
                >
                  {OFFER_LABEL[o]}
                </Button>
              ))}
            </div>
            <p className="text-sm text-muted-foreground">
              {t(
                "Choisir une offre remplit la grille avec ses plages et des prix indicatifs : vérifiez-les sur votre facture avant d'enregistrer."
              )}
            </p>
          </div>
        </CardContent>
      </Card>

      {!tariff ? (
        <Card className="rounded-3xl">
          <CardContent className="grid justify-items-start gap-3">
            <p className="text-muted-foreground">
              {t(
                "Aucun prix pour l'instant. Choisissez votre offre ci-dessus pour partir de sa grille, ou une grille personnalisée."
              )}
            </p>
          </CardContent>
        </Card>
      ) : (
        <>
          <Card className="rounded-3xl">
            <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-3">
              <div>
                <CardTitle>{t("Prix")}</CardTitle>
                <CardDescription>
                  {t("À partir du {day}", { day: dayText(shown!.from) })}
                  {edit &&
                    !versions.some((v) => v.valid_from === edit.from) && (
                      <Badge variant="secondary" className="ml-2">
                        {t("nouveaux prix")}
                      </Badge>
                    )}
                </CardDescription>
              </div>
            </CardHeader>
            <CardContent className="grid gap-5">
              <div className="grid gap-4 sm:grid-cols-2">
                <Field
                  label={t("Abonnement ({currency} par mois)", {
                    currency: c.currency,
                  })}
                  htmlFor="subscription"
                >
                  <PriceInput
                    id="subscription"
                    value={tariff.subscription}
                    onChange={(v) =>
                      v !== null && setTariff({ ...tariff, subscription: v })
                    }
                  />
                </Field>
                <Field
                  label={t("Rachat du surplus ({currency} / kWh, facultatif)", {
                    currency: c.currency,
                  })}
                  htmlFor="export"
                >
                  <PriceInput
                    id="export"
                    value={tariff.export_price}
                    onChange={(v) => setTariff({ ...tariff, export_price: v })}
                  />
                </Field>
              </div>
              <div className="grid gap-3">
                <div className="flex items-center justify-between gap-3">
                  <Label>{t("Profils de jour")}</Label>
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={tariff.profiles.length >= 10}
                    onClick={() => {
                      const base = tariff.profiles[0]
                      const p: DayProfile = {
                        ...base,
                        id: newProfileId(tariff.profiles),
                        name: t("Nouveau profil"),
                        color:
                          PROFILE_COLORS[
                            tariff.profiles.length % PROFILE_COLORS.length
                          ],
                      }
                      setTariff({
                        ...tariff,
                        profiles: [...tariff.profiles, p],
                      })
                    }}
                  >
                    <Plus /> {t("Ajouter un profil")}
                  </Button>
                </div>
                {tariff.profiles.map((p, i) => (
                  <ProfileEditor
                    key={p.id}
                    profile={p}
                    currency={c.currency}
                    onChange={(next) =>
                      setTariff({
                        ...tariff,
                        profiles: tariff.profiles.map((x, j) =>
                          j === i ? next : x
                        ),
                      })
                    }
                    onRemove={
                      tariff.profiles.length > 1
                        ? () => {
                            const rest = tariff.profiles.filter(
                              (_, j) => j !== i
                            )
                            const fallback = rest[0].id
                            const cal = tariff.calendar
                            const swap = (id?: string) =>
                              id === p.id ? fallback : id
                            setTariff({
                              ...tariff,
                              profiles: rest,
                              calendar: {
                                ...cal,
                                weekdays: cal.weekdays.map((w) => swap(w)!),
                                colors: Object.fromEntries(
                                  Object.entries(cal.colors).map(([k, v]) => [
                                    k,
                                    swap(v),
                                  ])
                                ),
                              },
                            })
                          }
                        : undefined
                    }
                  />
                ))}
              </div>
            </CardContent>
          </Card>

          <CalendarCard
            tariff={tariff}
            onChange={(calendar) => setTariff({ ...tariff, calendar })}
          />
        </>
      )}

      {dirty && (
        <div className="sticky bottom-4 z-10 flex flex-wrap items-center justify-end gap-3 rounded-2xl border bg-card/95 p-3 shadow-lg backdrop-blur">
          {wrong && (
            <span className="mr-auto text-sm text-destructive">{wrong}</span>
          )}
          <Button
            variant="ghost"
            onClick={() => {
              setContract(null)
              setEdit(null)
            }}
          >
            <Undo2 /> {t("Annuler")}
          </Button>
          <Button
            disabled={save.isPending || !!wrong}
            onClick={() => save.mutate()}
          >
            <Save /> {t("Enregistrer")}
          </Button>
        </div>
      )}

      <Card className="rounded-3xl">
        <CardHeader>
          <CardTitle>{t("Historique des prix")}</CardTitle>
          <CardDescription>
            {t(
              "Chaque changement de prix commence un jour : les coûts passés gardent leurs prix."
            )}
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-4">
          {versions.length > 0 && (
            <ul className="grid">
              {[...versions].reverse().map((v, i) => {
                const until = versions[versions.length - i]?.valid_from
                const current = shown?.from === v.valid_from
                return (
                  <li
                    key={v.valid_from}
                    className="flex flex-wrap items-center gap-3 border-b py-2.5 last:border-b-0"
                  >
                    <History
                      className="size-4 text-muted-foreground"
                      aria-hidden
                    />
                    <div className="min-w-0 flex-1">
                      <div className="font-medium">
                        {until
                          ? t("Du {from} au {until}", {
                              from: dayText(v.valid_from),
                              until: dayText(until),
                            })
                          : t("Depuis le {from}", {
                              from: dayText(v.valid_from),
                            })}
                      </div>
                      <div className="text-sm text-muted-foreground">
                        {v.tariff.profiles.map((p) => p.name).join(", ")} ·{" "}
                        {t("abonnement {price} par mois", {
                          price: money(v.tariff.subscription, c.currency, 2),
                        })}
                      </div>
                    </div>
                    {i === 0 && (
                      <Badge variant="secondary">{t("en cours")}</Badge>
                    )}
                    <Button
                      variant={current ? "secondary" : "outline"}
                      size="sm"
                      disabled={current}
                      onClick={() =>
                        setEdit({ from: v.valid_from, tariff: v.tariff })
                      }
                    >
                      {current ? t("Affichés") : t("Voir et modifier")}
                    </Button>
                    <Confirm
                      title={t("Supprimer ces prix ?")}
                      description={t(
                        "Les jours qu'ils couvraient prendront les prix d'avant."
                      )}
                      onConfirm={() => remove.mutate(v.valid_from)}
                    >
                      <Button
                        variant="ghost"
                        size="icon"
                        aria-label={t("Supprimer ces prix")}
                      >
                        <Trash2 />
                      </Button>
                    </Confirm>
                  </li>
                )
              })}
            </ul>
          )}
          {tariff && (
            <div className="flex flex-wrap items-end gap-3">
              <Field label={t("Nouveaux prix à partir du")} htmlFor="new-from">
                <Input
                  id="new-from"
                  type="date"
                  className="w-44"
                  value={newFrom}
                  onChange={(e) => setNewFrom(e.target.value)}
                />
              </Field>
              <Button
                variant="outline"
                disabled={!newFrom}
                onClick={() =>
                  setEdit({ from: newFrom, tariff: structuredClone(tariff) })
                }
              >
                <CalendarPlus /> {t("Partir des prix affichés")}
              </Button>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
