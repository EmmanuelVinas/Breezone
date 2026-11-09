import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  ArrowRight,
  CheckCircle2,
  Cloud,
  CloudOff,
  Link2,
  Loader2,
  MapPin,
  Network,
  Plus,
  Server,
  Trash2,
  TriangleAlert,
  Unlink,
} from "lucide-react"
import { toast } from "sonner"
import type {
  AttachResult,
  CloudInstallation,
  Installation,
  InstallationConfig,
} from "@/lib/api"
import { api } from "@/lib/api"
import { cn } from "@/lib/utils"
import { ConnectionDot } from "@/components/connection-badge"
import { Confirm } from "@/components/confirm"
import { AddNodeDialog, NodeStateBadge } from "@/components/federation"
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
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

type Way = "existing" | "local" | "node"

function useRefresh() {
  const qc = useQueryClient()
  return () => {
    for (const key of ["cloud", "status", "installations", "nodes"])
      qc.invalidateQueries({ queryKey: [key] })
  }
}

// ---------------------------------------------------------------------------------------
// Boxes
// ---------------------------------------------------------------------------------------

function AirzoneBox({ c }: { c: CloudInstallation | null }) {
  if (!c)
    return (
      <div className="rounded-2xl border border-dashed p-3 text-sm text-muted-foreground">
        {t("Absente du compte Airzone")}
      </div>
    )
  return (
    <div className="min-w-0 rounded-2xl border bg-card/60 p-3">
      <div className="flex items-center gap-2">
        <Cloud className="size-4 shrink-0 text-muted-foreground" />
        <span className="truncate font-medium">{c.name}</span>
      </div>
      <div className="mt-0.5 flex flex-wrap gap-x-3 text-xs text-muted-foreground">
        {c.location && (
          <span className="inline-flex items-center gap-1">
            <MapPin className="size-3" /> {c.location}
          </span>
        )}
        <span>{tn(c.zones, "{n} zone", "{n} zones")}</span>
        {c.ws_ids[0] && <span>MAC {c.ws_ids[0]}</span>}
      </div>
    </div>
  )
}

/** How this server reaches the installation. */
function AccessBox({
  live,
  config,
}: {
  live: Installation | undefined
  config: InstallationConfig | undefined
}) {
  if (!live && !config)
    return (
      <div className="flex items-center gap-2 rounded-2xl border border-dashed p-3 text-sm text-muted-foreground">
        <CloudOff className="size-4" /> {t("Non jointe")}
      </div>
    )
  const viaNode = live?.node
  return (
    <div
      className={cn(
        "min-w-0 rounded-2xl border p-3",
        viaNode ? "border-fan/40 bg-fan/5" : "border-primary/40 bg-primary/5"
      )}
    >
      <div className="flex items-center gap-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
        {viaNode ? (
          <>
            <Server className="size-3.5" />{" "}
            {t("Via le serveur « {name} »", { name: viaNode.name })}
          </>
        ) : (
          <>
            <Network className="size-3.5" /> {t("Réseau de ce serveur")}
          </>
        )}
      </div>
      <div className="mt-1 flex items-center gap-2">
        <ConnectionDot state={live?.connection.state ?? "unknown"} />
        <span className="truncate font-medium">
          {live?.name ?? config?.name}
        </span>
        {viaNode && <NodeStateBadge state={viaNode.state} />}
      </div>
      {!viaNode && (
        <div className="truncate text-xs text-muted-foreground">
          WebServer {config?.host}:{config?.port}
          {config?.mac ? ` · ${config.mac}` : ""}
        </div>
      )}
    </div>
  )
}

function Connector({ joined }: { joined: boolean }) {
  return (
    <div className="flex items-center justify-center md:w-16">
      <div
        className={cn(
          "hidden h-0 flex-1 border-t-2 md:block",
          joined ? "border-primary" : "border-dashed border-muted-foreground/40"
        )}
      />
      <ArrowRight
        className={cn(
          "size-4 shrink-0 rotate-90 md:rotate-0",
          joined ? "text-primary" : "text-muted-foreground/60"
        )}
      />
    </div>
  )
}

// ---------------------------------------------------------------------------------------
// Link dialog: the three ways to reach an Airzone installation, in one place
// ---------------------------------------------------------------------------------------

function LinkDialog({
  inst,
  subnet,
  onClose,
}: {
  inst: CloudInstallation
  subnet: string | null
  onClose: () => void
}) {
  const refresh = useRefresh()
  const qc = useQueryClient()
  const installations = useQuery({
    queryKey: ["installations"],
    queryFn: api.installations,
  })
  const nodes = useQuery({ queryKey: ["nodes"], queryFn: api.nodes })
  const linked = installations.data?.find((i) => i.id === inst.installation_id)
  const candidates = (installations.data ?? []).filter(
    (i) => i.cloud_id === null || i.cloud_id === inst.cloud_id
  )
  const [way, setWay] = useState<Way>(
    candidates.some((c) => c.id !== linked?.id) ? "existing" : "local"
  )
  const [existing, setExisting] = useState<string>(
    linked ? String(linked.id) : ""
  )
  const [host, setHost] = useState(inst.access?.host ?? "")
  const [port, setPort] = useState(String(inst.access?.port ?? 3000))
  const [result, setResult] = useState<AttachResult | null>(null)
  const [addingNode, setAddingNode] = useState(false)

  const link = useMutation({
    mutationFn: (id: number) => api.linkCloud(id, inst.cloud_id),
    onSuccess: () => {
      refresh()
      toast.success(t("« {name} » est reliée", { name: inst.name }))
      onClose()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const unlink = useMutation({
    mutationFn: (id: number) => api.linkCloud(id, null),
    onSuccess: () => {
      refresh()
      toast.success(t("Lien retiré"))
      onClose()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const attach = useMutation({
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

  const ways: { id: Way; icon: typeof Network; title: string; text: string }[] =
    [
      {
        id: "existing",
        icon: Link2,
        title: t("Déjà pilotée ici"),
        text: t("C'est une installation déjà présente sur ce serveur"),
      },
      {
        id: "local",
        icon: Network,
        title: t("Réseau de ce serveur"),
        text: t("Son WebServer est sur le même réseau"),
      },
      {
        id: "node",
        icon: Server,
        title: t("Autre serveur Breezone"),
        text: t("Elle est sur un autre site, avec son propre serveur"),
      },
    ]

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>
            {t("Relier « {name} »", { name: inst.name })}
          </DialogTitle>
          <DialogDescription>
            {inst.ws_ids.length > 0
              ? t(
                  "Comment ce serveur joint-il cette installation de votre compte Airzone (WebServer {mac}) ?",
                  {
                    mac: inst.ws_ids[0],
                  }
                )
              : t(
                  "Comment ce serveur joint-il cette installation de votre compte Airzone ?"
                )}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-2 sm:grid-cols-3">
          {ways.map((w) => (
            <button
              key={w.id}
              type="button"
              onClick={() => setWay(w.id)}
              className={cn(
                "flex flex-col gap-1 rounded-2xl border p-3 text-left transition-colors",
                way === w.id
                  ? "border-primary bg-primary/10"
                  : "hover:bg-muted/50"
              )}
            >
              <w.icon className="size-4 text-primary" />
              <span className="text-sm font-medium">{w.title}</span>
              <span className="text-xs text-muted-foreground">{w.text}</span>
            </button>
          ))}
        </div>

        {way === "existing" &&
          (candidates.length ? (
            <div className="grid gap-2">
              <Label>{t("Installation de ce serveur")}</Label>
              <Select value={existing} onValueChange={setExisting}>
                <SelectTrigger>
                  <SelectValue placeholder={t("Choisir")} />
                </SelectTrigger>
                <SelectContent>
                  {candidates.map((i) => (
                    <SelectItem key={i.id} value={String(i.id)}>
                      {i.name} ·{" "}
                      {i.node_id !== null
                        ? t("via un autre serveur")
                        : `${i.host}:${i.port}`}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">
                {t(
                  "Refusé si son WebServer (adresse MAC) n'est pas celui de cette installation."
                )}
              </p>
            </div>
          ) : (
            <p className="text-sm text-muted-foreground">
              {t(
                "Aucune installation libre sur ce serveur : choisissez une autre façon."
              )}
            </p>
          ))}

        {way === "local" && (
          <div className="grid grid-cols-[1fr_6rem] gap-3">
            <div className="grid gap-2">
              <Label htmlFor="lk-host">{t("Adresse du WebServer")}</Label>
              <Input
                id="lk-host"
                value={host}
                placeholder={
                  subnet
                    ? t("vide = recherche sur {subnet}", { subnet })
                    : "192.168.1.25"
                }
                onChange={(e) => setHost(e.target.value)}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="lk-port">{t("Port")}</Label>
              <Input
                id="lk-port"
                type="number"
                value={port}
                onChange={(e) => setPort(e.target.value)}
              />
            </div>
            {!subnet && !host.trim() && (
              <p className="col-span-2 text-xs text-warning">
                {t(
                  "Le réseau du serveur n'est pas défini : indiquez l'adresse du WebServer, ou définissez le réseau en haut de la page."
                )}
              </p>
            )}
            {result && (
              <Alert
                className="col-span-2"
                variant={result.attached ? "default" : "destructive"}
              >
                {result.attached ? <CheckCircle2 /> : <TriangleAlert />}
                <AlertTitle>
                  {result.attached ? t("Installation jointe") : t("Non jointe")}
                </AlertTitle>
                <AlertDescription>{result.message}</AlertDescription>
              </Alert>
            )}
          </div>
        )}

        {way === "node" && (
          <div className="grid gap-3">
            <p className="text-sm text-muted-foreground">
              {t(
                "Sur le serveur Breezone de ce site : Réglages › Partages › Nouveau partage, puis ajoutez son jeton ici. Ses installations sont reliées automatiquement (même WebServer)."
              )}
            </p>
            {(nodes.data ?? []).map((n) => (
              <div
                key={n.id}
                className="flex items-center gap-2 rounded-xl border p-2.5 text-sm"
              >
                <Server className="size-4 text-muted-foreground" />
                <span className="flex-1">
                  {n.name}
                  <span className="text-muted-foreground">
                    {" "}
                    · {n.installations.map((i) => i.name).join(", ") || "—"}
                  </span>
                </span>
                <NodeStateBadge state={n.status.state} />
              </div>
            ))}
            {(nodes.data ?? [])
              .filter((n) => n.status.state === "pending")
              .map((n) => (
                <Alert key={`pending-${n.id}`}>
                  <TriangleAlert />
                  <AlertTitle>{t("En attente d'acceptation")}</AlertTitle>
                  <AlertDescription>
                    {t(
                      "« {name} » doit encore accepter votre demande : sur ce serveur-là, Réglages › Partages › Accepter. Ses installations apparaîtront ensuite ici.",
                      {
                        name: n.name,
                      }
                    )}
                  </AlertDescription>
                </Alert>
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

        <DialogFooter className="sm:justify-between">
          {linked ? (
            <Button
              variant="ghost"
              className="text-destructive"
              onClick={() => unlink.mutate(linked.id)}
              disabled={unlink.isPending}
            >
              <Unlink /> {t("Délier")}
            </Button>
          ) : (
            <span />
          )}
          <div className="flex gap-2">
            <Button variant="ghost" onClick={onClose}>
              {t("Fermer")}
            </Button>
            {way === "existing" && (
              <Button
                onClick={() => link.mutate(Number(existing))}
                disabled={!existing || link.isPending}
              >
                {link.isPending && <Loader2 className="animate-spin" />}{" "}
                {t("Relier")}
              </Button>
            )}
            {way === "local" && (
              <Button
                onClick={() => attach.mutate()}
                disabled={attach.isPending}
              >
                {attach.isPending && <Loader2 className="animate-spin" />}{" "}
                {t("Chercher et relier")}
              </Button>
            )}
          </div>
        </DialogFooter>
        {addingNode && <AddNodeDialog onClose={() => setAddingNode(false)} />}
      </DialogContent>
    </Dialog>
  )
}

/** For an installation of this server that is in no Airzone installation. */
function LinkToCloudDialog({
  config,
  cloud,
  onClose,
}: {
  config: InstallationConfig
  cloud: CloudInstallation[]
  onClose: () => void
}) {
  const refresh = useRefresh()
  const free = cloud.filter((c) => c.installation_id === null)
  const [cloudId, setCloudId] = useState("")
  const link = useMutation({
    mutationFn: () => api.linkCloud(config.id, cloudId),
    onSuccess: () => {
      refresh()
      toast.success(t("« {name} » est reliée", { name: config.name }))
      onClose()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>
            {t("Relier « {name} » à Airzone", { name: config.name })}
          </DialogTitle>
          <DialogDescription>
            {t(
              "À quelle installation de votre compte Airzone correspond-elle ?"
            )}
          </DialogDescription>
        </DialogHeader>
        {free.length ? (
          <Select value={cloudId} onValueChange={setCloudId}>
            <SelectTrigger>
              <SelectValue placeholder={t("Choisir")} />
            </SelectTrigger>
            <SelectContent>
              {free.map((c) => (
                <SelectItem key={c.cloud_id} value={c.cloud_id}>
                  {c.name}
                  {c.location ? ` · ${c.location}` : ""}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : (
          <p className="text-sm text-muted-foreground">
            {t("Toutes les installations du compte Airzone sont déjà reliées.")}
          </p>
        )}
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            {t("Fermer")}
          </Button>
          <Button
            onClick={() => link.mutate()}
            disabled={!cloudId || link.isPending}
          >
            {t("Relier")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ---------------------------------------------------------------------------------------
// The map
// ---------------------------------------------------------------------------------------

/**
 * The installations of the Airzone account on the left, how this server reaches each one on
 * the right (its own network, or another Breezone server), and the installations of this
 * server that are in no Airzone installation.
 */
export function InstallationMap({
  openFor,
  onAdd,
}: {
  openFor?: string | null
  /** Adds an installation that is not in the Airzone account (a WebServer of this network). */
  onAdd: () => void
}) {
  const refresh = useRefresh()
  const remove = useMutation({
    mutationFn: api.deleteInstallation,
    onSuccess: () => {
      refresh()
      toast.success(t("Installation supprimée"))
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const cloud = useQuery({ queryKey: ["cloud"], queryFn: api.cloud })
  const status = useQuery({ queryKey: ["status"], queryFn: api.status })
  const configs = useQuery({
    queryKey: ["installations"],
    queryFn: api.installations,
  })
  const discovery = useQuery({
    queryKey: ["discovery"],
    queryFn: api.discoverySettings,
  })
  const [editing, setEditing] = useState<string | null>(openFor ?? null)
  const [linkingLocal, setLinkingLocal] = useState<number | null>(null)
  const [addingNode, setAddingNode] = useState(false)

  const list = cloud.data?.installations ?? []
  const all = configs.data ?? []
  const orphans = all.filter(
    (i) => !list.some((c) => c.installation_id === i.id)
  )
  const live = (id: number | null) =>
    status.data?.find((s) => s.id === id) ?? undefined
  const config = (id: number | null) =>
    all.find((i) => i.id === id) ?? undefined
  const current = list.find((c) => c.cloud_id === editing)
  const orphanEditing = all.find((i) => i.id === linkingLocal)
  const subnet = discovery.data?.subnet ?? discovery.data?.effective ?? null

  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{t("Vos installations")}</CardTitle>
        <CardDescription>
          {t(
            "À gauche, votre compte Airzone ; à droite, comment ce serveur joint chaque installation : sur son propre réseau, ou par le serveur Breezone d'un autre site."
          )}
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-3">
        <div className="hidden grid-cols-[1fr_4rem_1fr_7rem] gap-3 px-1 text-xs font-medium tracking-wide text-muted-foreground uppercase md:grid">
          <span className="inline-flex items-center gap-1.5">
            <Cloud className="size-3.5" /> {t("Compte Airzone")}
          </span>
          <span />
          <span className="inline-flex items-center gap-1.5">
            <Network className="size-3.5" /> {t("Joint par ce serveur")}
          </span>
        </div>
        {list.map((c) => {
          const joined = c.installation_id !== null
          return (
            <div
              key={c.cloud_id}
              className="grid items-center gap-2 md:grid-cols-[1fr_4rem_1fr_7rem] md:gap-3"
            >
              <AirzoneBox c={c} />
              <Connector joined={joined} />
              <AccessBox
                live={live(c.installation_id)}
                config={config(c.installation_id)}
              />
              <Button
                size="sm"
                variant={joined ? "ghost" : "default"}
                onClick={() => setEditing(c.cloud_id)}
              >
                <Link2 /> {joined ? t("Modifier") : t("Relier")}
              </Button>
            </div>
          )
        })}
        {orphans.map((i) => (
          <div
            key={i.id}
            className="grid items-center gap-2 md:grid-cols-[1fr_4rem_1fr_7rem] md:gap-3"
          >
            <AirzoneBox c={null} />
            <Connector joined={false} />
            <AccessBox live={live(i.id)} config={i} />
            <div className="flex items-center justify-end gap-1">
              {list.some((c) => c.installation_id === null) && (
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => setLinkingLocal(i.id)}
                >
                  <Link2 /> {t("Relier")}
                </Button>
              )}
              {i.node_id === null && (
                <Confirm
                  title={t("Supprimer « {name} » ?", { name: i.name })}
                  description={t(
                    "Son historique et ses réglages Breezone seront supprimés. Le WebServer n'est pas modifié."
                  )}
                  onConfirm={() => remove.mutate(i.id)}
                >
                  <Button
                    size="icon"
                    variant="ghost"
                    aria-label={t("Supprimer")}
                  >
                    <Trash2 />
                  </Button>
                </Confirm>
              )}
            </div>
          </div>
        ))}
        {!list.length && !all.length && (
          <p className="text-sm text-muted-foreground">
            {t(
              "Aucune installation : connectez votre compte Airzone (onglet « Compte Airzone »), ou ajoutez-en une."
            )}
          </p>
        )}
      </CardContent>
      <CardFooter>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="outline">
              <Plus /> {t("Ajouter une installation hors Airzone")}
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="w-80">
            <DropdownMenuItem onSelect={onAdd} className="items-start">
              <Network className="mt-0.5" />
              <span>
                <span className="font-medium">
                  {t("WebServer sur ce réseau")}
                </span>
                <span className="block text-xs text-muted-foreground">
                  {t("Ce serveur le pilote directement (adresse IP).")}
                </span>
              </span>
            </DropdownMenuItem>
            <DropdownMenuItem
              onSelect={() => setAddingNode(true)}
              className="items-start"
            >
              <Server className="mt-0.5" />
              <span>
                <span className="font-medium">
                  {t("Via un autre serveur Breezone")}
                </span>
                <span className="block text-xs text-muted-foreground">
                  {t(
                    "Une installation d'un autre site, pilotée par le serveur Breezone de ce site (jeton d'invitation)."
                  )}
                </span>
              </span>
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        {addingNode && <AddNodeDialog onClose={() => setAddingNode(false)} />}
      </CardFooter>
      {current && (
        <LinkDialog
          key={current.cloud_id}
          inst={current}
          subnet={subnet}
          onClose={() => setEditing(null)}
        />
      )}
      {orphanEditing && (
        <LinkToCloudDialog
          config={orphanEditing}
          cloud={list}
          onClose={() => setLinkingLocal(null)}
        />
      )}
    </Card>
  )
}
