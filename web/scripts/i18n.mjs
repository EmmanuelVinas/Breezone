// Lists the texts of t() / tn() missing from the translations, and the unused translations.
// Usage: node scripts/i18n.mjs [--missing-json]   (exit code 1 when something is missing)
import { readFileSync, readdirSync, statSync } from "node:fs"
import { join } from "node:path"

const root = new URL("../src/", import.meta.url).pathname
const files = []
const walk = (dir) => {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name)
    if (statSync(p).isDirectory()) {
      if (name !== "locales") walk(p)
    } else if (/\.tsx?$/.test(name) && name !== "i18n.ts") files.push(p)
  }
}
walk(root)

// A string literal: "…" or `…` without ${}.
const lit = String.raw`("(?:[^"\\]|\\.)*"|` + "`[^`$]*`)"
const used = new Set()
const unquote = (s) => (s.startsWith("`") ? s.slice(1, -1) : JSON.parse(s))
for (const f of files) {
  const src = readFileSync(f, "utf8")
  for (const m of src.matchAll(new RegExp(String.raw`\bt\(\s*` + lit, "g"))) used.add(unquote(m[1]))
  for (const m of src.matchAll(new RegExp(String.raw`\btn\(\s*[^,]+,\s*` + lit + String.raw`\s*,\s*` + lit, "g"))) {
    used.add(unquote(m[1]))
    used.add(unquote(m[2]))
  }
}

const keys = (lang) => {
  const src = readFileSync(join(root, "locales", `${lang}.ts`), "utf8")
  const out = new Set()
  // Keys are quoted, or bare identifiers once Prettier has removed the quotes.
  const key = String.raw`(` + lit.slice(1, -1) + String.raw`|[\p{L}_$][\p{L}\p{N}_$]*)`
  for (const m of src.matchAll(new RegExp(String.raw`^\s*` + key + String.raw`\s*:`, "gmu")))
    out.add(/^["`]/.test(m[1]) ? unquote(m[1]) : m[1])
  return out
}

let missing = 0
const report = {}
for (const lang of ["en", "es"]) {
  const have = keys(lang)
  const miss = [...used].filter((k) => !have.has(k))
  const unused = [...have].filter((k) => !used.has(k))
  missing += miss.length
  report[lang] = miss
  if (!process.argv.includes("--missing-json")) {
    for (const k of miss) console.log(`${lang}: missing  ${JSON.stringify(k)}`)
    for (const k of unused) console.log(`${lang}: unused   ${JSON.stringify(k)}`)
  }
}
if (process.argv.includes("--missing-json")) console.log(JSON.stringify([...new Set([...report.en, ...report.es])], null, 1))
else console.log(`${used.size} texts, ${missing} missing translations`)
process.exit(missing ? 1 : 0)
