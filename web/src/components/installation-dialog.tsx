import { useState } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Loader2 } from "lucide-react"
import { toast } from "sonner"
import type { InstallationConfig } from "@/lib/api"
import { api } from "@/lib/api"
import { t } from "@/lib/i18n"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Switch } from "@/components/ui/switch"

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e))

type InstallationForm = {
  name: string
  host: string
  port: string
  enabled: boolean
  record_demand: boolean
}

/** Adds or edits an installation reached directly by this server (or renames a shared one). */
export function InstallationDialog({
  initial,
  onClose,
}: {
  initial?: InstallationConfig | { name: string; host: string; port: number }
  onClose: () => void
}) {
  const qc = useQueryClient()
  const editing = initial && "id" in initial ? initial : undefined
  const [form, setForm] = useState<InstallationForm>({
    name: initial?.name ?? "",
    host: initial?.host ?? "",
    port: String(initial?.port ?? 3000),
    enabled: editing?.enabled ?? true,
    record_demand: editing?.record_demand ?? false,
  })
  // Set on the server that owns the installation: a shared one follows it.
  const owned = !editing || editing.node_id === null
  const [test, setTest] = useState<{ ok: boolean; text: string } | null>(null)
  const set = (p: Partial<InstallationForm>) => setForm((f) => ({ ...f, ...p }))

  const runTest = useMutation({
    mutationFn: () => api.testInstallation(form.host, Number(form.port)),
    onSuccess: (r) =>
      setTest(
        r.ok
          ? {
              ok: true,
              text: t("{n} zone(s) : {names}", {
                n: r.zones?.length ?? 0,
                names: r.zones?.map((z) => z.name).join(", ") ?? "",
              }),
            }
          : { ok: false, text: r.error ?? t("Pas de réponse") }
      ),
    onError: (e) => setTest({ ok: false, text: errorText(e) }),
  })
  const save = useMutation({
    mutationFn: () => {
      const data = {
        name: form.name,
        host: form.host,
        port: Number(form.port),
        enabled: form.enabled,
        ...(owned && { record_demand: form.record_demand }),
      }
      return editing
        ? api.updateInstallation(editing.id, data)
        : api.createInstallation(data)
    },
    onSuccess: () => {
      toast.success(
        editing ? t("Installation modifiée") : t("Installation ajoutée")
      )
      qc.invalidateQueries({ queryKey: ["installations"] })
      qc.invalidateQueries({ queryKey: ["status"] })
      onClose()
    },
    onError: (e) => toast.error(errorText(e)),
  })

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {editing
              ? t("Modifier l'installation")
              : t("Ajouter une installation")}
          </DialogTitle>
          <DialogDescription>
            {t(
              "Adresse du WebServer Airzone sur le réseau local (API locale, port 3000)."
            )}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor="in">{t("Nom")}</Label>
            <Input
              id="in"
              value={form.name}
              placeholder={t("Maison")}
              onChange={(e) => set({ name: e.target.value })}
            />
          </div>
          <div className="grid grid-cols-[1fr_6rem] gap-3">
            <div className="grid gap-2">
              <Label htmlFor="ih">{t("Adresse IP")}</Label>
              <Input
                id="ih"
                value={form.host}
                placeholder="192.168.1.25"
                onChange={(e) => set({ host: e.target.value })}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="ip">{t("Port")}</Label>
              <Input
                id="ip"
                type="number"
                value={form.port}
                onChange={(e) => set({ port: e.target.value })}
              />
            </div>
          </div>
          <label className="flex items-center gap-3 text-sm">
            <Switch
              checked={form.enabled}
              onCheckedChange={(enabled) => set({ enabled })}
            />{" "}
            {t("Activée (interrogée régulièrement)")}
          </label>
          {owned && (
            <label className="flex items-start gap-3 text-sm">
              <Switch
                className="mt-0.5"
                checked={form.record_demand}
                onCheckedChange={(record_demand) => set({ record_demand })}
              />
              <span>
                {t("Enregistrer le temps de demande des zones")}
                <span className="block text-xs text-muted-foreground">
                  {t(
                    "Durée pendant laquelle chaque zone demande du chaud ou du froid, gardée avec l'historique. Les serveurs à qui l'installation est partagée suivent ce réglage."
                  )}
                </span>
              </span>
            </label>
          )}
          {test && (
            <Alert variant={test.ok ? "default" : "destructive"}>
              <AlertTitle>
                {test.ok ? t("WebServer joignable") : t("Échec")}
              </AlertTitle>
              <AlertDescription>{test.text}</AlertDescription>
            </Alert>
          )}
        </div>
        <DialogFooter className="gap-2">
          <Button
            variant="outline"
            onClick={() => runTest.mutate()}
            disabled={!form.host || runTest.isPending}
          >
            {runTest.isPending && <Loader2 className="animate-spin" />}{" "}
            {t("Tester")}
          </Button>
          <Button
            onClick={() => save.mutate()}
            disabled={!form.name || !form.host || save.isPending}
          >
            {t("Enregistrer")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
