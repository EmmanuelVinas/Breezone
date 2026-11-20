import { locale, t } from "@/lib/i18n"

/** An event a camera tells by a key (Reolink), in words; other drivers give words already. */
export function cameraEvent(event: string) {
  const known: Record<string, string> = {
    person: t("Personne détectée"),
    vehicle: t("Véhicule détecté"),
    animal: t("Animal détecté"),
    motion: t("Mouvement détecté"),
    ring: t("Coup de sonnette"),
  }
  return known[event] ?? event
}

const num = (v: unknown) => (typeof v === "number" ? v : null)
const fmt = (v: number, digits = 1) =>
  v.toLocaleString(locale(), { maximumFractionDigits: digits })

/** The readings of an environment sensor (environment.sensor@1), short. */
export function sensorText(s: Record<string, unknown>) {
  const parts = [
    num(s.temperature) !== null ? `${fmt(num(s.temperature)!)} °C` : null,
    num(s.humidity) !== null ? `${fmt(num(s.humidity)!, 0)} %` : null,
    num(s.pressure) !== null ? `${fmt(num(s.pressure)!, 0)} hPa` : null,
    num(s.co2) !== null ? `${fmt(num(s.co2)!, 0)} ppm` : null,
    num(s.illuminance) !== null ? `${fmt(num(s.illuminance)!, 0)} lx` : null,
    num(s.battery) !== null
      ? t("batterie {n} %", { n: fmt(num(s.battery)!, 0) })
      : null,
  ].filter(Boolean)
  return parts.length ? parts.join(" · ") : null
}

/** A solar forecast (solar.forecast@1), short. */
export function forecastText(s: Record<string, unknown>) {
  const kwh = (v: number) => `${fmt(v)} kWh`
  const parts = [
    num(s.energy_today) !== null
      ? t("{value} prévus aujourd'hui", { value: kwh(num(s.energy_today)!) })
      : null,
    num(s.energy_remaining) !== null
      ? t("{value} restants", { value: kwh(num(s.energy_remaining)!) })
      : null,
    num(s.energy_tomorrow) !== null
      ? t("{value} demain", { value: kwh(num(s.energy_tomorrow)!) })
      : null,
  ].filter(Boolean)
  return parts.length ? parts.join(" · ") : null
}
