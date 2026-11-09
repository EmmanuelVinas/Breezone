// Texts of the web UI: written in French in the code, translated by src/locales/{en,es}.ts
// (keyed by the French text; `npm run i18n` lists what is missing).
import en from "@/locales/en"
import es from "@/locales/es"

export type Lang = "fr" | "en" | "es"

export const LANG_NAMES: Record<Lang, string> = {
  fr: "Français",
  en: "English",
  es: "Español",
}

const STORAGE_KEY = "lang"

function supported(code: string | null | undefined): Lang | null {
  const primary = code?.slice(0, 2).toLowerCase()
  return primary === "fr" || primary === "en" || primary === "es"
    ? primary
    : null
}

export function browserLang(): Lang {
  for (const code of navigator.languages ?? [navigator.language]) {
    const l = supported(code)
    if (l) return l
  }
  return "en"
}

function storedLang(): Lang | null {
  try {
    return supported(localStorage.getItem(STORAGE_KEY))
  } catch {
    return null
  }
}

// Fixed for the life of the page: module-level labels are translated once, and changing the
// language reloads the page.
const current: Lang = storedLang() ?? browserLang()
document.documentElement.lang = current

export function lang(): Lang {
  return current
}

/** Locale for Intl formatting (dates, numbers). */
export function locale(): string {
  return { fr: "fr-FR", en: "en-GB", es: "es-ES" }[current]
}

/**
 * Applies the language of the profile (null: the browser's). Reloads the page when it changes
 * what is shown.
 */
export function setLang(l: Lang | null) {
  try {
    if (l) localStorage.setItem(STORAGE_KEY, l)
    else localStorage.removeItem(STORAGE_KEY)
  } catch {
    // Private mode: the language stays the one of this page.
  }
  if ((l ?? browserLang()) !== current) location.reload()
}

const DICTS: Record<Exclude<Lang, "fr">, Record<string, string>> = { en, es }

type Params = Record<string, string | number>

function fill(text: string, params?: Params) {
  if (!params) return text
  return text.replace(/\{(\w+)\}/g, (m, k: string) =>
    k in params ? String(params[k]) : m
  )
}

/** `t("Supprimer « {name} » ?", { name })`: the text in the current language. */
export function t(fr: string, params?: Params): string {
  return fill(current === "fr" ? fr : (DICTS[current][fr] ?? fr), params)
}

/** Singular or plural form for `n` (available as `{n}`). */
export function tn(n: number, one: string, other: string, params?: Params) {
  const singular = current === "fr" ? Math.abs(n) < 2 : Math.abs(n) === 1
  return t(singular ? one : other, { n, ...params })
}
