import { useQuery } from "@tanstack/react-query"
import { Link } from "react-router"
import { AirVent, ExternalLink, PlugZap, Power, Sun } from "lucide-react"
import type { SurplusLoad, SurplusStatus } from "@/lib/api"
import { api } from "@/lib/api"
import { locale, t } from "@/lib/i18n"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"

const kw = (w: number) =>
  `${(Math.round(w / 100) / 10).toLocaleString(locale(), { maximumFractionDigits: 1 })} kW`

const TONE: Record<SurplusStatus["state"], string> = {
  solar: "var(--sun, var(--warning))",
  guarantee: "var(--primary)",
  waiting: "var(--muted-foreground)",
  no_car: "var(--muted-foreground)",
  unavailable: "var(--destructive)",
  no_meter: "var(--destructive)",
  idle: "var(--muted-foreground)",
  manual: "var(--primary)",
  held: "var(--primary)",
  no_charge: "var(--muted-foreground)",
  charging: "var(--primary)",
  now: "var(--primary)",
}

const ICON = { charger: PlugZap, relay: Power, climate: AirVent }

export function useSurplus() {
  return useQuery({
    queryKey: ["surplus"],
    queryFn: api.surplus,
    refetchInterval: 10_000,
  })
}

/** "2 kW · 30 min au moins", "chauffe à 20 °C · 1,5 kW", "jusqu'à 3,7 kW". */
function summary(l: SurplusLoad) {
  if (l.kind === "charger") {
    const s = l.solar
    return [
      s?.kw ? t("jusqu'à {kw}", { kw: kw(s.kw * 1000) }) : null,
      s?.start_kw
        ? t("dès {kw} de surplus", { kw: kw(s.start_kw * 1000) })
        : null,
    ]
      .filter(Boolean)
      .join(" · ")
  }
  return [
    l.mode === "heat" && l.setpoint !== null
      ? t("chauffe à {sp} °C", { sp: l.setpoint })
      : null,
    l.mode === "cool" && l.setpoint !== null
      ? t("rafraîchit à {sp} °C", { sp: l.setpoint })
      : null,
    l.power ? kw(l.power) : null,
    l.min_on_minutes ? t("{n} min au moins", { n: l.min_on_minutes }) : null,
  ]
    .filter(Boolean)
    .join(" · ")
}

/**
 * The devices automations put on the solar surplus, in the order it is shared (their priority),
 * and what each does now. Read only: it is set in the automations.
 */
export function SurplusProgramme() {
  const surplus = useSurplus()
  const v = surplus.data
  if (!v || v.loads.length === 0) return null
  return (
    <Card className="rounded-3xl">
      <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-3">
        <div className="grid gap-1.5">
          <CardTitle className="flex items-center gap-2">
            <Sun className="size-5 text-warning" />
            {t("Sur le surplus solaire")}
          </CardTitle>
          <CardDescription>
            {v.meter === false
              ? t(
                  "Il faut un compteur au tableau (une voie « réseau ») pour connaître le surplus."
                )
              : v.export !== null && v.export > 0
                ? t(
                    "Par ordre de priorité · {kw} repartent au réseau en ce moment",
                    { kw: kw(v.export) }
                  )
                : t(
                    "Par ordre de priorité : le surplus va d'abord au premier."
                  )}
          </CardDescription>
        </div>
        <Button asChild variant="outline" size="sm">
          <Link to="/programmations">
            {t("Automatismes")} <ExternalLink />
          </Link>
        </Button>
      </CardHeader>
      <CardContent>
        <ol className="grid">
          {v.loads.map((l, i) => {
            const Icon = ICON[l.kind]
            const line = (
              <>
                <span className="grid size-7 place-items-center rounded-lg bg-muted text-xs text-muted-foreground">
                  {l.priority ?? i + 1}
                </span>
                <Icon
                  className="size-5 shrink-0"
                  style={{
                    color: l.status ? TONE[l.status.state] : undefined,
                  }}
                />
                <div className="grid min-w-0 flex-1">
                  <span className="font-medium">{l.name}</span>
                  <span className="text-xs text-muted-foreground">
                    {[
                      l.automation
                        ? t("« {name} »", { name: l.automation })
                        : null,
                      summary(l),
                    ]
                      .filter(Boolean)
                      .join(" · ")}
                  </span>
                </div>
                {l.status && (
                  <span
                    className="text-right text-xs"
                    style={{ color: TONE[l.status.state] }}
                  >
                    {l.status.text}
                  </span>
                )}
              </>
            )
            return (
              <li key={l.device_id} className="border-b last:border-b-0">
                {l.automation_id !== null ? (
                  <Link
                    to={`/programmations?automation=${l.automation_id}`}
                    className="-mx-2 flex items-center gap-3 rounded-xl px-2 py-3 hover:bg-muted/50"
                  >
                    {line}
                  </Link>
                ) : (
                  <div className="flex items-center gap-3 py-3">{line}</div>
                )}
              </li>
            )
          })}
        </ol>
        <p className="mt-2 text-xs text-muted-foreground">
          {t(
            "Chaque ligne ouvre son automatisme ; l'ordre se change avec « Priorités » dans Automatismes."
          )}
        </p>
      </CardContent>
    </Card>
  )
}
