import { useState, type FormEvent } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Loader2, Plus, Save } from "lucide-react"
import { toast } from "sonner"
import { api, type DriverInfo, type SourceView } from "@/lib/api"
import { errorText } from "@/lib/format"
import { t } from "@/lib/i18n"
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

type Field = DriverInfo["config"][number]

/** The fields of a driver beyond its address, typed by hand (credentials, keys, panels…). */
export function extraFields(info: DriverInfo | undefined): Field[] {
  return (info?.config ?? []).filter(
    (f) => f.name !== "host" && f.name !== "device_id"
  )
}

export function useDrivers() {
  return useQuery({
    queryKey: ["drivers"],
    queryFn: api.drivers,
    staleTime: Infinity,
  })
}

/** The values typed, as the server reads them: numbers as numbers, blanks left out. */
function configOf(fields: Field[], values: Record<string, string>) {
  const config: Record<string, unknown> = {}
  for (const f of fields) {
    const v = (values[f.name] ?? "").trim()
    if (!v) continue
    config[f.name] =
      f.type === "number" || f.type === "integer"
        ? Number(v.replace(",", "."))
        : v
  }
  return config
}

function missing(fields: Field[], values: Record<string, string>) {
  return fields.some(
    (f) =>
      !f.optional && f.type !== "password" && !(values[f.name] ?? "").trim()
  )
}

function Fields({
  fields,
  values,
  onChange,
  keptSecrets,
  id,
}: {
  fields: Field[]
  values: Record<string, string>
  onChange: (name: string, value: string) => void
  /** Editing: a password left blank stays as it is. */
  keptSecrets?: boolean
  id: string
}) {
  return (
    <div className="grid gap-3 sm:grid-cols-2">
      {fields.map((f) => (
        <div key={f.name} className="grid gap-1.5">
          <Label htmlFor={`${id}-${f.name}`}>{f.label}</Label>
          <Input
            id={`${id}-${f.name}`}
            type={f.type === "password" ? "password" : "text"}
            inputMode={
              f.type === "number"
                ? "decimal"
                : f.type === "integer"
                  ? "numeric"
                  : undefined
            }
            autoComplete={f.type === "password" ? "new-password" : "off"}
            value={values[f.name] ?? ""}
            placeholder={
              f.type === "password" && keptSecrets
                ? t("inchangé")
                : f.optional
                  ? t("facultatif")
                  : undefined
            }
            onChange={(e) => onChange(f.name, e.target.value)}
          />
        </div>
      ))}
    </div>
  )
}

/** Adding a module whose driver asks more than its address. */
export function AddWithConfig({
  info,
  name,
  host,
  deviceId,
  note,
  onDone,
  onCancel,
}: {
  info: DriverInfo
  name: string
  host?: string
  deviceId?: string | null
  note?: string | null
  onDone: (s: SourceView) => void
  onCancel: () => void
}) {
  const [values, setValues] = useState<Record<string, string>>(
    host ? { host } : {}
  )
  const fields = info.config.filter((f) => f.name !== "device_id")
  const add = useMutation({
    mutationFn: () =>
      api.createSource({
        driver: info.id,
        name,
        config: {
          ...configOf(fields, values),
          ...(deviceId ? { device_id: deviceId } : {}),
        },
      }),
    onSuccess: onDone,
    onError: (e) => toast.error(errorText(e)),
  })
  const submit = (e: FormEvent) => {
    e.preventDefault()
    add.mutate()
  }
  return (
    <form
      className="grid gap-3 rounded-2xl border border-primary/40 bg-primary/5 p-3"
      onSubmit={submit}
    >
      <div className="text-sm font-medium">
        {t("Ajouter « {name} »", { name })}
      </div>
      {note && <p className="text-xs text-muted-foreground">{note}</p>}
      <Fields
        id="add"
        fields={fields}
        values={values}
        onChange={(k, v) => setValues((o) => ({ ...o, [k]: v }))}
      />
      <div className="flex flex-wrap gap-2">
        <Button
          type="submit"
          disabled={add.isPending || missing(fields, values)}
        >
          {add.isPending ? <Loader2 className="animate-spin" /> : <Plus />}
          {t("Ajouter")}
        </Button>
        <Button type="button" variant="ghost" onClick={onCancel}>
          {t("Annuler")}
        </Button>
      </div>
    </form>
  )
}

/** Settings › Modules › a module: what was typed when it was added, to change. */
export function ModuleSettingsCard({ source }: { source: SourceView }) {
  const qc = useQueryClient()
  const drivers = useDrivers()
  const info = drivers.data?.find((d) => d.id === source.driver)
  const fields = extraFields(info)
  const initial = () =>
    Object.fromEntries(
      fields
        .filter((f) => f.type !== "password")
        .map((f) => [
          f.name,
          source.config[f.name] == null ? "" : String(source.config[f.name]),
        ])
    )
  const [values, setValues] = useState<Record<string, string> | null>(null)
  const shown = values ?? initial()
  const save = useMutation({
    mutationFn: () =>
      api.updateSource(source.id, {
        driver: source.driver,
        name: source.name,
        enabled: source.enabled,
        config: {
          ...Object.fromEntries(
            Object.entries(source.config).filter(
              ([k]) => !fields.some((f) => f.name === k)
            )
          ),
          ...configOf(fields, shown),
        },
      }),
    onSuccess: () => {
      toast.success(t("Réglages enregistrés"))
      setValues(null)
      qc.invalidateQueries({ queryKey: ["sources"] })
      qc.invalidateQueries({ queryKey: ["devices"] })
    },
    onError: (e) => toast.error(errorText(e)),
  })
  if (fields.length === 0) return null
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{t("Réglages")}</CardTitle>
        <CardDescription>
          {t(
            "Les mots de passe et clés restent chiffrés sur le serveur : laissés vides, ils ne changent pas."
          )}
        </CardDescription>
      </CardHeader>
      <CardContent>
        <form
          className="grid gap-3"
          onSubmit={(e) => {
            e.preventDefault()
            save.mutate()
          }}
        >
          <Fields
            id={`source-${source.id}`}
            fields={fields}
            values={shown}
            keptSecrets
            onChange={(k, v) => setValues({ ...shown, [k]: v })}
          />
          <div>
            <Button
              type="submit"
              variant="outline"
              disabled={!values || save.isPending || missing(fields, shown)}
            >
              {save.isPending ? <Loader2 className="animate-spin" /> : <Save />}
              {t("Enregistrer")}
            </Button>
          </div>
        </form>
      </CardContent>
    </Card>
  )
}
