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
  PeerView,
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
import { t } from "@/lib/i18n"

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e))

const NODE_STATE: Record<NodeState, { label: string; tone: string }> = {
  online: { label: t("Connecté"), tone: "bg-success" },
  offline: { label: t("Injoignable"), tone: "bg-destructive" },
  revoked: { label: t("Clé révoquée"), tone: "bg-destructive" },
  expired: { label: t("Partage expiré"), tone: "bg-warning" },
  error: { label: t("Erreur"), tone: "bg-destructive" },
  pending: { label: t("En attente d'acceptation"), tone: "bg-warning" },
  certificate: { label: t("Certificat refusé"), tone: "bg-destructive" },
  unknown: { label: t("En attente"), tone: "bg-muted-foreground" },
}

/** Connection state of another Breezone server. */
/** Names of what servers exchange (ADR 0012); unknown ones are shown as they come. */
const CAPABILITY_LABEL: Record<string, string> = {
  "climate.zone": t("climatisation"),
  "cover.position": t("volets"),
  "cover.tilt": t("orientation des lames"),
  "light.onoff": t("lumières"),
  "energy.power": t("énergie"),
  "charger.session": t("recharge"),
  "pool.filtration": t("piscine"),
  "federation.watch": t("suivi en direct"),
  "federation.control": t("pilotage"),
  "federation.schedules": t("programmations Airzone"),
  "federation.hello": t("échange des versions"),
}

const capabilityLabel = (item: string) => {
  const [name, version] = item.split("@")
  const label = CAPABILITY_LABEL[name] ?? name
  return version && version !== "1" ? `${label} (v${version})` : label
}

/** Version of another server and how well it understands this one. */
function PeerBadges({ peer }: { peer: PeerView | null }) {
  if (!peer) return null
  const version = peer.version ?? (peer.legacy ? "≤ 0.10.0" : null)
  return (
    <>
      {version && (
        <Badge variant="outline" className="tabular">
          Breezone {version}
        </Badge>
      )}
      {peer.channel === "beta" && (
        <Badge variant="outline" className="border-warning/50 text-warning">
          {t("bêta")}
        </Badge>
      )}
      {peer.compatibility.state === "limited" && (
        <Badge variant="secondary">{t("Fonctions limitées")}</Badge>
      )}
      {peer.compatibility.state === "outdated" && (
        <Badge variant="destructive">{t("À mettre à jour")}</Badge>
      )}
    </>
  )
}

/** What one side can do that the other cannot, and who should be updated. */
function PeerLimits({ peer, name }: { peer: PeerView | null; name: string }) {
  if (!peer || peer.compatibility.state === "compatible") return null
  const { missing, newer, state } = peer.compatibility
  return (
    <div className="mt-1 space-y-0.5 text-xs text-muted-foreground">
      {state === "outdated" && (
        <p className="text-destructive">
          {t(
            "Les deux serveurs ne se comprennent plus : mettez à jour le plus ancien."
          )}
        </p>
      )}
      {missing.length > 0 && (
        <p>
          {t("Pas encore sur « {name} » : {list}. Mettez-le à jour.", {
            name,
            list: missing.map(capabilityLabel).join(", "),
          })}
        </p>
      )}
      {newer.length > 0 && (
        <p>
          {t(
            "« {name} » sait aussi : {list}. Mettez à jour ce serveur pour en profiter.",
            { name, list: newer.map(capabilityLabel).join(", ") }
          )}
        </p>
      )}
    </div>
  )
}

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
          <p className="font-medium">{t("Certificat HTTPS non valide")}</p>
          {reason && <p className="text-muted-foreground">{reason}</p>}
        </div>
      </div>
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
        <dt className="text-muted-foreground">{t("Adresse")}</dt>
        <dd className="break-all">{host}</dd>
        <dt className="text-muted-foreground">{t("Délivré à")}</dt>
        <dd className="break-all">{certificate.subject ?? "—"}</dd>
        <dt className="text-muted-foreground">{t("Valable pour")}</dt>
        <dd className="break-all">
          {certificate.names.length ? certificate.names.join(", ") : "—"}
        </dd>
        <dt className="text-muted-foreground">{t("Émis par")}</dt>
        <dd className="break-all">
          {certificate.self_signed
            ? t("lui-même (auto-signé)")
            : (certificate.issuer ?? "—")}
        </dd>
        <dt className="text-muted-foreground">{t("Valable")}</dt>
        <dd>
          {certificate.not_before
            ? t("du {date}", { date: dateTime(certificate.not_before) }) + " "
            : ""}
          {certificate.not_after
            ? t("au {date}", { date: dateTime(certificate.not_after) })
            : ""}
        </dd>
        <dt className="text-muted-foreground">{t("Empreinte SHA-256")}</dt>
        <dd className="font-mono break-all">
          {fingerprintText(certificate.fingerprint)}
        </dd>
      </dl>
      <p className="text-xs text-muted-foreground">
        {t(
          "Faites confiance à ce certificat seulement si vous savez pourquoi il n'est pas valide (par exemple un certificat du NAS qui ne couvre pas ce nom). Pour en être sûr, comparez l'empreinte avec celle affichée par votre navigateur sur cette adresse. Breezone n'acceptera ensuite que ce certificat précis et vous redemandera votre accord s'il change."
        )}
      </p>
      <div className="flex flex-wrap justify-end gap-2">
        {onCancel && (
          <Button size="sm" variant="ghost" onClick={onCancel}>
            {t("Annuler")}
          </Button>
        )}
        <Button
          size="sm"
          variant="destructive"
          onClick={onTrust}
          disabled={pending}
        >
          {pending && <Loader2 className="animate-spin" />}
          {t("Faire confiance à ce certificat")}
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
  p === "control" ? t("Lecture et pilotage") : t("Lecture seule")

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
      toast.success(t("Serveur enregistré"))
    },
    onError: (e) => toast.error(errorText(e)),
  })
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{t("Ce serveur")}</CardTitle>
        <CardDescription>
          {t(
            "Comment les autres serveurs Breezone le voient et le joignent. L'adresse publique est celle de votre tunnel Cloudflare (https://…), ou son adresse locale si les deux serveurs sont sur le même réseau."
          )}
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4 sm:grid-cols-2">
        <div className="grid gap-2">
          <Label htmlFor="node-name">{t("Nom")}</Label>
          <Input
            id="node-name"
            value={nameValue}
            placeholder={t("Bureau")}
            onChange={(e) => setName(e.target.value)}
          />
        </div>
        <div className="grid gap-2">
          <Label htmlFor="node-url">{t("Adresse publique")}</Label>
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
          {t("Enregistrer")}
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
    onSuccess: (created) => {
      qc.invalidateQueries({ queryKey: ["shares"] })
      if (created) setToken(created)
      else {
        toast.success(t("Partage modifié"))
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
            <DialogTitle>
              {t("Jeton d'invitation · {label}", { label })}
            </DialogTitle>
            <DialogDescription>
              {t(
                "Sur l'autre serveur Breezone : Réglages › Serveurs › Ajouter, puis collez ce jeton (ou scannez le QR code avec l'application). Il est utilisable une seule fois, pendant 15 minutes, et ne contient aucune clé d'accès. Vous devrez ensuite accepter la demande ici, dans « À qui ce serveur est partagé »."
              )}
              {reciprocal &&
                " " +
                  t(
                    "L'autre serveur pourra aussi vous partager ses installations : elles apparaîtront dans « Quels serveurs partagent avec celui-ci » une fois la demande acceptée."
                  )}
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
                  .then(() => toast.success(t("Jeton copié")))
              }
            >
              <Copy /> {t("Copier")}
            </Button>
          </div>
          <DialogFooter>
            <Button onClick={onClose}>{t("Terminé")}</Button>
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
            {share ? t("Modifier le partage") : t("Nouveau partage")}
          </DialogTitle>
          <DialogDescription>
            {t(
              "Un autre serveur Breezone pourra afficher (et piloter, si vous le permettez) les installations choisies. Seules les installations jointes directement par ce serveur sont partageables."
            )}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor="share-label">{t("Pour qui")}</Label>
            <Input
              id="share-label"
              value={label}
              placeholder={t("Maison de Camille")}
              onChange={(e) => setLabel(e.target.value)}
            />
          </div>
          <div className="grid gap-2">
            <Label>{t("Installations partagées")}</Label>
            {local.length === 0 && (
              <p className="text-sm text-muted-foreground">
                {t("Aucune installation locale.")}
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
              <Label>{t("Droit")}</Label>
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
                  <SelectItem value="control">
                    {t("Lecture et pilotage")}
                  </SelectItem>
                  <SelectItem value="read">{t("Lecture seule")}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="grid gap-2">
              <Label>{t("Validité")}</Label>
              <Select value={expiry} onValueChange={setExpiry}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="never">
                    {share?.expires_at
                      ? t("Inchangée ({date})", {
                          date: dateTime(share.expires_at),
                        })
                      : t("Jusqu'à révocation")}
                  </SelectItem>
                  <SelectItem value="1">{t("1 jour")}</SelectItem>
                  <SelectItem value="7">{t("{n} jours", { n: 7 })}</SelectItem>
                  <SelectItem value="30">
                    {t("{n} jours", { n: 30 })}
                  </SelectItem>
                  <SelectItem value="365">{t("1 an")}</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
          {!share && (
            <div className="grid gap-2">
              <Label htmlFor="share-url">
                {t("Adresse de ce serveur pour l'autre")}
              </Label>
              <Input
                id="share-url"
                value={url}
                placeholder={
                  identity.data?.public_url ??
                  t(
                    "https://breezone.mondomaine.fr ou http://192.168.1.20:8427"
                  )
                }
                onChange={(e) => setUrl(e.target.value)}
              />
              <p className="text-xs text-muted-foreground">
                {t(
                  "Vide = l'adresse publique de ce serveur. HTTP n'est accepté que vers une adresse locale."
                )}
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
                <span className="font-medium">{t("Partage réciproque")}</span>
                <span className="block text-muted-foreground">
                  {t(
                    "Proposer à l'autre serveur de partager aussi ses installations avec celui-ci. Il choisira lesquelles en ajoutant le jeton ; elles apparaîtront ici quand vous accepterez sa demande."
                  )}
                </span>
              </span>
            </label>
          )}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            {t("Annuler")}
          </Button>
          <Button
            onClick={() => save.mutate()}
            disabled={save.isPending || !label.trim() || selected.size === 0}
          >
            {save.isPending && <Loader2 className="animate-spin" />}
            {share ? t("Enregistrer") : t("Créer le jeton")}
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
      toast.success(t("Partage révoqué"))
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const approve = useMutation({
    mutationFn: api.approveShare,
    onSuccess: (s) => {
      qc.invalidateQueries({ queryKey: ["shares"] })
      toast.success(
        t("Accès accordé au serveur « {name} »", { name: s.remote_name ?? "?" })
      )
    },
    onError: (e) => toast.error(errorText(e)),
  })
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{t("À qui ce serveur est partagé")}</CardTitle>
        <CardDescription>
          {t(
            "Les autres serveurs Breezone qui voient des installations de celui-ci. Révoquer un partage coupe l'accès immédiatement."
          )}
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
                    · {t("serveur « {name} »", { name: s.remote_name })}
                  </span>
                )}
                <Badge variant="outline">{permissionLabel(s.permission)}</Badge>
                {s.expired && (
                  <Badge variant="destructive">{t("Expiré")}</Badge>
                )}
                {s.status === "invited" && (
                  <Badge variant="secondary">
                    {t("Invitation non utilisée")}
                  </Badge>
                )}
                {(s.reciprocal || s.reverse) && (
                  <Badge variant="outline">
                    <ArrowLeftRight /> {t("Réciproque")}
                  </Badge>
                )}
                {s.status === "pending" && (
                  <Badge className="bg-warning text-black">
                    {t("Demande d'accès")}
                  </Badge>
                )}
                <PeerBadges peer={s.peer} />
              </div>
              <div className="text-xs text-muted-foreground">
                {s.installations.map((i) => i.name).join(", ") ||
                  t("Aucune installation")}
              </div>
              <PeerLimits peer={s.peer} name={s.remote_name ?? s.label} />
              {s.status === "pending" && (
                <p className="mt-1 text-sm">
                  {t(
                    "Le serveur « {name} » demande l'accès{from}{when}. Acceptez seulement si c'est bien vous qui venez d'utiliser l'invitation.",
                    {
                      name: s.remote_name ?? "?",
                      from: s.last_address
                        ? t(" depuis {address}", { address: s.last_address })
                        : "",
                      when: s.requested_at
                        ? ` (${relativeTime(s.requested_at)})`
                        : "",
                    }
                  )}
                  {s.reciprocal &&
                    " " +
                      (s.remote_shares_back
                        ? t(
                            "Il vous partage aussi ses installations : elles apparaîtront dans « Quels serveurs partagent avec celui-ci » dès que vous accepterez."
                          )
                        : t("Il n'a pas accepté le partage réciproque."))}
                </p>
              )}
              {s.reciprocal && s.status === "active" && (
                <p className="mt-1 text-xs text-muted-foreground">
                  {s.remote_shares_back
                    ? t(
                        "« {name} » vous partage aussi ses installations : choisissez celles à afficher dans Réglages › Serveurs.",
                        { name: s.remote_name ?? "?" }
                      )
                    : t(
                        "« {name} » ne partage pas ses installations en retour. Pour les voir ici : sur ce serveur-là, Réglages › Serveurs › Nouveau partage, puis ajoutez le jeton dans Réglages › Serveurs.",
                        { name: s.remote_name ?? "?" }
                      )}
                </p>
              )}
              {s.reverse && (
                <p className="mt-1 text-xs text-muted-foreground">
                  {t("En retour de l'invitation de « {name} ».", {
                    name: s.remote_name ?? "?",
                  })}
                </p>
              )}
              <div className="text-xs text-muted-foreground">
                {s.status === "invited"
                  ? s.pairing_expires_at && !s.pairing_expired
                    ? t("Invitation valable jusqu'à {date}", {
                        date: dateTime(s.pairing_expires_at),
                      })
                    : t(
                        "Invitation expirée : supprimez-la et créez-en une nouvelle"
                      )
                  : s.status === "pending"
                    ? t("En attente de votre réponse")
                    : s.connected
                      ? t("Connecté maintenant")
                      : s.last_seen
                        ? t("Dernière connexion {when}", {
                            when: relativeTime(s.last_seen),
                          })
                        : t("Jamais connecté")}
                {s.last_address &&
                  " ·" + t(" depuis {address}", { address: s.last_address })}
                {s.expires_at &&
                  !s.expired &&
                  " · " +
                    t("expire le {date}", { date: dateTime(s.expires_at) })}
              </div>
            </div>
            {s.status === "pending" && (
              <Button
                size="sm"
                onClick={() => approve.mutate(s.id)}
                disabled={approve.isPending}
              >
                {t("Accepter")}
              </Button>
            )}
            <Button
              size="sm"
              variant="ghost"
              onClick={() => setDialog({ share: s, nonce: Date.now() })}
            >
              {t("Modifier")}
            </Button>
            <Confirm
              title={
                s.status === "pending"
                  ? t("Refuser la demande de « {name} » ?", {
                      name: s.remote_name ?? "?",
                    })
                  : t("Révoquer « {name} » ?", { name: s.label })
              }
              description={t(
                "L'autre serveur perd l'accès à ces installations immédiatement."
              )}
              onConfirm={() => revoke.mutate(s.id)}
            >
              <Button size="sm" variant="ghost" className="text-destructive">
                {s.status === "pending" ? t("Refuser") : t("Révoquer")}
              </Button>
            </Confirm>
          </div>
        ))}
        {shares.data?.length === 0 && (
          <p className="text-sm text-muted-foreground">
            {t("Ce serveur n'est partagé avec aucun autre.")}
          </p>
        )}
      </CardContent>
      <CardFooter>
        <Button onClick={() => setDialog({ nonce: Date.now() })}>
          <Share2 /> {t("Nouveau partage")}
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

/** The servers of the other sites: this one, those it shares with, those that share with it. */
export function ServersTab() {
  return (
    <div className="grid gap-6">
      <IdentityCard />
      <SharesCard />
      <NodesCard />
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
  const clean = token.replace(/\s+/g, "")
  if (!clean.startsWith("bz2.")) return null
  try {
    const b64 = clean.slice(4).replace(/-/g, "+").replace(/_/g, "/")
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
        t(
          "Serveur « {name} » ajouté : acceptez la demande sur ce serveur pour voir ses installations",
          { name: n.name }
        )
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
          <DialogTitle>{t("Ajouter un serveur Breezone")}</DialogTitle>
          <DialogDescription>
            {t(
              "Sur le serveur de l'autre site : Réglages › Serveurs › Nouveau partage. Collez ici le jeton obtenu (valable 15 minutes, une seule fois), puis acceptez la demande sur ce serveur-là. Ses installations apparaîtront alors ici. Les deux serveurs se reconnaissent ensuite par une signature : aucune clé ne circule."
            )}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor="node-token">{t("Jeton d'invitation")}</Label>
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
            <Label htmlFor="node-url-override">
              {t("Adresse (facultatif)")}
            </Label>
            <Input
              id="node-url-override"
              value={url}
              placeholder={t(
                "Celle du jeton, sinon par ex. http://192.168.1.20:8427"
              )}
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
                    {t("Partager aussi mes installations avec « {name} »", {
                      name: invitation?.name ?? "?",
                    })}
                  </span>
                  <span className="block text-muted-foreground">
                    {t(
                      "Ce serveur propose un partage réciproque. Il verra les installations cochées dès qu'il aura accepté votre demande ; vous pourrez révoquer ce partage dans Réglages › Serveurs."
                    )}
                  </span>
                </span>
              </label>
              {shareBack && (
                <>
                  {local.length === 0 && (
                    <p className="text-sm text-muted-foreground">
                      {t("Aucune installation locale à partager.")}
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
                    <Label>{t("Droit")}</Label>
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
                          {t("Lecture et pilotage")}
                        </SelectItem>
                        <SelectItem value="read">
                          {t("Lecture seule")}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  {!identity.data?.public_url && (
                    <p className="text-sm text-destructive">
                      {t(
                        "Renseignez d'abord l'adresse publique de ce serveur (Réglages › Serveurs › Ce serveur) : l'autre serveur en a besoin pour le joindre."
                      )}
                    </p>
                  )}
                </>
              )}
            </div>
          )}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            {t("Annuler")}
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
            {add.isPending && <Loader2 className="animate-spin" />}{" "}
            {t("Ajouter")}
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
      toast.success(t("Adresse enregistrée"))
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const remove = useMutation({
    mutationFn: () => api.deleteNode(node.id),
    onSuccess: invalidate,
    onError: (e) => toast.error(errorText(e)),
  })
  // Which of the installations it shares are shown on this server.
  const [picking, setPicking] = useState<Set<number> | null>(null)
  const pick = useMutation({
    mutationFn: (included: number[]) =>
      api.setNodeInstallations(node.id, included),
    onSuccess: () => {
      setPicking(null)
      invalidate()
      toast.success(t("Installations affichées mises à jour"))
    },
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
      toast.success(t("Certificat de « {name} » accepté", { name: node.name }))
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
      toast.success(t("Vérification HTTPS habituelle rétablie"))
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
                title={t("Certificat HTTPS accepté manuellement")}
              >
                <ShieldCheck /> {t("Certificat accepté")}
              </Badge>
            )}
            <PeerBadges peer={node.peer} />
          </div>
          <div className="text-xs text-muted-foreground">
            {node.installations.map((i) => i.name).join(", ") ||
              (s.state === "pending"
                ? t(
                    "Ses installations apparaîtront quand « {name} » aura accepté la demande (Réglages › Serveurs sur ce serveur)",
                    { name: node.name }
                  )
                : t("Aucune installation partagée"))}
          </div>
          <div className="text-xs text-muted-foreground">
            {s.state === "online"
              ? t("Connecté")
              : s.last_seen
                ? t("Dernière réponse {when}", {
                    when: relativeTime(s.last_seen),
                  })
                : t("Jamais joint")}
            {s.last_error &&
              s.state !== "online" &&
              s.state !== "pending" &&
              ` · ${s.last_error}`}
          </div>
          <PeerLimits peer={node.peer} name={node.name} />
        </div>
        <Button
          size="icon"
          variant="ghost"
          aria-label={t("Actualiser")}
          onClick={() => refresh.mutate()}
          disabled={refresh.isPending}
        >
          <RefreshCw className={cn(refresh.isPending && "animate-spin")} />
        </Button>
        <Confirm
          title={t("Retirer « {name} » ?", { name: node.name })}
          description={t(
            "Ses installations disparaîtront de ce serveur (historique local compris). Pensez à révoquer le partage sur l'autre serveur."
          )}
          onConfirm={() => remove.mutate()}
        >
          <Button size="icon" variant="ghost" aria-label={t("Retirer")}>
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
            <Link2 /> {t("Enregistrer l'adresse")}
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
            {t("Examiner le certificat")}
          </Button>
        )}
        {node.offered.length > 0 && picking === null && (
          <Button
            variant="outline"
            onClick={() =>
              setPicking(
                new Set(
                  node.offered.filter((o) => o.included).map((o) => o.remote_id)
                )
              )
            }
          >
            {t("Choisir les installations ({n}/{total})", {
              n: node.offered.filter((o) => o.included).length,
              total: node.offered.length,
            })}
          </Button>
        )}
        {node.certificate_pinned && s.state !== "certificate" && (
          <Confirm
            title={t("Rétablir la vérification HTTPS habituelle ?")}
            description={t(
              "À faire quand l'autre serveur a un certificat valide. S'il n'en a pas, la connexion sera de nouveau refusée."
            )}
            onConfirm={() => untrust.mutate()}
          >
            <Button variant="ghost" disabled={untrust.isPending}>
              {t("Rétablir la vérification habituelle")}
            </Button>
          </Confirm>
        )}
      </div>
      {picking && (
        <div className="grid gap-2 rounded-2xl border p-3 text-sm">
          <p className="text-muted-foreground">
            {t(
              "Installations que « {name} » partage avec ce serveur : cochez celles à afficher ici. Celles qu'il partagera plus tard seront affichées jusqu'à ce que vous les décochiez.",
              { name: node.name }
            )}
          </p>
          {node.offered.map((o) => (
            <label
              key={o.remote_id}
              className="flex items-center gap-3 rounded-xl border p-2.5"
            >
              <Checkbox
                checked={picking.has(o.remote_id)}
                onCheckedChange={(v) => {
                  const next = new Set(picking)
                  if (v === true) next.add(o.remote_id)
                  else next.delete(o.remote_id)
                  setPicking(next)
                }}
              />
              <span className="flex-1">{o.name}</span>
              {o.location && (
                <span className="text-xs text-muted-foreground">
                  {o.location}
                </span>
              )}
            </label>
          ))}
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="ghost" onClick={() => setPicking(null)}>
              {t("Annuler")}
            </Button>
            <Button
              size="sm"
              onClick={() => pick.mutate([...picking])}
              disabled={pick.isPending}
            >
              {pick.isPending && <Loader2 className="animate-spin" />}
              {t("Enregistrer")}
            </Button>
          </div>
        </div>
      )}
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

function NodesCard() {
  const nodes = useQuery({
    queryKey: ["nodes"],
    queryFn: api.nodes,
    refetchInterval: 10_000,
  })
  const [adding, setAdding] = useState(false)
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{t("Quels serveurs partagent avec celui-ci")}</CardTitle>
        <CardDescription>
          {t(
            "Les serveurs d'autres sites qui partagent leurs installations avec celui-ci : vous les pilotez d'ici, sans VPN. Les serveurs se parlent entre eux ; la clé n'arrive jamais sur les téléphones."
          )}
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-3">
        {(nodes.data ?? []).map((n) => (
          <NodeRow key={n.id} node={n} />
        ))}
        {nodes.data?.length === 0 && (
          <p className="text-sm text-muted-foreground">
            {t("Aucun autre serveur pour l'instant.")}
          </p>
        )}
      </CardContent>
      <CardFooter>
        <Button onClick={() => setAdding(true)}>
          <Plus /> {t("Ajouter un serveur")}
        </Button>
      </CardFooter>
      {adding && <AddNodeDialog onClose={() => setAdding(false)} />}
    </Card>
  )
}
