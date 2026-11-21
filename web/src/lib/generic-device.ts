import type { DeviceView, TraitField, TraitInfo } from "@/lib/api"
import { number } from "@/lib/format"
import { lang, t } from "@/lib/i18n"

/** The descriptor of a trait (`name@version`) in the catalog the server serves. */
export function traitOf(catalog: TraitInfo[] | undefined, id: string) {
  const [name, version] = id.split("@")
  return catalog?.find((x) => x.trait === name && String(x.version) === version)
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
    const text = fieldText(
      field,
      (d.state as Record<string, unknown> | null)?.[field.name]
    )
    if (text) return text
  }
  return null
}
