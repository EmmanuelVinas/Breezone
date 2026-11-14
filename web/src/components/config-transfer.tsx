import { useRef, useState } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import {
  CircleCheck,
  Download,
  Loader2,
  TriangleAlert,
  Upload,
} from "lucide-react"
import { toast } from "sonner"
import type { ConfigImported } from "@/lib/api"
import { api } from "@/lib/api"
import { t } from "@/lib/i18n"
import { ConfirmDialog } from "@/components/confirm"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e))

/** Export of the configuration to a file, and import of such a file (restore or move). */
export function ConfigTransfer() {
  const qc = useQueryClient()
  const input = useRef<HTMLInputElement>(null)
  const [file, setFile] = useState<{ name: string; data: unknown } | null>(null)
  const [done, setDone] = useState<ConfigImported | null>(null)

  const exported = useMutation({
    mutationFn: api.exportConfig,
    onSuccess: (data) => {
      const day = new Date().toISOString().slice(0, 10)
      const url = URL.createObjectURL(
        new Blob([JSON.stringify(data, null, 2)], { type: "application/json" })
      )
      const a = document.createElement("a")
      a.href = url
      a.download = `breezone-config-${day}.json`
      a.click()
      URL.revokeObjectURL(url)
    },
    onError: (e) => toast.error(errorText(e)),
  })

  const imported = useMutation({
    mutationFn: (data: unknown) => api.importConfig(data),
    onSuccess: (r) => {
      setDone(r)
      qc.invalidateQueries()
      toast.success(t("Configuration importée"))
    },
    onError: (e) => toast.error(errorText(e)),
  })

  const choose = async (picked: File | undefined) => {
    if (!picked) return
    try {
      setFile({ name: picked.name, data: JSON.parse(await picked.text()) })
    } catch {
      toast.error(t("Ce fichier n'est pas une configuration Breezone."))
    }
  }

  const todo = done && [
    ...(done.airzone_password
      ? [
          t(
            "Saisissez à nouveau le mot de passe du compte Airzone (Réglages › Modules › Airzone)."
          ),
        ]
      : []),
    ...done.to_pair.map((name) =>
      t("Appairez à nouveau « {name} » (Réglages › Modules).", { name })
    ),
    ...done.without_password.map((name) =>
      t("Donnez un mot de passe à « {name} » (Réglages › Utilisateurs).", {
        name,
      })
    ),
  ]

  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{t("Configuration")}</CardTitle>
        <CardDescription>
          {t(
            "Un fichier avec tout ce qui a été réglé : installations, modules, appareils et pièces, automatismes, utilisateurs et leurs droits. Pour restaurer ce serveur ou passer sur une autre machine."
          )}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <p className="text-sm text-muted-foreground">
          {t(
            "Il ne contient aucun secret ni historique : ni mots de passe, ni clés des modules, ni partages entre serveurs, ni relevés. Importé sur ce même serveur, ce qu'il a déjà est gardé ; ailleurs, il faudra les ressaisir."
          )}
        </p>
        {done && todo && (
          <Alert>
            {todo.length ? <TriangleAlert /> : <CircleCheck />}
            <AlertTitle>
              {todo.length
                ? t("Configuration importée : reste à faire")
                : t("Configuration importée : rien d'autre à faire")}
            </AlertTitle>
            {todo.length > 0 && (
              <AlertDescription>
                <ul className="ml-4 list-disc space-y-1">
                  {todo.map((line) => (
                    <li key={line}>{line}</li>
                  ))}
                </ul>
              </AlertDescription>
            )}
          </Alert>
        )}
      </CardContent>
      <CardFooter className="flex flex-wrap gap-2">
        <Button
          variant="outline"
          disabled={exported.isPending}
          onClick={() => exported.mutate()}
        >
          {exported.isPending ? (
            <Loader2 className="animate-spin" />
          ) : (
            <Download />
          )}
          {t("Exporter")}
        </Button>
        <Button
          variant="outline"
          disabled={imported.isPending}
          onClick={() => input.current?.click()}
        >
          {imported.isPending ? (
            <Loader2 className="animate-spin" />
          ) : (
            <Upload />
          )}
          {t("Importer…")}
        </Button>
        <input
          ref={input}
          type="file"
          accept="application/json,.json"
          className="hidden"
          onChange={(e) => {
            void choose(e.target.files?.[0])
            e.target.value = ""
          }}
        />
      </CardFooter>
      <ConfirmDialog
        open={file !== null}
        title={t("Remplacer la configuration de ce serveur ?")}
        description={t(
          "Les installations, modules, appareils, pièces, automatismes et droits de ce serveur sont remplacés par ceux de « {name} ». Les utilisateurs du fichier sont ajoutés ou mis à jour, aucun n'est supprimé. Les partages entre serveurs ne changent pas.",
          { name: file?.name ?? "" }
        )}
        onConfirm={() => {
          if (file) imported.mutate(file.data)
          setFile(null)
        }}
        onCancel={() => setFile(null)}
      />
    </Card>
  )
}
