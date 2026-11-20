import { useMemo, useState } from "react"
import { useQuery } from "@tanstack/react-query"
import { Download } from "lucide-react"
import { api } from "@/lib/api"
import { time } from "@/lib/format"
import { t, tn } from "@/lib/i18n"
import { PageHeader } from "@/components/app-shell"
import {
  DRIVER_LABEL,
  downloadCsv,
  isoDay,
  NoMeasure,
  type Period,
} from "@/components/energy/sections"
import { Board } from "@/components/overview/board"
import {
  EnergyPeriodContext,
  useEnergy,
} from "@/components/overview/energy-blocks"
import { useHomeData } from "@/components/overview/home-data"
import { energyLayout } from "@/components/overview/model"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

/** The energy of this server's home: a dashboard of its parts, for a period chosen above. */
export function EnergyPage() {
  const today = isoDay(new Date())
  const [period, setPeriod] = useState<Period>("day")
  const [date, setDate] = useState(today)
  const home = useHomeData()
  const e = useEnergy(period, date)
  const devices = useQuery({ queryKey: ["devices"], queryFn: api.devices })
  const range = useQuery({
    queryKey: ["energy-range"],
    queryFn: api.energyRange,
    staleTime: 3_600_000,
  })
  const generated = useMemo(() => energyLayout(home.priced), [home.priced])

  const firstDay = range.data?.first_day
  const age = firstDay
    ? (range.dataUpdatedAt - new Date(`${firstDay}T00:00:00`).getTime()) /
      86_400_000
    : 0
  const anyRole = e.live && Object.values(e.live.roles).some(Boolean)
  const measured = (devices.data ?? []).filter(
    (d) => d.source_id !== null && d.traits.includes("energy.power@1")
  )
  const products = [
    ...new Set(
      measured
        .filter((d) => d.role)
        .map((d) => DRIVER_LABEL[d.source] ?? d.source)
    ),
  ]
  const description = measured.length
    ? [...products, tn(measured.length, "{n} mesure", "{n} mesures")]
    : []

  const exportCsv = () => {
    if (period === "day" && e.day)
      downloadCsv(`breezone-energie-${date}.csv`, [
        ["heure", "reseau_w", "solaire_w", "batterie_w", "maison_w"],
        ...e.day.points.map((p) => [
          time(p.ts),
          p.grid,
          p.solar,
          p.battery,
          p.home,
        ]),
      ])
    else
      downloadCsv(`breezone-energie-${e.shown[0]?.day ?? today}-${today}.csv`, [
        [
          "jour",
          "consommation_kwh",
          "importe_kwh",
          "injecte_kwh",
          "production_kwh",
          "autoconsomme_kwh",
          "charge_kwh",
          "decharge_kwh",
        ],
        ...e.shown.map((d) => [
          d.day,
          d.consumption,
          d.imported,
          d.exported,
          d.production,
          d.self_consumed,
          d.charged,
          d.discharged,
        ]),
      ])
  }

  if (!e.live || !home.data)
    return (
      <>
        <PageHeader title={t("Énergie")} />
        {home.data || !home.installation ? (
          <Skeleton className="h-96 w-full rounded-3xl" />
        ) : null}
      </>
    )
  if (!anyRole)
    return (
      <>
        <PageHeader title={t("Énergie")} />
        <NoMeasure />
      </>
    )

  return (
    <EnergyPeriodContext.Provider value={{ period, date, setDate }}>
      <Board
        page="energy"
        title={t("Énergie")}
        description={description}
        // The parts of the page show this server's energy, whatever the installation.
        data={{ ...home.data, live: e.live, energy: true }}
        generated={generated}
        shows={(b) => b.type !== "price" || home.priced}
        actions={() => (
          <>
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              value={period}
              onValueChange={(v) => v && setPeriod(v as Period)}
              // The period chosen in the accent colour, as the other choices of the pages.
              className="[&_[data-state=on]]:border-primary [&_[data-state=on]]:bg-primary [&_[data-state=on]]:text-primary-foreground"
            >
              <ToggleGroupItem value="day">{t("Jour")}</ToggleGroupItem>
              <ToggleGroupItem value="week">{t("7 jours")}</ToggleGroupItem>
              <ToggleGroupItem value="month">{t("30 jours")}</ToggleGroupItem>
              {age > 30 && (
                <ToggleGroupItem value="year">{t("Année")}</ToggleGroupItem>
              )}
              {age > 365 && (
                <ToggleGroupItem value="years">{t("5 ans")}</ToggleGroupItem>
              )}
            </ToggleGroup>
            <Button variant="outline" size="sm" onClick={exportCsv}>
              <Download /> {t("Exporter CSV")}
            </Button>
          </>
        )}
      />
    </EnergyPeriodContext.Provider>
  )
}
