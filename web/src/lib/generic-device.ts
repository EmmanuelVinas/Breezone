import type { DeviceView, TraitField, TraitInfo } from "@/lib/api"
import { number } from "@/lib/format"
import { lang, t } from "@/lib/i18n"

/** The descriptor of a trait (`name@version`) in the catalog the server serves. */
export function traitOf(catalog: TraitInfo[] | undefined, id: string) {
  const [name, version] = id.split("@")
  return catalog?.find((x) => x.trait === name && String(x.version) === version)
}

/** A field and its step, as this device has them: the unit, bounds, step and values it tells in its state. */
export function fieldOf(
  field: TraitField,
  state: Record<string, unknown> | null | undefined
): TraitField & { step?: number } {
  const told = (name?: string) => (name ? state?.[name] : undefined)
  const num = (name?: string) => {
    const v = told(name)
    return typeof v === "number" ? v : undefined
  }
  const unit = told(field.unit_from)
  const values = told(field.values_from)
  return {
    ...field,
    unit: typeof unit === "string" && unit ? unit : field.unit,
    min: num(field.min_from) ?? field.min,
    max: num(field.max_from) ?? field.max,
    step: num(field.step_from),
    values: Array.isArray(values)
      ? values.filter((v): v is string => typeof v === "string")
      : field.values,
  }
}

/** A value of a field in words, from its descriptor: an enum's label, a number and its unit. */
export function fieldText(field: TraitField, value: unknown): string | null {
  if (value === null || value === undefined) return null
  switch (field.type) {
    case "bool":
      return value ? t("Oui") : t("Non")
    case "enum":
      return field.value_labels?.[String(value)]?.[lang()] ?? String(value)
    case "integer":
    case "number":
      if (typeof value !== "number") return null
      return `${number(value, field.type === "integer" ? 0 : 1)}${field.unit ? ` ${field.unit}` : ""}`
    default:
      return String(value)
  }
}

/**
 * What sums a device up when this client does not know its kind: the `ui.primary` field of the
 * first of its traits that has one, in words.
 */
export function primaryText(
  d: Pick<DeviceView, "traits" | "state">,
  catalog: TraitInfo[] | undefined
): string | null {
  for (const id of d.traits) {
    const trait = traitOf(catalog, id)
    const name = trait?.ui?.primary
    const field = trait?.state.find((f) => f.name === name)
    if (!field) continue
    const state = d.state as Record<string, unknown> | null
    const text = fieldText(fieldOf(field, state), state?.[field.name])
    if (text) return text
  }
  return null
}
