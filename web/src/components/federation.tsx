import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { QRCodeSVG } from "qrcode.react"
import {
  Copy,
  Link2,
  Loader2,
  Plus,
  RefreshCw,
  Server,
  Share2,
  Trash2,
} from "lucide-react"
import { toast } from "sonner"
import type { BreezoneNode, NodeState, Share, ShareInput } from "@/lib/api"
import { api } from "@/lib/api"
import { dateTime, relativeTime } from "@/lib/format"
import { cn } from "@/lib/utils"
import { Confirm } from "@/components/confirm"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Checkbox } from "@/components/ui/checkbox"
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

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e))

const NODE_STATE: Record<NodeState, { label: string; tone: string }> = {
  online: { label: "Connecté", tone: "bg-success" },
  offline: { label: "Injoignable", tone: "bg-destructive" },
  revoked: { label: "Clé révoquée", tone: "bg-destructive" },
  expired: { label: "Partage expiré", tone: "bg-warning" },
  error: { label: "Erreur", tone: "bg-destructive" },
  unknown: { label: "En attente", tone: "bg-muted-foreground" },
}

/** Connection state of another Breezone server. */
export function NodeStateBadge({
  state,
  latency,
  className,
}: {
  state: NodeState
  latency?: number | null
  className?: string
}) {
  const s = NODE_STATE[state]
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-xs font-medium",
        className
      )}
    >
      <span className={cn("size-2 rounded-full", s.tone)} />
      {s.label}
      {state === "online" && latency != null && (
        <span className="tabular text-muted-foreground">{latency} ms</span>
      )}
    </span>
  )
}

const permissionLabel = (p: string) =>
  p === "control" ? "Lecture et pilotage" : "Lecture seule"

// ---------------------------------------------------------------------------------------
// This server
// ---------------------------------------------------------------------------------------

function IdentityCard() {
  const qc = useQueryClient()
  const identity = useQuery({ queryKey: ["identity"], queryFn: api.identity })
  const [name, setName] = useState<string | null>(null)
  const [url, setUrl] = useState<string | null>(null)
  const nameValue = name ?? identity.data?.name ?? ""
  const urlValue = url ?? identity.data?.public_url ?? ""
  const save = useMutation({
    mutationFn: () => api.saveIdentity(nameValue, urlValue.trim() || null),
    onSuccess: (d) => {
      qc.setQueryData(["identity"], d)
      setName(null)
      setUrl(null)
      toast.success("Serveur enregistré")
    },
    onError: (e) => toast.error(errorText(e)),
  })
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>Ce serveur</CardTitle>
        <CardDescription>
          Comment les autres serveurs Breezone le voient et le joignent.
          L'adresse publique est celle de votre tunnel Cloudflare (https://…),
          ou son adresse locale si les deux serveurs sont sur le même réseau.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4 sm:grid-cols-2">
        <div className="grid gap-2">
          <Label htmlFor="node-name">Nom</Label>
          <Input
            id="node-name"
            value={nameValue}
            placeholder="Bureau"
            onChange={(e) => setName(e.target.value)}
          />
        </div>
        <div className="grid gap-2">
          <Label htmlFor="node-url">Adresse publique</Label>
          <Input
            id="node-url"
            value={urlValue}
            placeholder="https://breezone.mondomaine.fr"
            onChange={(e) => setUrl(e.target.value)}
          />
        </div>
      </CardContent>
      <CardFooter>
        <Button
          variant="outline"
          onClick={() => save.mutate()}
          disabled={save.isPending || (name === null && url === null)}
        >
          Enregistrer
        </Button>
      </CardFooter>
    </Card>
  )
}

// ---------------------------------------------------------------------------------------
// Shares (installations this server exposes)
// ---------------------------------------------------------------------------------------

function ShareDialog({
  share,
  onClose,
}: {
  share?: Share
  onClose: () => void
}) {
  const qc = useQueryClient()
  const installations = useQuery({
    queryKey: ["installations"],
    queryFn: api.installations,
  })
  const identity = useQuery({ queryKey: ["identity"], queryFn: api.identity })
  const local = (installations.data ?? []).filter((i) => i.node_id === null)
  const [label, setLabel] = useState(share?.label ?? "")
  const [selected, setSelected] = useState<Set<number>>(
    () => new Set(share?.installations.map((i) => i.id) ?? [])
  )
  const [permission, setPermission] = useState<ShareInput["permission"]>(
    share?.permission ?? "control"
  )
  const [expiry, setExpiry] = useState("never")
  const [url, setUrl] = useState("")
  const [token, setToken] = useState<string | null>(null)

  const input = (): ShareInput => ({
    label: label.trim(),
    installation_ids: [...selected],
    permission,
    expires_in_days: expiry === "never" ? null : Number(expiry),
    url: url.trim() || null,
  })
  const save = useMutation({
    mutationFn: async () => {
      if (share) {
        await api.updateShare(share.id, input())
        return null
      }
      return (await api.createShare(input())).token
    },
    onSuccess: (t) => {
      qc.invalidateQueries({ queryKey: ["shares"] })
      if (t) setToken(t)
      else {
        toast.success("Partage modifié")
        onClose()
      }
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const toggle = (id: number, on: boolean) =>
    setSelected((s) => {
      const next = new Set(s)
      if (on) next.add(id)
      else next.delete(id)
      return next
    })

  if (token) {
    return (
      <Dialog open onOpenChange={(o) => !o && onClose()}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>Jeton d'invitation · {label}</DialogTitle>
            <DialogDescription>
              Sur l'autre serveur Breezone : Réglages › Nœuds › Ajouter, puis
              collez ce jeton (ou scannez le QR code avec l'application). Il ne
              sera plus affiché : seule son empreinte est conservée ici.
            </DialogDescription>
          </DialogHeader>
          <div className="flex justify-center rounded-2xl bg-white p-4">
            <QRCodeSVG value={token} size={220} marginSize={1} />
          </div>
          <div className="flex gap-2">
            <Input readOnly value={token} className="font-mono text-xs" />
            <Button
              variant="outline"
              onClick={() =>
                navigator.clipboard
                  .writeText(token)
                  .then(() => toast.success("Jeton copié"))
              }
            >
              <Copy /> Copier
            </Button>
          </div>
          <DialogFooter>
            <Button onClick={onClose}>Terminé</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    )
  }

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {share ? "Modifier le partage" : "Nouveau partage"}
          </DialogTitle>
          <DialogDescription>
            Un autre serveur Breezone pourra afficher (et piloter, si vous le
            permettez) les installations choisies. Seules les installations
            jointes directement par ce serveur sont partageables.
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor="share-label">Pour qui</Label>
            <Input
              id="share-label"
              value={label}
              placeholder="Maison de Camille"
              onChange={(e) => setLabel(e.target.value)}
            />
          </div>
          <div className="grid gap-2">
            <Label>Installations partagées</Label>
            {local.length === 0 && (
              <p className="text-sm text-muted-foreground">
                Aucune installation locale.
              </p>
            )}
            {local.map((i) => (
              <label
                key={i.id}
                className="flex items-center gap-3 rounded-xl border p-2.5 text-sm"
              >
                <Checkbox
                  checked={selected.has(i.id)}
                  onCheckedChange={(v) => toggle(i.id, v === true)}
                />
                <span className="flex-1">{i.name}</span>
                <span className="text-xs text-muted-foreground">
                  {i.location ?? `${i.host}:${i.port}`}
                </span>
              </label>
            ))}
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="grid gap-2">
              <Label>Droit</Label>
              <Select
                value={permission}
                onValueChange={(v) =>
                  setPermission(v as ShareInput["permission"])
                }
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="control">Lecture et pilotage</SelectItem>
                  <SelectItem value="read">Lecture seule</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="grid gap-2">
              <Label>Validité</Label>
              <Select value={expiry} onValueChange={setExpiry}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="never">
                    {share?.expires_at
                      ? `Inchangée (${dateTime(share.expires_at)})`
                      : "Jusqu'à révocation"}
                  </SelectItem>
                  <SelectItem value="1">1 jour</SelectItem>
                  <SelectItem value="7">7 jours</SelectItem>
                  <SelectItem value="30">30 jours</SelectItem>
                  <SelectItem value="365">1 an</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
          {!share && (
            <div className="grid gap-2">
              <Label htmlFor="share-url">
                Adresse de ce serveur pour l'autre
              </Label>
              <Input
                id="share-url"
                value={url}
                placeholder={
                  identity.data?.public_url ??
                  "https://breezone.mondomaine.fr ou http://192.168.1.20:8080"
                }
                onChange={(e) => setUrl(e.target.value)}
              />
              <p className="text-xs text-muted-foreground">
                Vide = l'adresse publique de ce serveur. HTTP n'est accepté que
                vers une adresse locale.
              </p>
            </div>
          )}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Annuler
          </Button>
          <Button
            onClick={() => save.mutate()}
            disabled={save.isPending || !label.trim() || selected.size === 0}
          >
            {save.isPending && <Loader2 className="animate-spin" />}
            {share ? "Enregistrer" : "Créer le jeton"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function SharesCard() {
  const qc = useQueryClient()
  const shares = useQuery({
    queryKey: ["shares"],
    queryFn: api.shares,
    refetchInterval: 15_000,
  })
  const [dialog, setDialog] = useState<{ share?: Share; nonce: number } | null>(
    null
  )
  const revoke = useMutation({
    mutationFn: api.deleteShare,
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["shares"] })
      toast.success("Partage révoqué")
    },
    onError: (e) => toast.error(errorText(e)),
  })
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>À qui ce serveur est partagé</CardTitle>
        <CardDescription>
          Les autres serveurs Breezone qui voient des installations de celui-ci.
          Révoquer un partage coupe l'accès immédiatement.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-3">
        {(shares.data ?? []).map((s) => (
          <div
            key={s.id}
            className="flex flex-wrap items-center gap-3 rounded-2xl border p-3"
          >
            <span
              className={cn(
                "size-2.5 rounded-full",
                s.connected ? "bg-success" : "bg-muted-foreground"
              )}
            />
            <div className="min-w-0 flex-1">
              <div className="flex flex-wrap items-center gap-2">
                <span className="font-medium">{s.label}</span>
                {s.remote_name && (
                  <span className="text-sm text-muted-foreground">
                    · serveur « {s.remote_name} »
                  </span>
                )}
                <Badge variant="outline">{permissionLabel(s.permission)}</Badge>
                {s.expired && <Badge variant="destructive">Expiré</Badge>}
              </div>
              <div className="text-xs text-muted-foreground">
                {s.installations.map((i) => i.name).join(", ") ||
                  "Aucune installation"}
              </div>
              <div className="text-xs text-muted-foreground">
                {s.connected
                  ? "Connecté maintenant"
                  : s.last_seen
                    ? `Dernière connexion ${relativeTime(s.last_seen)}`
                    : "Jamais connecté"}
                {s.last_address && ` · depuis ${s.last_address}`}
                {s.expires_at &&
                  !s.expired &&
                  ` · expire le ${dateTime(s.expires_at)}`}
              </div>
            </div>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => setDialog({ share: s, nonce: Date.now() })}
            >
              Modifier
            </Button>
            <Confirm
              title={`Révoquer « ${s.label} » ?`}
              description="L'autre serveur perd l'accès à ces installations immédiatement."
              onConfirm={() => revoke.mutate(s.id)}
            >
              <Button size="sm" variant="ghost" className="text-destructive">
                Révoquer
              </Button>
            </Confirm>
          </div>
        ))}
        {shares.data?.length === 0 && (
          <p className="text-sm text-muted-foreground">
            Ce serveur n'est partagé avec aucun autre.
          </p>
        )}
      </CardContent>
      <CardFooter>
        <Button onClick={() => setDialog({ nonce: Date.now() })}>
          <Share2 /> Nouveau partage
        </Button>
      </CardFooter>
      {dialog && (
        <ShareDialog
          key={dialog.nonce}
          share={dialog.share}
          onClose={() => setDialog(null)}
        />
      )}
    </Card>
  )
}

export function SharesTab() {
  return (
    <div className="grid gap-6">
      <IdentityCard />
      <SharesCard />
    </div>
  )
}

// ---------------------------------------------------------------------------------------
// Nodes (other servers whose installations this server shows)
// ---------------------------------------------------------------------------------------

export function AddNodeDialog({ onClose }: { onClose: () => void }) {
  const qc = useQueryClient()
  const [token, setToken] = useState("")
  const [url, setUrl] = useState("")
  const add = useMutation({
    mutationFn: () => api.addNode(token.trim(), url.trim() || null),
    onSuccess: (n) => {
      qc.invalidateQueries({ queryKey: ["nodes"] })
      qc.invalidateQueries({ queryKey: ["installations"] })
      qc.invalidateQueries({ queryKey: ["status"] })
      qc.invalidateQueries({ queryKey: ["cloud"] })
      toast.success(
        `Serveur « ${n.name} » ajouté : ${n.installations.length} installation(s)`
      )
      onClose()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Ajouter un serveur Breezone</DialogTitle>
          <DialogDescription>
            Sur le serveur de l'autre site : Réglages › Partages › Nouveau
            partage. Collez ici le jeton obtenu. Ses installations apparaîtront
            ici, jointes par son intermédiaire.
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor="node-token">Jeton d'invitation</Label>
            <textarea
              id="node-token"
              className="min-h-24 rounded-xl border bg-transparent px-3 py-2 font-mono text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
              value={token}
              placeholder="bz1.…"
              onChange={(e) => setToken(e.target.value)}
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="node-url-override">Adresse (facultatif)</Label>
            <Input
              id="node-url-override"
              value={url}
              placeholder="Celle du jeton, sinon par ex. http://192.168.1.20:8080"
              onChange={(e) => setUrl(e.target.value)}
            />
          </div>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Annuler
          </Button>
          <Button
            onClick={() => add.mutate()}
            disabled={add.isPending || !token.trim().startsWith("bz1.")}
          >
            {add.isPending && <Loader2 className="animate-spin" />} Ajouter
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function NodeRow({ node }: { node: BreezoneNode }) {
  const qc = useQueryClient()
  const [url, setUrl] = useState<string | null>(null)
  const invalidate = () => {
    qc.invalidateQueries({ queryKey: ["nodes"] })
    qc.invalidateQueries({ queryKey: ["installations"] })
    qc.invalidateQueries({ queryKey: ["status"] })
  }
  const refresh = useMutation({
    mutationFn: () => api.refreshNode(node.id),
    onSuccess: invalidate,
    onError: (e) => toast.error(errorText(e)),
  })
  const saveUrl = useMutation({
    mutationFn: () => api.updateNode(node.id, url ?? node.url),
    onSuccess: () => {
      setUrl(null)
      invalidate()
      toast.success("Adresse enregistrée")
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const remove = useMutation({
    mutationFn: () => api.deleteNode(node.id),
    onSuccess: invalidate,
    onError: (e) => toast.error(errorText(e)),
  })
  const s = node.status
  return (
    <div className="grid gap-3 rounded-2xl border p-3">
      <div className="flex flex-wrap items-center gap-3">
        <Server className="size-4 text-muted-foreground" />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-medium">{node.name}</span>
            <NodeStateBadge state={s.state} latency={s.latency_ms} />
            <Badge variant="outline">{permissionLabel(s.permission)}</Badge>
          </div>
          <div className="text-xs text-muted-foreground">
            {node.installations.map((i) => i.name).join(", ") ||
              "Aucune installation partagée"}
          </div>
          <div className="text-xs text-muted-foreground">
            {s.state === "online"
              ? "Connecté"
              : s.last_seen
                ? `Dernière réponse ${relativeTime(s.last_seen)}`
                : "Jamais joint"}
            {s.last_error && s.state !== "online" && ` · ${s.last_error}`}
          </div>
        </div>
        <Button
          size="icon"
          variant="ghost"
          aria-label="Actualiser"
          onClick={() => refresh.mutate()}
          disabled={refresh.isPending}
        >
          <RefreshCw className={cn(refresh.isPending && "animate-spin")} />
        </Button>
        <Confirm
          title={`Retirer « ${node.name} » ?`}
          description="Ses installations disparaîtront de ce serveur (historique local compris). Pensez à révoquer le partage sur l'autre serveur."
          onConfirm={() => remove.mutate()}
        >
          <Button size="icon" variant="ghost" aria-label="Retirer">
            <Trash2 />
          </Button>
        </Confirm>
      </div>
      <div className="flex flex-wrap gap-2">
        <Input
          className="max-w-md flex-1"
          value={url ?? node.url}
          onChange={(e) => setUrl(e.target.value)}
        />
        {url !== null && url !== node.url && (
          <Button
            variant="outline"
            onClick={() => saveUrl.mutate()}
            disabled={saveUrl.isPending}
          >
            <Link2 /> Enregistrer l'adresse
          </Button>
        )}
      </div>
    </div>
  )
}

export function NodesTab() {
  const nodes = useQuery({
    queryKey: ["nodes"],
    queryFn: api.nodes,
    refetchInterval: 10_000,
  })
  const [adding, setAdding] = useState(false)
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>Autres serveurs Breezone</CardTitle>
        <CardDescription>
          Les serveurs d'autres sites qui partagent leurs installations avec
          celui-ci : vous les pilotez d'ici, sans VPN. Les serveurs se parlent
          entre eux ; la clé n'arrive jamais sur les téléphones.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-3">
        {(nodes.data ?? []).map((n) => (
          <NodeRow key={n.id} node={n} />
        ))}
        {nodes.data?.length === 0 && (
          <p className="text-sm text-muted-foreground">
            Aucun autre serveur pour l'instant.
          </p>
        )}
      </CardContent>
      <CardFooter>
        <Button onClick={() => setAdding(true)}>
          <Plus /> Ajouter un serveur
        </Button>
      </CardFooter>
      {adding && <AddNodeDialog onClose={() => setAdding(false)} />}
    </Card>
  )
}
