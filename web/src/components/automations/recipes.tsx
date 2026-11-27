import { useState } from "react"
import type {
  AutomationInput,
  AutomationStep,
  AutomationTrigger,
  DeviceView,
} from "@/lib/api"
import { t } from "@/lib/i18n"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import type { Home } from "./fields"
import { MAX_ACTIONS } from "./fields"
import { newTrigger } from "./triggers"
import { SurplusClimateSetup } from "./surplus-climate"

export type Recipe = {
  /** For the recipes other pages open (`/programmations?recette=…`). */
  id?: string
  title: string
  description: string
  rule: AutomationInput
  /** Set up in a form of its own before its rules are made (instead of opening the editor). */
  setup?: "surplus_climate"
}

/** The recipes of the charger the page of the charging profiles offers. */
export const CHARGE_RECIPES = [
  "charge-offpeak",
  "charge-blue-night",
  "charge-solar",
]

export function blank(home: Home): AutomationInput {
  return {
    name: "",
    enabled: true,
    triggers: [newTrigger("measure", home)!],
    match: "all",
    conditions: [],
    actions: [],
    else: [],
    mode: "single",
    over: [],
  }
}

export function recipes(home: Home): Recipe[] {
  const mine = home.devices.filter((d) => home.drivable.has(d.id))
  const has = (d: DeviceView, trait: string) =>
    d.traits.some((x) => x.startsWith(`${trait}@`))
  const zones = mine.filter((d) => has(d, "climate.zone"))
  const charger = mine.find((d) => has(d, "ev.charger"))
  const shutters = mine.filter(
    (d) => has(d, "cover.position") && d.kind !== "window" && d.kind !== "door"
  )
  const windows = mine.filter(
    (d) => has(d, "cover.position") && d.kind === "window"
  )
  const lights = mine.filter((d) => has(d, "light"))
  const humid = home.devices.find(
    (d) => has(d, "ventilation.room") && typeof d.state?.humidity === "number"
  )
  const vmc = humid ?? mine.find((d) => has(d, "ventilation.unit"))
  const step = (
    d: DeviceView,
    trait: string,
    action: string,
    params: Record<string, unknown> = {}
  ): AutomationStep => ({
    kind: "device",
    device_id: d.id,
    trait,
    action,
    params,
  })
  const base = (name: string, triggers: AutomationTrigger[]) => ({
    ...blank(home),
    name,
    triggers,
  })
  const out: Recipe[] = []
  if (charger) {
    const profile = home.chargeProfiles?.[0]?.id ?? null
    const tell = (
      mode: "profile" | "off" | "solar",
      solar?: AutomationStep
    ): AutomationStep =>
      solar ?? {
        kind: "charger_profile",
        device_id: charger.id,
        mode,
        profile_id: mode === "profile" ? profile : null,
      }
    // The off-peak hours of the tariff ("HC"), and its blue days (Tempo).
    const offpeak =
      home.tariff?.slots.find((s) => s.toUpperCase() === "HC") ??
      home.tariff?.slots[0] ??
      "HC"
    const blue = home.tariff?.profiles.find(
      (p) => p.id === "blue" || /bleu|blue/i.test(p.name)
    )
    out.push({
      id: "charge-offpeak",
      title: t("Recharge : charger en heures creuses"),
      description: t(
        "Pendant les heures creuses du tarif, la borne charge (jusqu'à la puissance d'un profil, ou à son maximum) ; en heures pleines, elle ne charge pas."
      ),
      rule: {
        ...base(t("Heures creuses"), [{ kind: "tariff", slot: offpeak }]),
        actions: [tell("profile")],
        over: [tell("off")],
      },
    })
    if (blue)
      out.push({
        id: "charge-blue-night",
        title: t("Recharge : nuit bleue (Tempo)"),
        description: t(
          "Les jours bleus, la borne charge pendant les heures creuses (jusqu'à la puissance d'un profil, ou à son maximum). Les jours blancs et rouges, et en heures pleines, elle ne charge pas."
        ),
        rule: {
          ...base(t("Nuit bleue"), [{ kind: "tariff", slot: offpeak }]),
          conditions: [{ kind: "tariff", day: "today", profile: blue.id }],
          actions: [tell("profile")],
          else: [tell("off")],
          over: [tell("off")],
        },
      })
    if (home.roles.grid)
      out.push({
        id: "charge-solar",
        title: t("Surplus solaire : charger la voiture"),
        description: t(
          "Au lever du soleil, la borne charge sur le surplus (son intensité suit le soleil) ; au coucher, elle ne charge plus."
        ),
        rule: {
          ...base(t("Solaire en journée"), [
            { kind: "sun", event: "sunrise", offset_minutes: 0, days: [] },
            { kind: "sun", event: "sunset", offset_minutes: 0, days: [] },
          ]),
          conditions: [{ kind: "daylight", day: true }],
          actions: [
            {
              kind: "charger_profile",
              device_id: charger.id,
              mode: "solar",
              profile_id: null,
              solar: {},
            },
          ],
          else: [tell("off")],
        },
      })
  }
  if (home.roles.grid && zones.length)
    out.push({
      title: t("Surplus solaire : chauffer ou rafraîchir la maison"),
      description: t(
        "Plutôt que de rendre le surplus au réseau, chauffer les pièces choisies quand il fait froid dehors, ou les rafraîchir quand il fait chaud, jusqu'à la température voulue ; tout remettre comme avant quand le surplus s'arrête."
      ),
      rule: blank(home),
      setup: "surplus_climate",
    })
  // While it is cold (or warm) outside, a room follows the surplus; then it is put back. The mode
  // belongs to the system (its master zone): the room heats only while its system heats.
  const onSurplus = (
    zone: DeviceView,
    name: string,
    mode: "heat" | "cool",
    op: "below" | "above",
    outdoor: number,
    setpoint: number
  ): AutomationInput => ({
    ...base(name, [
      {
        kind: "measure",
        measure: { kind: "outdoor" },
        op,
        value: outdoor,
        for_minutes: 0,
      },
    ]),
    actions: [
      {
        kind: "surplus",
        device_id: zone.id,
        enabled: true,
        power: 1500,
        min_on_minutes: 30,
        mode,
        setpoint,
      },
    ],
    over: [{ kind: "surplus", device_id: zone.id, enabled: false }],
  })
  if (home.roles.grid && zones[0]) {
    out.push({
      title: t("Surplus solaire : chauffer une pièce"),
      description: t(
        "Tant qu'il fait moins de 15 °C dehors, la pièce chauffe à 21 °C quand le surplus suffit (si sa climatisation est en mode chaud), puis revient comme avant."
      ),
      rule: onSurplus(
        zones[0],
        t("Surplus solaire : chauffer le salon"),
        "heat",
        "below",
        15,
        21
      ),
    })
    out.push({
      title: t("Surplus solaire : rafraîchir une pièce"),
      description: t(
        "Tant qu'il fait plus de 25 °C dehors, la pièce se rafraîchit à 25 °C quand le surplus suffit (si sa climatisation est en mode froid), puis revient comme avant."
      ),
      rule: onSurplus(
        zones[0],
        t("Surplus solaire : rafraîchir le salon"),
        "cool",
        "above",
        25,
        25
      ),
    })
  }
  const heater = (home.surplusTargets ?? []).find((x) => !x.zone)
  if (home.roles.grid && heater)
    out.push({
      id: "heater-solar",
      title: t("Surplus solaire : chauffe-eau"),
      description: t(
        "Tant qu'il fait jour, le chauffe-eau (ou une autre prise pilotée) se met en marche quand le surplus suffit à sa puissance, au moins 30 min ; la nuit, il est remis comme avant."
      ),
      rule: {
        ...base(t("Chauffe-eau sur le surplus"), [
          { kind: "daylight", day: true },
        ]),
        actions: [
          {
            kind: "surplus",
            device_id: heater.device_id,
            enabled: true,
            power: heater.power ? Math.round(heater.power) : 2000,
            min_on_minutes: 30,
          },
        ],
        over: [
          { kind: "surplus", device_id: heater.device_id, enabled: false },
        ],
      },
    })
  if (shutters.length) {
    const some = shutters.slice(0, MAX_ACTIONS)
    out.push({
      title: t("Fermer les volets au coucher du soleil"),
      description: t(
        "Chaque soir, au coucher du soleil, fermer les volets (à ajuster)."
      ),
      rule: {
        ...base(t("Fermer les volets le soir"), [
          { kind: "sun", event: "sunset", offset_minutes: 0, days: [] },
        ]),
        actions: some.map((d) =>
          step(d, "cover.position", "set", { position: 0 })
        ),
      },
    })
    out.push({
      title: t("Ouvrir les volets le matin, pas avant 7 h"),
      description: t(
        "Au lever du soleil ou à 7 h, au plus tard des deux : ouvrir les volets."
      ),
      rule: {
        ...base(t("Ouvrir les volets le matin"), [
          { kind: "sun", event: "sunrise", offset_minutes: 0, days: [] },
          { kind: "time", time: "07:00", days: [] },
        ]),
        conditions: [
          { kind: "hours", from: "07:00", to: "12:00" },
          { kind: "daylight", day: true },
        ],
        actions: some.map((d) =>
          step(d, "cover.position", "set", { position: 100 })
        ),
      },
    })
    out.push({
      title: t("Protéger de la chaleur"),
      description: t(
        "Quand le soleil arrive sur la façade sud et qu'il fait plus de 26 °C dehors, fermer les volets à 20 %."
      ),
      rule: {
        ...base(t("Protéger de la chaleur"), [{ kind: "facade", facing: 180 }]),
        conditions: [
          {
            kind: "measure",
            measure: { kind: "outdoor" },
            op: "above",
            value: 26,
          },
        ],
        actions: some.map((d) =>
          step(d, "cover.position", "set", { position: 20 })
        ),
      },
    })
  }
  if (windows[0] && zones[0])
    out.push({
      title: t("Fenêtre ouverte : couper la clim, puis la remettre"),
      description: t(
        "Quand la fenêtre est ouverte depuis 2 min, éteindre la zone ; quand elle se referme, la remettre comme avant."
      ),
      rule: {
        ...base(t("Fenêtre ouverte : couper la clim"), [
          {
            kind: "measure",
            measure: {
              kind: "device",
              device_id: windows[0].id,
              field: "position",
            },
            op: "above",
            value: 0,
            for_minutes: 2,
          },
        ]),
        actions: [
          step(zones[0], "climate.zone", "set", { on: false }),
          {
            kind: "wait_until",
            condition: {
              kind: "measure",
              measure: {
                kind: "device",
                device_id: windows[0].id,
                field: "position",
              },
              op: "below",
              value: 1,
            },
            for_seconds: 0,
            timeout_seconds: 12 * 3600,
            on_timeout: "stop",
          },
          { kind: "restore" },
        ],
      },
    })
  if (windows.length)
    out.push({
      title: t("Pluie annoncée : fermer les fenêtres de toit"),
      description: t(
        "Quand le risque de pluie dans l'heure dépasse 70 %, fermer les fenêtres."
      ),
      rule: {
        ...base(t("Pluie annoncée : fermer les fenêtres"), [
          {
            kind: "measure",
            measure: { kind: "weather", field: "rain_next_hour" },
            op: "above",
            value: 70,
            for_minutes: 0,
          },
        ]),
        actions: windows
          .slice(0, MAX_ACTIONS)
          .map((d) => step(d, "cover.position", "set", { position: 0 })),
      },
    })
  if (lights[0])
    out.push({
      title: t("Allumer une lumière au crépuscule"),
      description: t("Chaque soir au crépuscule, allumer la lumière choisie."),
      rule: {
        ...base(t("Lumière au crépuscule"), [
          { kind: "sun", event: "dusk", offset_minutes: 0, days: [] },
        ]),
        actions: [step(lights[0], "light", "set", { on: true })],
      },
    })
  if (vmc && humid)
    out.push({
      title: t("Salle de bains humide : boost de la VMC"),
      description: t(
        "Quand l'humidité de la pièce dépasse 75 % pendant 2 min, lancer un boost de 30 min."
      ),
      rule: {
        ...base(t("Boost VMC à la douche"), [
          {
            kind: "measure",
            measure: { kind: "device", device_id: humid.id, field: "humidity" },
            op: "above",
            value: 75,
            for_minutes: 2,
          },
        ]),
        actions: [
          step(
            vmc,
            has(vmc, "ventilation.room")
              ? "ventilation.room"
              : "ventilation.unit",
            "boost",
            { minutes: 30 }
          ),
        ],
      },
    })
  if (zones.length)
    out.push({
      title: t("Tout le monde est parti : éteindre la climatisation"),
      description: t(
        "Quand le dernier quitte la maison, éteindre les zones et prévenir."
      ),
      rule: {
        ...base(t("Maison vide"), [{ kind: "presence", home: false }]),
        actions: [
          ...zones
            .slice(0, MAX_ACTIONS - 1)
            .map((d) => step(d, "climate.zone", "set", { on: false })),
          {
            kind: "notify",
            message: t(
              "Tout le monde est parti : la climatisation est éteinte."
            ),
          },
        ],
      },
    })
  if (home.prices?.tempo)
    out.push({
      title: t("Demain jour rouge : prévenir"),
      description: t(
        "Dès que demain est annoncé rouge (Tempo), prévenir pour décaler ce qui peut l'être."
      ),
      rule: {
        ...base(t("Demain jour rouge"), [
          {
            kind: "measure",
            measure: { kind: "tempo", day: "tomorrow" },
            op: "above",
            value: 2,
            for_minutes: 0,
          },
        ]),
        actions: [
          {
            kind: "notify",
            message: t(
              "Demain est un jour rouge Tempo : l'électricité sera chère de 6 h à 22 h."
            ),
          },
        ],
      },
    })
  out.push({
    title: t("Vigilance orange ou rouge : prévenir"),
    description: t(
      "Quand Météo-France passe le département en vigilance orange ou rouge, prévenir."
    ),
    rule: {
      ...base(t("Vigilance météo"), [
        {
          kind: "measure",
          measure: { kind: "weather", field: "vigilance" },
          op: "above",
          value: 1,
          for_minutes: 0,
        },
      ]),
      actions: [
        {
          kind: "notify",
          message: t("Vigilance météo orange ou rouge dans le département."),
        },
      ],
    },
  })
  const sun = home.devices.find((d) => has(d, "solar.forecast"))
  if (sun)
    out.push({
      title: t("Belle journée solaire demain : prévenir"),
      description: t(
        "À 20 h, si la production prévue demain dépasse 10 kWh, prévenir pour y décaler lessives et recharges."
      ),
      rule: {
        ...base(t("Soleil demain"), [
          { kind: "time", time: "20:00", days: [] },
        ]),
        conditions: [
          {
            kind: "measure",
            measure: {
              kind: "device",
              device_id: sun.id,
              field: "energy_tomorrow",
            },
            op: "above",
            value: 10,
          },
        ],
        actions: [
          {
            kind: "notify",
            message: t(
              "Beaucoup de soleil prévu demain : c'est le jour pour les lessives et la recharge."
            ),
          },
        ],
      },
    })
  if (zones.length)
    out.push({
      title: t("Gel annoncé : garder la maison hors gel"),
      description: t(
        "À 22 h, si la minimale de demain est sous 0 °C, chauffer les zones à 16°."
      ),
      rule: {
        ...base(t("Gel annoncé"), [{ kind: "time", time: "22:00", days: [] }]),
        conditions: [
          {
            kind: "measure",
            measure: { kind: "weather", field: "tomorrow_min" },
            op: "below",
            value: 0,
          },
        ],
        actions: zones.slice(0, MAX_ACTIONS).map((d) =>
          step(d, "climate.zone", "set", {
            on: true,
            mode: "heating",
            setpoint: 16,
          })
        ),
      },
    })
  return out
}

export function RecipesDialog({
  home,
  onPick,
  onClose,
}: {
  home: Home
  onPick: (rule: AutomationInput) => void
  onClose: () => void
}) {
  const list = recipes(home)
  const [setup, setSetup] = useState<Recipe | null>(null)
  const zones = home.devices.filter(
    (d) =>
      home.drivable.has(d.id) &&
      d.traits.some((x) => x.startsWith("climate.zone@"))
  )
  if (setup)
    return (
      <Dialog open onOpenChange={(o) => !o && onClose()}>
        <DialogContent className="max-h-[90svh] overflow-y-auto sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{setup.title}</DialogTitle>
            <DialogDescription>{setup.description}</DialogDescription>
          </DialogHeader>
          <SurplusClimateSetup
            home={home}
            zones={zones}
            onDone={onClose}
            onBack={() => setSetup(null)}
          />
        </DialogContent>
      </Dialog>
    )
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90svh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("Recettes")}</DialogTitle>
          <DialogDescription>
            {t(
              "Des règles toutes prêtes, faites avec vos appareils : choisissez-en une, ajustez 2 ou 3 valeurs, enregistrez."
            )}
          </DialogDescription>
        </DialogHeader>
        {list.length ? (
          <div className="grid gap-2">
            {list.map((r) => (
              <button
                key={r.title}
                type="button"
                className="grid gap-1 rounded-2xl border p-3 text-left transition-colors hover:bg-muted/50"
                onClick={() => (r.setup ? setSetup(r) : onPick(r.rule))}
              >
                <span className="font-medium">{r.title}</span>
                <span className="text-sm text-muted-foreground">
                  {r.description}
                </span>
              </button>
            ))}
          </div>
        ) : (
          <p className="text-sm text-muted-foreground">
            {t(
              "Aucune recette ne correspond encore à vos appareils (borne, volets, lumières, VMC, mesure d'énergie)."
            )}
          </p>
        )}
      </DialogContent>
    </Dialog>
  )
}

// ---------------------------------------------------------------------------------------
// The list
// ---------------------------------------------------------------------------------------
