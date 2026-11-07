import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { QRCodeSVG } from "qrcode.react"
import {
  ArrowLeftRight,
  ShieldAlert,
  ShieldCheck,
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
import type {
  BreezoneNode,
  CertificateInfo,
  NodeState,
  Share,
  ShareInput,
} from "@/lib/api"
import { api, ApiError } from "@/lib/api"
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
  pending: { label: "En attente d'acceptation", tone: "bg-warning" },
  certificate: { label: "Certificat refusé", tone: "bg-destructive" },
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

/** `ab12…` → `AB:12:…`, as browsers show certificate fingerprints. */
function fingerprintText(fp: string) {
  return fp.toUpperCase().match(/.{2}/g)?.join(":") ?? fp
}

/** Review of an HTTPS certificate that fails the usual checks, before trusting it. */
function CertificateReview({
  certificate,
  reason,
  host,
  pending,
  onTrust,
  onCancel,
}: {
  certificate: CertificateInfo
  reason?: string
  host: string
  pending: boolean
  onTrust: () => void
  onCancel?: () => void
}) {
  return (
    <div className="grid gap-3 rounded-2xl border border-destructive/50 bg-destructive/5 p-3 text-sm">
      <div className="flex items-start gap-2">
        <ShieldAlert className="mt-0.5 size-4 shrink-0 text-destructive" />
        <div>
          <p className="font-medium">Certificat HTTPS non valide</p>
          {reason && <p className="text-muted-foreground">{reason}</p>}
        </div>
      </div>
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
        <dt className="text-muted-foreground">Adresse</dt>
        <dd className="break-all">{host}</dd>
        <dt className="text-muted-foreground">Délivré à</dt>
        <dd className="break-all">{certificate.subject ?? "—"}</dd>
        <dt className="text-muted-foreground">Valable pour</dt>
        <dd className="break-all">
          {certificate.names.length ? certificate.names.join(", ") : "—"}
        </dd>
        <dt className="text-muted-foreground">Émis par</dt>
        <dd className="break-all">
          {certificate.self_signed
            ? "lui-même (auto-signé)"
            : (certificate.issuer ?? "—")}
        </dd>
        <dt className="text-muted-foreground">Valable</dt>
        <dd>
          {certificate.not_before
            ? `du ${dateTime(certificate.not_before)} `
            : ""}
          {certificate.not_after ? `au ${dateTime(certificate.not_after)}` : ""}
        </dd>
        <dt className="text-muted-foreground">Empreinte SHA-256</dt>
        <dd className="font-mono break-all">
          {fingerprintText(certificate.fingerprint)}
        </dd>
      </dl>
      <p className="text-xs text-muted-foreground">
        Faites confiance à ce certificat seulement si vous savez pourquoi il
        n'est pas valide (par exemple un certificat du NAS qui ne couvre pas ce
        nom). Pour en être sûr, comparez l'empreinte avec celle affichée par
        votre navigateur sur cette adresse. Breezone n'acceptera ensuite que ce
        certificat précis et vous redemandera votre accord s'il change.
      </p>
      <div className="flex flex-wrap justify-end gap-2">
        {onCancel && (
          <Button size="sm" variant="ghost" onClick={onCancel}>
            Annuler
          </Button>
        )}
        <Button
          size="sm"
          variant="destructive"
          onClick={onTrust}
          disabled={pending}
        >
          {pending && <Loader2 className="animate-spin" />}
          Faire confiance à ce certificat
        </Button>
      </div>
    </div>
  )
}

/** Certificate attached to an API error, if the other server's certificate was refused. */
function certificateOf(e: unknown): CertificateInfo | null {
  if (!(e instanceof ApiError)) return null
  const c = e.data.certificate
  return c && typeof c === "object" ? (c as CertificateInfo) : null
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
  const [reciprocal, setReciprocal] = useState(false)
  const [token, setToken] = useState<string | null>(null)

  const input = (): ShareInput => ({
    label: label.trim(),
    installation_ids: [...selected],
    permission,
    expires_in_days: expiry === "never" ? null : Number(expiry),
    url: url.trim() || null,
    reciprocal: share ? undefined : reciprocal,
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
              collez ce jeton (ou scannez le QR code avec l'application). Il est
              utilisable <strong>une seule fois</strong>, pendant{" "}
              <strong>15 minutes</strong>, et ne contient aucune clé d'accès.
              Vous devrez ensuite <strong>accepter la demande</strong> ici, dans
              « À qui ce serveur est partagé ».
              {reciprocal &&
                " L'autre serveur pourra aussi vous partager ses installations : elles apparaîtront dans Nœuds une fois la demande acceptée."}
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
                  "https://breezone.mondomaine.fr ou http://192.168.1.20:8427"
                }
                onChange={(e) => setUrl(e.target.value)}
              />
              <p className="text-xs text-muted-foreground">
                Vide = l'adresse publique de ce serveur. HTTP n'est accepté que
                vers une adresse locale.
              </p>
            </div>
          )}
          {!share && (
            <label className="flex items-start gap-3 rounded-xl border p-3 text-sm">
              <Checkbox
                className="mt-0.5"
                checked={reciprocal}
                onCheckedChange={(v) => setReciprocal(v === true)}
              />
              <span>
                <span className="font-medium">Partage réciproque</span>
                <span className="block text-muted-foreground">
                  Proposer à l'autre serveur de partager aussi ses installations
                  avec celui-ci. Il choisira lesquelles en ajoutant le jeton ;
                  elles apparaîtront ici quand vous accepterez sa demande.
                </span>
              </span>
            </label>
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
  const approve = useMutation({
    mutationFn: api.approveShare,
    onSuccess: (s) => {
      qc.invalidateQueries({ queryKey: ["shares"] })
      toast.success(`Accès accordé au serveur « ${s.remote_name ?? "?"} »`)
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
                {s.status === "invited" && (
                  <Badge variant="secondary">Invitation non utilisée</Badge>
                )}
                {(s.reciprocal || s.reverse) && (
                  <Badge variant="outline">
                    <ArrowLeftRight /> Réciproque
                  </Badge>
                )}
                {s.status === "pending" && (
                  <Badge className="bg-warning text-black">
                    Demande d'accès
                  </Badge>
                )}
              </div>
              <div className="text-xs text-muted-foreground">
                {s.installations.map((i) => i.name).join(", ") ||
                  "Aucune installation"}
              </div>
              {s.status === "pending" && (
                <p className="mt-1 text-sm">
                  Le serveur <strong>« {s.remote_name} »</strong> demande
                  l'accès
                  {s.last_address ? ` depuis ${s.last_address}` : ""}
                  {s.requested_at ? ` (${relativeTime(s.requested_at)})` : ""}.
                  Acceptez seulement si c'est bien vous qui venez d'utiliser
                  l'invitation.
                  {s.reciprocal &&
                    (s.remote_shares_back
                      ? ` Il vous partage aussi ses installations : elles apparaîtront dans Nœuds dès que vous accepterez.`
                      : ` Il n'a pas accepté le partage réciproque.`)}
                </p>
              )}
              {s.reciprocal && s.status === "active" && (
                <p className="mt-1 text-xs text-muted-foreground">
                  {s.remote_shares_back
                    ? `« ${s.remote_name ?? "?"} » vous partage aussi ses installations (Réglages › Nœuds).`
                    : `« ${s.remote_name ?? "?"} » ne partage pas ses installations en retour. Pour les voir ici : sur ce serveur-là, Réglages › Partages › Nouveau partage, puis ajoutez le jeton dans Réglages › Nœuds.`}
                </p>
              )}
              {s.reverse && (
                <p className="mt-1 text-xs text-muted-foreground">
                  En retour de l'invitation de « {s.remote_name} ».
                </p>
              )}
              <div className="text-xs text-muted-foreground">
                {s.status === "invited"
                  ? s.pairing_expires_at && !s.pairing_expired
                    ? `Invitation valable jusqu'à ${dateTime(s.pairing_expires_at)}`
                    : "Invitation expirée : supprimez-la et créez-en une nouvelle"
                  : s.status === "pending"
                    ? "En attente de votre réponse"
                    : s.connected
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
            {s.status === "pending" && (
              <Button
                size="sm"
                onClick={() => approve.mutate(s.id)}
                disabled={approve.isPending}
              >
                Accepter
              </Button>
            )}
            <Button
              size="sm"
              variant="ghost"
              onClick={() => setDialog({ share: s, nonce: Date.now() })}
            >
              Modifier
            </Button>
            <Confirm
              title={
                s.status === "pending"
                  ? `Refuser la demande de « ${s.remote_name} » ?`
                  : `Révoquer « ${s.label} » ?`
              }
              description="L'autre serveur perd l'accès à ces installations immédiatement."
              onConfirm={() => revoke.mutate(s.id)}
            >
              <Button size="sm" variant="ghost" className="text-destructive">
                {s.status === "pending" ? "Refuser" : "Révoquer"}
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

/** Reads the invitation (`bz2.` + base64url JSON) to adapt the dialog; the server checks it. */
function readInvitation(
  token: string
): { name: string; url: string; reciprocal: boolean } | null {
  const t = token.replace(/\s+/g, "")
  if (!t.startsWith("bz2.")) return null
  try {
    const b64 = t.slice(4).replace(/-/g, "+").replace(/_/g, "/")
    const json = new TextDecoder().decode(
      Uint8Array.from(atob(b64), (c) => c.charCodeAt(0))
    )
    const inv = JSON.parse(json) as {
      name?: unknown
      url?: unknown
      reciprocal?: unknown
    }
    return {
      name: String(inv.name ?? "?"),
      url: String(inv.url ?? ""),
      reciprocal: inv.reciprocal === true,
    }
  } catch {
    return null
  }
}

export function AddNodeDialog({ onClose }: { onClose: () => void }) {
  const qc = useQueryClient()
  const [token, setToken] = useState("")
  const [url, setUrl] = useState("")
  const [shareBack, setShareBack] = useState(true)
  const [selected, setSelected] = useState<Set<number> | null>(null)
  const [permission, setPermission] =
    useState<ShareInput["permission"]>("control")
  const invitation = readInvitation(token)
  const reciprocal = invitation?.reciprocal === true
  const installations = useQuery({
    queryKey: ["installations"],
    queryFn: api.installations,
    enabled: reciprocal,
  })
  const identity = useQuery({
    queryKey: ["identity"],
    queryFn: api.identity,
    enabled: reciprocal,
  })
  const local = (installations.data ?? []).filter((i) => i.node_id === null)
  // All local installations by default.
  const chosen = selected ?? new Set(local.map((i) => i.id))
  const sharing = reciprocal && shareBack
  // Certificate refused by the usual checks, shown for review.
  const [review, setReview] = useState<{
    certificate: CertificateInfo
    reason: string
  } | null>(null)
  const add = useMutation({
    mutationFn: (certificate: string | null) =>
      api.addNode(
        token.trim(),
        url.trim() || null,
        sharing ? { installation_ids: [...chosen], permission } : null,
        certificate
      ),
    onSuccess: (n) => {
      qc.invalidateQueries({ queryKey: ["nodes"] })
      qc.invalidateQueries({ queryKey: ["shares"] })
      qc.invalidateQueries({ queryKey: ["installations"] })
      qc.invalidateQueries({ queryKey: ["status"] })
      qc.invalidateQueries({ queryKey: ["cloud"] })
      toast.success(
        `Serveur « ${n.name} » ajouté : acceptez la demande sur ce serveur pour voir ses installations`
      )
      onClose()
    },
    onError: (e) => {
      const certificate = certificateOf(e)
      if (certificate) {
        const reason =
          e instanceof ApiError && typeof e.data.certificate_error === "string"
            ? e.data.certificate_error
            : errorText(e)
        setReview({ certificate, reason })
      } else toast.error(errorText(e))
    },
  })
  const toggle = (id: number, on: boolean) => {
    const next = new Set(chosen)
    if (on) next.add(id)
    else next.delete(id)
    setSelected(next)
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Ajouter un serveur Breezone</DialogTitle>
          <DialogDescription>
            Sur le serveur de l'autre site : Réglages › Partages › Nouveau
            partage. Collez ici le jeton obtenu (valable 15 minutes, une seule
            fois), puis acceptez la demande sur ce serveur-là. Ses installations
            apparaîtront alors ici. Les deux serveurs se reconnaissent ensuite
            par une signature : aucune clé ne circule.
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor="node-token">Jeton d'invitation</Label>
            <textarea
              id="node-token"
              className="min-h-24 rounded-xl border bg-transparent px-3 py-2 font-mono text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
              value={token}
              placeholder="bz2.…"
              onChange={(e) => {
                setToken(e.target.value)
                setReview(null)
              }}
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="node-url-override">Adresse (facultatif)</Label>
            <Input
              id="node-url-override"
              value={url}
              placeholder="Celle du jeton, sinon par ex. http://192.168.1.20:8427"
              onChange={(e) => {
                setUrl(e.target.value)
                setReview(null)
              }}
            />
          </div>
          {review && (
            <CertificateReview
              certificate={review.certificate}
              reason={review.reason}
              host={url.trim() || invitation?.url || ""}
              pending={add.isPending}
              onTrust={() => add.mutate(review.certificate.fingerprint)}
              onCancel={() => setReview(null)}
            />
          )}
          {reciprocal && (
            <div className="grid gap-3 rounded-2xl border p-3">
              <label className="flex items-start gap-3 text-sm">
                <Checkbox
                  className="mt-0.5"
                  checked={shareBack}
                  onCheckedChange={(v) => setShareBack(v === true)}
                />
                <span>
                  <span className="font-medium">
                    Partager aussi mes installations avec « {invitation?.name} »
                  </span>
                  <span className="block text-muted-foreground">
                    Ce serveur propose un partage réciproque. Il verra les
                    installations cochées dès qu'il aura accepté votre demande ;
                    vous pourrez révoquer ce partage dans Réglages › Partages.
                  </span>
                </span>
              </label>
              {shareBack && (
                <>
                  {local.length === 0 && (
                    <p className="text-sm text-muted-foreground">
                      Aucune installation locale à partager.
                    </p>
                  )}
                  {local.map((i) => (
                    <label
                      key={i.id}
                      className="flex items-center gap-3 rounded-xl border p-2.5 text-sm"
                    >
                      <Checkbox
                        checked={chosen.has(i.id)}
                        onCheckedChange={(v) => toggle(i.id, v === true)}
                      />
                      <span className="flex-1">{i.name}</span>
                      <span className="text-xs text-muted-foreground">
                        {i.location ?? `${i.host}:${i.port}`}
                      </span>
                    </label>
                  ))}
                  <div className="grid gap-2">
                    <Label>Droit</Label>
                    <Select
                      value={permission}
                      onValueChange={(v) =>
                        setPermission(v as ShareInput["permission"])
                      }
                    >
                      <SelectTrigger className="w-fit">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="control">
                          Lecture et pilotage
                        </SelectItem>
                        <SelectItem value="read">Lecture seule</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  {!identity.data?.public_url && (
                    <p className="text-sm text-destructive">
                      Renseignez d'abord l'adresse publique de ce serveur
                      (Réglages › Partages › Ce serveur) : l'autre serveur en a
                      besoin pour le joindre.
                    </p>
                  )}
                </>
              )}
            </div>
          )}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Annuler
          </Button>
          <Button
            onClick={() => add.mutate(null)}
            disabled={
              add.isPending ||
              review !== null ||
              !invitation ||
              (sharing && (chosen.size === 0 || !identity.data?.public_url))
            }
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
  // Certificate refused by the usual checks: review it, then trust it.
  const [review, setReview] = useState<CertificateInfo | null>(null)
  const inspect = useMutation({
    mutationFn: () => api.nodeCertificate(node.id),
    onSuccess: (r) => setReview(r.certificate),
    onError: (e) => toast.error(errorText(e)),
  })
  const trust = useMutation({
    mutationFn: (fingerprint: string) =>
      api.trustNodeCertificate(node.id, fingerprint),
    onSuccess: () => {
      setReview(null)
      invalidate()
      toast.success(`Certificat de « ${node.name} » accepté`)
    },
    onError: (e) => {
      // It changed meanwhile: show the new one.
      const certificate = certificateOf(e)
      if (certificate) setReview(certificate)
      toast.error(errorText(e))
    },
  })
  const untrust = useMutation({
    mutationFn: () => api.untrustNodeCertificate(node.id),
    onSuccess: () => {
      invalidate()
      toast.success("Vérification HTTPS habituelle rétablie")
    },
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
            {/* The permission is only known once the other server accepted. */}
            {s.state !== "pending" && (
              <Badge variant="outline">{permissionLabel(s.permission)}</Badge>
            )}
            {node.certificate_pinned && (
              <Badge
                variant="outline"
                title="Certificat HTTPS accepté manuellement"
              >
                <ShieldCheck /> Certificat accepté
              </Badge>
            )}
          </div>
          <div className="text-xs text-muted-foreground">
            {node.installations.map((i) => i.name).join(", ") ||
              (s.state === "pending"
                ? `Ses installations apparaîtront quand « ${node.name} » aura accepté la demande (Réglages › Partages sur ce serveur)`
                : "Aucune installation partagée")}
          </div>
          <div className="text-xs text-muted-foreground">
            {s.state === "online"
              ? "Connecté"
              : s.last_seen
                ? `Dernière réponse ${relativeTime(s.last_seen)}`
                : "Jamais joint"}
            {s.last_error &&
              s.state !== "online" &&
              s.state !== "pending" &&
              ` · ${s.last_error}`}
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
        {s.state === "certificate" && !review && (
          <Button
            variant="outline"
            onClick={() => inspect.mutate()}
            disabled={inspect.isPending}
          >
            {inspect.isPending ? (
              <Loader2 className="animate-spin" />
            ) : (
              <ShieldAlert />
            )}
            Examiner le certificat
          </Button>
        )}
        {node.certificate_pinned && s.state !== "certificate" && (
          <Confirm
            title="Rétablir la vérification HTTPS habituelle ?"
            description="À faire quand l'autre serveur a un certificat valide. S'il n'en a pas, la connexion sera de nouveau refusée."
            onConfirm={() => untrust.mutate()}
          >
            <Button variant="ghost" disabled={untrust.isPending}>
              Rétablir la vérification habituelle
            </Button>
          </Confirm>
        )}
      </div>
      {review && (
        <CertificateReview
          certificate={review}
          reason={s.last_error ?? undefined}
          host={node.url}
          pending={trust.isPending}
          onTrust={() => trust.mutate(review.fingerprint)}
          onCancel={() => setReview(null)}
        />
      )}
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
