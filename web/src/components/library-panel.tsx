import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  Download,
  Loader2,
  RefreshCw,
  Trash2,
  Undo2,
  Upload,
} from "lucide-react"
import { toast } from "sonner"
import type { LibraryModule, ModuleView } from "@/lib/api"
import { api } from "@/lib/api"
import { errorText } from "@/lib/format"
import { lang, t } from "@/lib/i18n"
import { Confirm } from "@/components/confirm"
import { Badge } from "@/components/ui/badge"
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
import { Skeleton } from "@/components/ui/skeleton"

/** Why a module does not work with this server, in words. */
function reasonText(r: LibraryModule["reasons"][number]) {
  switch (r.kind) {
    case "api":
      return t("demande une autre version de l'interface des modules")
    case "breezone":
      return t("demande Breezone {version}", { version: r.required ?? "" })
    case "trait":
    case "service":
      return t("demande {name} (version plus récente)", { name: r.name ?? "" })
    case "account":
      return t("demande le compte {name}", { name: r.id ?? "" })
  }
}

/** A package chosen on disk, in base64. */
async function base64(file: File) {
  const bytes = new Uint8Array(await file.arrayBuffer())
  let text = ""
  for (let i = 0; i < bytes.length; i += 0x8000)
    text += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
  return btoa(text)
}

/**
 * The library of modules: those that work with this server first, installed or not; then the
 * modules installed from a package, with their version, to go back or remove them.
 */
export function LibraryPanel() {
  const qc = useQueryClient()
  const library = useQuery({
    queryKey: ["modules-library"],
    queryFn: () => api.moduleLibrary(false),
    retry: false,
  })
  const modules = useQuery({ queryKey: ["modules"], queryFn: api.modules })
  const changed = () => {
    qc.invalidateQueries({ queryKey: ["modules"] })
    qc.invalidateQueries({ queryKey: ["modules-library"] })
    qc.invalidateQueries({ queryKey: ["drivers"] })
  }
  const refresh = useMutation({
    mutationFn: () => api.moduleLibrary(true),
    onSuccess: (data) => qc.setQueryData(["modules-library"], data),
    onError: (e) => toast.error(errorText(e)),
  })
  const install = useMutation({
    mutationFn: (id: string) => api.installModule(id),
    onSuccess: (m) => {
      toast.success(
        t("{name} {version} installé", {
          name: m.manifest.name[lang()],
          version: m.version,
        })
      )
      changed()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const rollback = useMutation({
    mutationFn: (id: string) => api.rollbackModule(id),
    onSuccess: () => changed(),
    onError: (e) => toast.error(errorText(e)),
  })
  const remove = useMutation({
    mutationFn: (id: string) => api.removeModule(id),
    onSuccess: () => changed(),
    onError: (e) => toast.error(errorText(e)),
  })
  const [pkg, setPkg] = useState<File | null>(null)
  const [sig, setSig] = useState<File | null>(null)
  const upload = useMutation({
    mutationFn: async () =>
      api.uploadModule(await base64(pkg!), (await sig!.text()).trim()),
    onSuccess: (m) => {
      toast.success(
        t("{name} {version} installé", {
          name: m.manifest.name[lang()],
          version: m.version,
        })
      )
      setPkg(null)
      setSig(null)
      changed()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const installed = (modules.data ?? []).filter(
    (m: ModuleView) => m.origin !== "native"
  )

  return (
    <>
      <Card className="rounded-3xl">
        <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-3">
          <div className="grid gap-1.5">
            <CardTitle>{t("Bibliothèque de modules")}</CardTitle>
            <CardDescription>
              {t(
                "Les modules publiés par Breezone, signés comme ses mises à jour. Seuls ceux qui fonctionnent avec ce serveur s'installent ; ils ne se chargent que lorsqu'un appareil ou un compte s'en sert."
              )}
            </CardDescription>
          </div>
          <Button
            variant="outline"
            size="sm"
            disabled={refresh.isPending}
            onClick={() => refresh.mutate()}
          >
            {refresh.isPending ? (
              <Loader2 className="animate-spin" />
            ) : (
              <RefreshCw />
            )}
            {t("Actualiser")}
          </Button>
        </CardHeader>
        <CardContent className="grid gap-2">
          {library.isLoading ? (
            <Skeleton className="h-24 rounded-2xl" />
          ) : library.isError ? (
            <p className="text-sm text-muted-foreground">
              {errorText(library.error)}
            </p>
          ) : (library.data ?? []).length === 0 ? (
            <p className="text-sm text-muted-foreground">
              {t("Aucun module publié pour l'instant.")}
            </p>
          ) : (
            (library.data ?? []).map((m) => {
              const update =
                m.installed && m.fits && m.fits !== m.installed ? m.fits : null
              return (
                <div
                  key={m.id}
                  className="flex flex-wrap items-center gap-3 rounded-2xl border p-3"
                >
                  <div className="grid min-w-0 flex-1 gap-0.5">
                    <span className="truncate font-medium">
                      {m.manifest.name[lang()]}
                    </span>
                    <span className="text-xs text-muted-foreground">
                      {m.fits
                        ? t("version {version}", { version: m.fits })
                        : m.reasons.map(reasonText).join(" · ")}
                    </span>
                  </div>
                  {m.installed && !update ? (
                    <Badge variant="secondary">
                      {t("Installé ({version})", { version: m.installed })}
                    </Badge>
                  ) : m.fits ? (
                    <Button
                      size="sm"
                      disabled={install.isPending}
                      onClick={() => install.mutate(m.id)}
                    >
                      <Download />{" "}
                      {update
                        ? t("Mettre à jour ({version})", { version: update })
                        : t("Installer")}
                    </Button>
                  ) : (
                    <Badge variant="outline">{t("Incompatible")}</Badge>
                  )}
                </div>
              )
            })
          )}
        </CardContent>
      </Card>

      <Card className="rounded-3xl">
        <CardHeader>
          <CardTitle>{t("Modules installés")}</CardTitle>
          <CardDescription>
            {t(
              "Ceux qui viennent de la bibliothèque, de Breezone ou d'un fichier. Ceux qui font partie de Breezone n'y figurent pas."
            )}
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-2">
          {installed.length === 0 && (
            <p className="text-sm text-muted-foreground">
              {t("Aucun pour l'instant.")}
            </p>
          )}
          {installed.map((m) => (
            <div
              key={m.id}
              className="flex flex-wrap items-center gap-3 rounded-2xl border p-3"
            >
              <div className="grid min-w-0 flex-1 gap-0.5">
                <span className="truncate font-medium">
                  {m.manifest.name[lang()]} · {m.version}
                </span>
                <span className="text-xs text-muted-foreground">
                  {m.state === "incompatible"
                    ? t("Ne fonctionne pas avec cette version de Breezone")
                    : m.loaded
                      ? t("En service")
                      : t("Chargé seulement quand il sert")}
                  {m.last_error ? ` · ${m.last_error}` : ""}
                </span>
              </div>
              <Button
                variant="ghost"
                size="sm"
                disabled={rollback.isPending}
                onClick={() => rollback.mutate(m.id)}
              >
                <Undo2 /> {t("Version précédente")}
              </Button>
              <Confirm
                title={t("Retirer « {name} » ?", {
                  name: m.manifest.name[lang()],
                })}
                description={t(
                  "Le module est supprimé de ce serveur. Retirez d'abord ses appareils et son compte."
                )}
                onConfirm={() => remove.mutate(m.id)}
              >
                <Button variant="ghost" size="icon" aria-label={t("Retirer")}>
                  <Trash2 />
                </Button>
              </Confirm>
            </div>
          ))}
          <div className="grid gap-2 border-t pt-4">
            <Label>{t("Installer depuis un fichier")}</Label>
            <p className="text-xs text-muted-foreground">
              {t(
                "Le paquet (.bzm) et sa signature (.bzm.sig) : un module que Breezone n'a pas signé est refusé."
              )}
            </p>
            <div className="flex flex-wrap gap-2">
              <Input
                type="file"
                accept=".bzm"
                aria-label={t("Paquet")}
                className="max-w-60"
                onChange={(e) => setPkg(e.target.files?.[0] ?? null)}
              />
              <Input
                type="file"
                accept=".sig"
                aria-label={t("Signature")}
                className="max-w-60"
                onChange={(e) => setSig(e.target.files?.[0] ?? null)}
              />
              <Button
                variant="outline"
                disabled={!pkg || !sig || upload.isPending}
                onClick={() => upload.mutate()}
              >
                {upload.isPending ? (
                  <Loader2 className="animate-spin" />
                ) : (
                  <Upload />
                )}
                {t("Installer")}
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>
    </>
  )
}
