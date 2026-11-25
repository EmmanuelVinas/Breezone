import { useState } from "react"
import { Link } from "react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { toast } from "sonner"
import { Clock, Zap } from "lucide-react"
import type {
  ChargeDay,
  ChargeEffect,
  ChargeNext,
  ChargingView,
  DeviceView,
} from "@/lib/api"
import { api, currentUser } from "@/lib/api"
import { errorText, kwh, number, time } from "@/lib/format"
import { locale, t } from "@/lib/i18n"
import { TEMPO_COLOR, TEMPO_LABEL } from "@/lib/tempo"
import { cn } from "@/lib/utils"
import { effectColor, effectText } from "@/lib/charge-effects"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Slider } from "@/components/ui/slider"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

/** What the charger follows and does: its profile, a charge asked now, the next charge. */
export function useCharging(device: DeviceView) {
  return useQuery({
    queryKey: ["charging", device.id],
    queryFn: () => api.charging(device.id),
    refetchInterval: 10_000,
  })
}

function useChargingUpdate(device: DeviceView) {
  const qc = useQueryClient()
  return (view: ChargingView) => {
    qc.setQueryData(["charging", device.id], view)
    qc.invalidateQueries({ queryKey: ["devices"] })
    qc.invalidateQueries({ queryKey: ["charge-profiles"] })
  }
}

const SELECTED =
  "data-[state=on]:border-primary data-[state=on]:bg-primary data-[state=on]:text-primary-foreground"

/** Power of a current on this charger (kW). */
function kwOf(device: DeviceView, amps: number) {
  const s = device.state ?? {}
  const volts = typeof s.voltage === "number" && s.voltage > 0 ? s.voltage : 230
  const phases = typeof s.phases === "number" && s.phases > 0 ? s.phases : 1
  return (amps * volts * phases) / 1000
}

/** "22:00", "demain 06:00", "mardi 06:00". */
function when(ts: number) {
  const d = new Date(ts * 1000)
  const today = new Date()
  const days = Math.round(
    (new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime() -
      new Date(
        today.getFullYear(),
        today.getMonth(),
        today.getDate()
      ).getTime()) /
      86_400_000
  )
  if (days === 0) return time(ts)
  if (days === 1) return t("demain {time}", { time: time(ts) })
  return `${d.toLocaleDateString(locale(), { weekday: "long" })} ${time(ts)}`
}

function sameDay(a: number, b: number) {
  return new Date(a * 1000).toDateString() === new Date(b * 1000).toDateString()
}

function dayName(day: string, index: number) {
  if (index === 0) return t("Aujourd'hui")
  if (index === 1) return t("Demain")
  const d = new Date(`${day}T12:00:00`)
  const name = d.toLocaleDateString(locale(), { weekday: "long" })
  return name.charAt(0).toUpperCase() + name.slice(1)
}

function effectWords(e: ChargeEffect, kw: number | null) {
  if (e.mode === "solar") return t("surplus solaire")
  if (e.mode === "off") return t("pas de charge")
  if (e.mode === "free") return t("la borne décide")
  return t("jusqu'à {kw} kW", { kw: number(kw ?? 0, 1) })
}

/** "Prochaine charge : 22:00 → 06:00", what it does and how much it may give. */
function NextCharge({ next, now }: { next: ChargeNext; now: number }) {
  const hours = (next.end - Math.max(now, next.start)) / 3600
  const details = [
    effectWords(next.effect, next.kw),
    next.automation?.name,
    next.effect.mode !== "solar" && next.kw
      ? t("≈ {kwh} possibles", { kwh: kwh(next.kw * hours) })
      : null,
  ].filter(Boolean)
  return (
    <div className="flex items-start gap-2.5 rounded-2xl bg-muted/60 px-3 py-2.5 text-sm">
      <Clock className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
      <div>
        <div className="font-medium">
          {next.start <= now
            ? t("En charge selon les automatismes jusqu'à {end}", {
                end: when(next.end),
              })
            : t("Prochaine charge : {start} → {end}", {
                start: when(next.start),
                end: sameDay(next.start, next.end)
                  ? time(next.end)
                  : when(next.end),
              })}
        </div>
        <div className="text-xs text-muted-foreground">
          {details.join(" · ")}
        </div>
      </div>
    </div>
  )
}

/** Why the charger does what it does, and when it charges next (in the card of the charger). */
export function PlanLines({
  view,
  at,
}: {
  view: ChargingView | undefined
  /** When the view was read (seconds). */
  at: number
}) {
  if (!view) return null
  // A charge asked now has its own line; the regulator may not have seen yet one called off.
  const reason =
    view.now || view.status?.state === "now" ? null : view.status?.reason
  const next = view.now ? null : view.next
  if (!reason && !next) return null
  return (
    <div className="grid gap-2">
      {reason && <p className="text-sm text-muted-foreground">{reason}</p>}
      {next && <NextCharge next={next} now={at} />}
    </div>
  )
}

/** One day of what the automations plan, from midnight to midnight. */
function DayStrip({
  day,
  index,
  now,
  view,
}: {
  day: ChargeDay
  index: number
  now: number
  view: ChargingView
}) {
  const first = day.segments[0]?.start ?? 0
  const last = day.segments[day.segments.length - 1]?.end ?? first + 86_400
  const span = Math.max(1, last - first)
  return (
    <>
      <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
        {dayName(day.day, index)}
        {day.color && (
          <span
            className="size-2 rounded-full ring-1 ring-border"
            style={{ background: TEMPO_COLOR[day.color] }}
            title={TEMPO_LABEL[day.color]}
          />
        )}
      </span>
      <div className="relative flex h-5 overflow-hidden rounded-md border border-dashed bg-background">
        {day.segments.map((p) => {
          const kw =
            p.effect?.mode === "profile"
              ? (view.profiles.find((x) => x.id === p.effect!.profile_id)?.kw ??
                null)
              : null
          const words = p.effect
            ? p.effect.mode === "profile"
              ? effectText(p.effect, view.profiles)
              : effectWords(p.effect, kw)
            : ""
          const dim =
            !p.effect || p.effect.mode === "off" || p.effect.mode === "free"
          return (
            <div
              key={p.start}
              className={cn(
                "flex items-center justify-center overflow-hidden text-[10px] font-medium whitespace-nowrap",
                dim ? "text-muted-foreground" : "text-primary-foreground"
              )}
              style={{
                width: `${((p.end - p.start) / span) * 100}%`,
                background: effectColor(p.effect, view.profiles),
              }}
              title={`${time(p.start)} → ${time(p.end)}${words ? ` · ${words}` : ""}`}
            >
              {p.end - p.start >= 3 * 3600 && !dim ? words : null}
            </div>
          )
        })}
        {now > first && now < last && (
          <span
            className="absolute inset-y-0 w-0.5 bg-foreground"
            style={{ left: `${((now - first) / span) * 100}%` }}
          />
        )}
      </div>
    </>
  )
}

/** What the automations tell the charger now and over the next days (read only). */
export function ProfileCard({
  view,
  at,
}: {
  device: DeviceView
  view: ChargingView | undefined
  /** When the view was read (seconds). */
  at: number
}) {
  const admin = currentUser()?.role === "admin"
  if (!view) return null
  const current =
    view.mode === "profile"
      ? view.profile_id == null
        ? t("Maximum de la borne")
        : view.profiles.find((p) => p.id === view.profile_id)?.name
      : view.mode === "off"
        ? t("Ne pas charger")
        : view.mode === "solar"
          ? t("Surplus solaire")
          : null
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <div className="flex flex-wrap items-start justify-between gap-2">
          <CardTitle>{t("Ce que prévoient les automatismes")}</CardTitle>
          {admin && (
            <Link
              to="/recharge/profils"
              className="text-sm font-medium text-primary hover:underline"
            >
              {t("Profils ›")}
            </Link>
          )}
        </div>
        <CardDescription>
          {view.own_schedule
            ? null
            : current && view.automation
              ? t("Maintenant : « {what} », choisi par {rule}", {
                  what: current,
                  rule: view.automation.name,
                })
              : current
                ? t("Maintenant : « {what} »", { what: current })
                : view.automations.length
                  ? t("Aucun automatisme n'a encore dit quoi faire à la borne.")
                  : t(
                      "Aucun automatisme ne pilote la borne : elle charge quand la voiture est branchée."
                    )}
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4">
        {view.own_schedule && (
          <div className="rounded-2xl bg-muted/60 px-3 py-2.5 text-sm">
            <div className="font-medium">
              {t("L'appli de la borne décide des heures de charge")}
            </div>
            <div className="text-xs text-muted-foreground">
              {view.automations.length
                ? t(
                    "{rules} ne s'appliquent pas à cette borne. Coupez « Programmation de son appli » dans les réglages.",
                    { rules: view.automations.map((a) => a.name).join(", ") }
                  )
                : t(
                    "Coupez « Programmation de son appli » dans les réglages pour que les automatismes de Breezone décident."
                  )}
            </div>
          </div>
        )}
        {view.days.length > 0 && view.automations.length > 0 && (
          <div className={cn("grid gap-3", view.own_schedule && "opacity-40")}>
            <div className="grid grid-cols-[88px_minmax(0,1fr)] items-center gap-x-3 gap-y-2">
              {view.days.map((d, i) => (
                <DayStrip key={d.day} day={d} index={i} now={at} view={view} />
              ))}
              <span />
              <div className="flex justify-between text-[10px] text-muted-foreground">
                {["0 h", "6 h", "12 h", "18 h", "24 h"].map((h) => (
                  <span key={h}>{h}</span>
                ))}
              </div>
            </div>
            <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
              {[
                ...view.profiles.map(
                  (p) => [p.name, p.color ?? "var(--primary)"] as const
                ),
                [t("Surplus solaire"), "var(--warning)"] as const,
                [t("Ne pas charger"), "var(--muted)"] as const,
              ].map(([label, color]) => (
                <span key={label} className="inline-flex items-center gap-1.5">
                  <span
                    className="size-2.5 rounded-[3px] ring-1 ring-border"
                    style={{ background: color }}
                  />
                  {label}
                </span>
              ))}
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  )
}

type Until = "unplug" | "kwh" | "time"

/** The next time `hhmm` comes (today or tomorrow), as a timestamp. */
function nextTime(hhmm: string) {
  const [h, m] = hhmm.split(":").map(Number)
  const d = new Date()
  d.setHours(h, m, 0, 0)
  if (d.getTime() <= Date.now()) d.setDate(d.getDate() + 1)
  return Math.round(d.getTime() / 1000)
}

/** "Charge now": at some kW, until unplugged, some kWh more or a time. */
export function ChargeNowDialog({
  device,
  open,
  onClose,
}: {
  device: DeviceView
  open: boolean
  onClose: () => void
}) {
  const s = device.state ?? {}
  const lo = kwOf(device, typeof s.limit_min === "number" ? s.limit_min : 6)
  const hi = kwOf(device, typeof s.limit_max === "number" ? s.limit_max : 32)
  const [kw, setKw] = useState(hi)
  const [until, setUntil] = useState<Until>("unplug")
  const [energy, setEnergy] = useState("10")
  const [at, setAt] = useState("07:00")
  const update = useChargingUpdate(device)
  const start = useMutation({
    mutationFn: () =>
      api.chargeNow(device.id, {
        ...(kw < hi - 0.05 ? { kw: Math.round(kw * 10) / 10 } : {}),
        ...(until === "kwh" ? { kwh: Number(energy.replace(",", ".")) } : {}),
        ...(until === "time" ? { until: nextTime(at) } : {}),
      }),
    onSuccess: (v) => {
      update(v)
      onClose()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const amount = Number(energy.replace(",", "."))
  const valid =
    (until !== "kwh" || (amount > 0 && amount <= 200)) &&
    (until !== "time" || /^\d\d:\d\d$/.test(at))
  const presets = [3.7, 7.4].filter((p) => p > lo && p < hi - 0.05)
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t("Charger maintenant")}</DialogTitle>
          <DialogDescription>
            {t(
              "Sans interruption : passe avant les automatismes et le surplus solaire."
            )}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-5">
          <div className="grid gap-2">
            <div className="flex items-baseline justify-between">
              <Label>{t("Puissance")}</Label>
              <span className="tabular text-lg font-semibold">
                {kw >= hi - 0.05
                  ? t("{kw} kW (maximum)", { kw: number(hi, 1) })
                  : `${number(kw, 1)} kW`}
              </span>
            </div>
            <Slider
              min={Math.round(lo * 10) / 10}
              max={Math.round(hi * 10) / 10}
              step={0.1}
              value={[kw]}
              onValueChange={([v]) => setKw(v)}
              aria-label={t("Puissance")}
            />
            <div className="flex flex-wrap gap-1.5">
              {presets.map((p) => (
                <Button
                  key={p}
                  size="sm"
                  variant={Math.abs(kw - p) < 0.05 ? "default" : "outline"}
                  onClick={() => setKw(p)}
                >
                  {number(p, 1)} kW
                </Button>
              ))}
              <Button
                size="sm"
                variant={kw >= hi - 0.05 ? "default" : "outline"}
                onClick={() => setKw(hi)}
              >
                {t("Maximum")}
              </Button>
            </div>
          </div>
          <div className="grid gap-2">
            <Label>{t("Jusqu'à")}</Label>
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              value={until}
              onValueChange={(v) => v && setUntil(v as Until)}
              className="justify-start"
            >
              <ToggleGroupItem value="unplug" className={SELECTED}>
                {t("Débranchement")}
              </ToggleGroupItem>
              <ToggleGroupItem value="kwh" className={SELECTED}>
                {t("Énergie")}
              </ToggleGroupItem>
              <ToggleGroupItem value="time" className={SELECTED}>
                {t("Heure")}
              </ToggleGroupItem>
            </ToggleGroup>
            {until === "kwh" && (
              <div className="flex items-center gap-2 text-sm">
                <span>+</span>
                <Input
                  value={energy}
                  inputMode="decimal"
                  className="w-24"
                  aria-label={t("Énergie à ajouter")}
                  onChange={(e) => setEnergy(e.target.value)}
                />
                <span className="text-muted-foreground">{t("kWh")}</span>
              </div>
            )}
            {until === "time" && (
              <Input
                type="time"
                className="w-32"
                value={at}
                aria-label={t("Heure de fin")}
                onChange={(e) => setAt(e.target.value)}
              />
            )}
          </div>
          {s.auto_limit === true && (
            <p className="text-xs text-muted-foreground">
              {t(
                "La borne garde son délestage : elle baisse seule si la maison consomme trop."
              )}
            </p>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("Annuler")}
          </Button>
          <Button
            onClick={() => start.mutate()}
            disabled={!valid || start.isPending}
          >
            <Zap /> {t("Lancer à {kw} kW", { kw: number(kw, 1) })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** The charge asked now, and how to go back to the automations. */
export function NowLine({
  device,
  view,
}: {
  device: DeviceView
  view: ChargingView | undefined
}) {
  const update = useChargingUpdate(device)
  const cancel = useMutation({
    mutationFn: () => api.cancelChargeNow(device.id),
    onSuccess: update,
    onError: (e) => toast.error(errorText(e)),
  })
  const now = view?.now
  if (!now) return null
  const end =
    now.until !== null
      ? t("jusqu'à {time}", { time: when(now.until) })
      : now.left_kwh !== null
        ? t("encore {kwh}", { kwh: kwh(now.left_kwh) })
        : t("jusqu'au débranchement")
  return (
    <div className="flex flex-wrap items-center justify-between gap-2 rounded-2xl bg-primary/10 px-3 py-2.5 text-sm">
      <span className="inline-flex items-center gap-2 font-medium">
        <Zap className="size-4 text-primary" />
        {t("Charge immédiate à {kw} kW, {end}", {
          kw: number(now.kw ?? kwOf(device, now.amps), 1),
          end,
        })}
      </span>
      {view.can_control && (
        <Button
          size="sm"
          variant="outline"
          disabled={cancel.isPending}
          onClick={() => cancel.mutate()}
        >
          {view.automations.length && !view.own_schedule
            ? t("Revenir aux automatismes")
            : t("Arrêter la charge immédiate")}
        </Button>
      )}
    </div>
  )
}
