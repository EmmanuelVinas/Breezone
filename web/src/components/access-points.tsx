import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  CheckCircle2,
  Loader2,
  MapPin,
  Network,
  Plus,
  Server,
  TriangleAlert,
} from "lucide-react"
import { toast } from "sonner"
import type { AttachResult, CloudInstallation } from "@/lib/api"
import { api } from "@/lib/api"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { AddNodeDialog, NodeStateBadge } from "@/components/federation"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { t, tn } from "@/lib/i18n"

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e))

/** How this server reaches an installation's WebServer. */
export function accessSummary(
  c: CloudInstallation,
  defaultSubnet: string | null
) {
  if (c.access)
    return t("Adresse {address}", {
      address: `${c.access.host}:${c.access.port}`,
    })
  return defaultSubnet
    ? t("Recherche sur {subnet}", { subnet: defaultSubnet })
    : t("Réseau local à renseigner")
}

/**
 * How this server reaches one Airzone installation:
 * - on this server's network: WebServer found by the scan, or its address set by hand (checked
 *   against the installation's MAC address);
 * - through another Breezone server installed on that site (ADR 0008);
 * - or it is an installation already added here, which is then linked to it.
 */
export function AccessDialog({
  inst,
  defaultSubnet,
  onClose,
}: {
  inst: CloudInstallation
  defaultSubnet: string | null
  onClose: () => void
}) {
  const qc = useQueryClient()
  const installations = useQuery({
    queryKey: ["installations"],
    queryFn: api.installations,
  })
  const nodes = useQuery({ queryKey: ["nodes"], queryFn: api.nodes })
  const linked = installations.data?.find((i) => i.id === inst.installation_id)
  const [mode, setMode] = useState<"local" | "node">(
    linked?.node_id != null ||
      (!inst.access && (nodes.data?.length ?? 0) > 0 && !inst.reachable)
      ? "node"
      : "local"
  )
  const [host, setHost] = useState(inst.access?.host ?? "")
  const [port, setPort] = useState(String(inst.access?.port ?? 3000))
  const [result, setResult] = useState<AttachResult | null>(null)
  const [addingNode, setAddingNode] = useState(false)
  const refresh = () => {
    qc.invalidateQueries({ queryKey: ["cloud"] })
    qc.invalidateQueries({ queryKey: ["status"] })
    qc.invalidateQueries({ queryKey: ["installations"] })
  }
  const save = useMutation({
    mutationFn: () =>
      api.setAccess(inst.cloud_id, {
        host: host.trim() || null,
        port: Number(port) || 3000,
      }),
    onSuccess: (r) => {
      setResult(r.result)
      qc.setQueryData(["cloud"], r.status)
      refresh()
      if (r.result.attached) toast.success(r.result.message)
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const link = useMutation({
    mutationFn: (id: number) => api.linkCloud(id, inst.cloud_id),
    onSuccess: (i) => {
      refresh()
      toast.success(
        t("« {name} » est pilotée par ce serveur ({local})", {
          name: inst.name,
          local: i.name,
        })
      )
      onClose()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  // Installations already here that could be this one.
  const candidates = (installations.data ?? []).filter(
    (i) =>
      (mode === "node" ? i.node_id !== null : i.node_id === null) &&
      (i.cloud_id === null || i.cloud_id === inst.cloud_id)
  )

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("Accès · {name}", { name: inst.name })}</DialogTitle>
          <DialogDescription>
            {inst.ws_ids.length > 0
              ? t(
                  "Comment ce serveur joint le WebServer Airzone de cette installation (MAC {mac}).",
                  {
                    mac: inst.ws_ids.join(", "),
                  }
                )
              : t(
                  "Comment ce serveur joint le WebServer Airzone de cette installation."
                )}
          </DialogDescription>
        </DialogHeader>
        <ToggleGroup
          type="single"
          variant="outline"
          className="w-full"
          value={mode}
          onValueChange={(v) => v && setMode(v as "local" | "node")}
        >
          <ToggleGroupItem value="local" className="flex-1">
            <Network /> {t("Réseau de ce serveur")}
          </ToggleGroupItem>
          <ToggleGroupItem value="node" className="flex-1">
            <Server /> {t("Via un autre serveur")}
          </ToggleGroupItem>
        </ToggleGroup>

        {mode === "local" ? (
          <div className="grid grid-cols-[1fr_6rem] gap-3">
            <div className="grid gap-2">
              <Label htmlFor="ah">{t("Adresse IP ou nom")}</Label>
              <Input
                id="ah"
                value={host}
                placeholder={
                  defaultSubnet
                    ? t("vide = recherche sur {subnet}", {
                        subnet: defaultSubnet,
                      })
                    : t("vide = recherche")
                }
                onChange={(e) => setHost(e.target.value)}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="ap">{t("Port")}</Label>
              <Input
                id="ap"
                type="number"
                value={port}
                onChange={(e) => setPort(e.target.value)}
              />
            </div>
            <p className="col-span-2 text-xs text-muted-foreground">
              {t(
                "Le WebServer est sur le même réseau que ce serveur. Breezone vérifie qu'il s'agit bien de celui de cette installation (adresse MAC)."
              )}
            </p>
          </div>
        ) : (
          <div className="grid gap-3">
            <p className="text-sm text-muted-foreground">
              {t(
                "L'installation est sur un autre site, pilotée par le serveur Breezone installé là-bas. Sur ce serveur : Réglages › Partages › Nouveau partage, puis ajoutez son jeton ici. Les installations partagées sont reconnues automatiquement (même WebServer)."
              )}
            </p>
            {(nodes.data ?? []).map((n) => (
              <div
                key={n.id}
                className="flex items-center gap-2 rounded-xl border p-2.5 text-sm"
              >
                <Server className="size-4 text-muted-foreground" />
                <span className="flex-1">{n.name}</span>
                <NodeStateBadge state={n.status.state} />
              </div>
            ))}
            <Button
              variant="outline"
              className="justify-self-start"
              onClick={() => setAddingNode(true)}
            >
              <Plus /> {t("Ajouter un serveur Breezone")}
            </Button>
          </div>
        )}

        {candidates.length > 0 && (
          <div className="grid gap-2 rounded-2xl border border-dashed p-3">
            <Label>{t("C'est une installation déjà présente ici")}</Label>
            <Select
              value={
                linked && candidates.includes(linked) ? String(linked.id) : ""
              }
              onValueChange={(v) => link.mutate(Number(v))}
              disabled={link.isPending}
            >
              <SelectTrigger>
                <SelectValue
                  placeholder={t("Choisir l'installation correspondante")}
                />
              </SelectTrigger>
              <SelectContent>
                {candidates.map((i) => (
                  <SelectItem key={i.id} value={String(i.id)}>
                    {i.name} ·{" "}
                    {i.node_id !== null
                      ? t("via {name}", { name: i.host })
                      : `${i.host}:${i.port}`}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        )}

        {result && mode === "local" && (
          <Alert variant={result.attached ? "default" : "destructive"}>
            {result.attached ? <CheckCircle2 /> : <TriangleAlert />}
            <AlertTitle>
              {result.attached
                ? t("Installation jointe")
                : t("Installation non jointe")}
            </AlertTitle>
            <AlertDescription>{result.message}</AlertDescription>
          </Alert>
        )}
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            {t("Fermer")}
          </Button>
          {mode === "local" && (
            <Button onClick={() => save.mutate()} disabled={save.isPending}>
              {save.isPending && <Loader2 className="animate-spin" />}{" "}
              {t("Enregistrer et tester")}
            </Button>
          )}
        </DialogFooter>
        {addingNode && <AddNodeDialog onClose={() => setAddingNode(false)} />}
      </DialogContent>
    </Dialog>
  )
}

/** The installations of the Airzone account, with the way this server reaches them. */
export function AirzoneInstallationsCard({
  openFor,
}: {
  openFor?: string | null
}) {
  const cloud = useQuery({ queryKey: ["cloud"], queryFn: api.cloud })
  const status = useQuery({ queryKey: ["status"], queryFn: api.status })
  // Opened directly from a link (?access=<cloud_id>), e.g. from the dashboard.
  const [editing, setEditing] = useState<string | null>(openFor ?? null)
  const list = cloud.data?.installations ?? []
  if (!list.length) return null
  const defaultSubnet = cloud.data?.default_subnet ?? null
  const current = list.find((c) => c.cloud_id === editing)
  // Name of the Breezone server an installation is reached through, if any.
  const viaNode = (c: CloudInstallation) =>
    status.data?.find((s) => s.id === c.installation_id)?.node?.name

  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{t("Vos installations Airzone")}</CardTitle>
        <CardDescription>
          {t(
            "Importées de votre compte Airzone, avec la façon dont ce serveur joint leur WebServer."
          )}
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-2">
        {list.map((c) => {
          const via = viaNode(c)
          return (
            <div
              key={c.cloud_id}
              className="flex flex-wrap items-center gap-3 rounded-2xl border p-3"
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="truncate font-medium">{c.name}</span>
                  {c.reachable && via ? (
                    <Badge>{t("Via {name}", { name: via })}</Badge>
                  ) : c.reachable ? (
                    <Badge>{t("Pilotée ici")}</Badge>
                  ) : (
                    <Badge variant="outline">{t("Non jointe")}</Badge>
                  )}
                </div>
                <div className="flex flex-wrap gap-x-3 text-xs text-muted-foreground">
                  {c.location && (
                    <span className="inline-flex items-center gap-1">
                      <MapPin className="size-3" /> {c.location}
                    </span>
                  )}
                  <span>{tn(c.zones, "{n} zone", "{n} zones")}</span>
                  <span>
                    {via
                      ? t("Jointe via le serveur {name}", { name: via })
                      : accessSummary(c, defaultSubnet)}
                  </span>
                </div>
              </div>
              <Button
                size="sm"
                variant={c.reachable ? "ghost" : "outline"}
                onClick={() => setEditing(c.cloud_id)}
              >
                <Network /> {t("Accès")}
              </Button>
            </div>
          )
        })}
      </CardContent>
      {current && (
        <AccessDialog
          key={current.cloud_id}
          inst={current}
          defaultSubnet={defaultSubnet}
          onClose={() => setEditing(null)}
        />
      )}
    </Card>
  )
}
