import { useState } from "react"
import type { FormEvent } from "react"
import { useSearchParams } from "react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  CheckCircle2,
  Cloud,
  CloudOff,
  Loader2,
  LogOut,
  Network,
  Pencil,
  Plus,
  Radar,
  RefreshCw,
  Smartphone,
  Trash2,
  UserPlus,
} from "lucide-react"
import { toast } from "sonner"
import type {
  AccessEntry,
  Found,
  InstallationConfig,
  SyncReport,
} from "@/lib/api"
import { api, currentUser } from "@/lib/api"
import { dateTime, relativeTime } from "@/lib/format"
import { PageHeader } from "@/components/app-shell"
import { InstallationMap } from "@/components/installation-map"
import { ConnectionDot } from "@/components/connection-badge"
import { Confirm } from "@/components/confirm"
import { NodesTab, SharesTab } from "@/components/federation"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
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
import { Switch } from "@/components/ui/switch"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
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

// ---------------------------------------------------------------------------------------
// Installations
// ---------------------------------------------------------------------------------------

type InstallationForm = {
  name: string
  host: string
  port: string
  enabled: boolean
}

function InstallationDialog({
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
  })
  const [test, setTest] = useState<{ ok: boolean; text: string } | null>(null)
  const set = (p: Partial<InstallationForm>) => setForm((f) => ({ ...f, ...p }))

  const runTest = useMutation({
    mutationFn: () => api.testInstallation(form.host, Number(form.port)),
    onSuccess: (r) =>
      setTest(
        r.ok
          ? {
              ok: true,
              text: `${r.zones?.length ?? 0} zone(s) : ${r.zones?.map((z) => z.name).join(", ")}`,
            }
          : { ok: false, text: r.error ?? "Pas de réponse" }
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
      }
      return editing
        ? api.updateInstallation(editing.id, data)
        : api.createInstallation(data)
    },
    onSuccess: () => {
      toast.success(editing ? "Installation modifiée" : "Installation ajoutée")
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
            {editing ? "Modifier l'installation" : "Ajouter une installation"}
          </DialogTitle>
          <DialogDescription>
            Adresse du WebServer Airzone sur le réseau local (API locale, port
            3000).
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor="in">Nom</Label>
            <Input
              id="in"
              value={form.name}
              placeholder="Maison"
              onChange={(e) => set({ name: e.target.value })}
            />
          </div>
          <div className="grid grid-cols-[1fr_6rem] gap-3">
            <div className="grid gap-2">
              <Label htmlFor="ih">Adresse IP</Label>
              <Input
                id="ih"
                value={form.host}
                placeholder="192.168.1.25"
                onChange={(e) => set({ host: e.target.value })}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="ip">Port</Label>
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
            Activée (interrogée régulièrement)
          </label>
          {test && (
            <Alert variant={test.ok ? "default" : "destructive"}>
              <AlertTitle>
                {test.ok ? "WebServer joignable" : "Échec"}
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
            {runTest.isPending && <Loader2 className="animate-spin" />} Tester
          </Button>
          <Button
            onClick={() => save.mutate()}
            disabled={!form.name || !form.host || save.isPending}
          >
            Enregistrer
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function InstallationsTab({ openAccess }: { openAccess: string | null }) {
  const qc = useQueryClient()
  const discovery = useQuery({
    queryKey: ["discovery"],
    queryFn: api.discoverySettings,
  })
  const configs = useQuery({
    queryKey: ["installations"],
    queryFn: api.installations,
  })
  const status = useQuery({ queryKey: ["status"], queryFn: api.status })
  const [dialog, setDialog] = useState<{
    initial?: InstallationConfig | { name: string; host: string; port: number }
    nonce: number
  } | null>(null)
  const [subnet, setSubnet] = useState<string | null>(null)
  const saved = discovery.data?.subnet ?? null
  const effective = discovery.data?.effective ?? null
  const subnetValue = subnet ?? saved ?? effective ?? ""
  const saveSubnet = useMutation({
    mutationFn: () => api.saveDiscoverySettings(subnetValue.trim() || null),
    onSuccess: (d) => {
      qc.setQueryData(["discovery"], d)
      setSubnet(null)
      qc.invalidateQueries({ queryKey: ["cloud"] })
      toast.success(
        d.subnet
          ? `Réseau du serveur : ${d.subnet}`
          : "Réseau du serveur : automatique"
      )
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const scan = useMutation({
    mutationFn: () => api.scan(subnetValue.trim() || undefined),
    onError: (e) => toast.error(errorText(e)),
  })
  const remove = useMutation({
    mutationFn: api.deleteInstallation,
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["installations"] })
      qc.invalidateQueries({ queryKey: ["status"] })
      qc.invalidateQueries({ queryKey: ["cloud"] })
    },
  })
  const known = new Set((configs.data ?? []).map((c) => `${c.host}:${c.port}`))
  const dirty = subnet !== null && subnet.trim() !== (saved ?? "")
  const locals = (configs.data ?? []).filter((c) => c.node_id === null)
  // The network and the search only matter to add a WebServer.
  const [adding, setAdding] = useState(false)
  const showSearch = adding || (configs.isSuccess && locals.length === 0)

  return (
    <div className="grid gap-6">
      <InstallationMap
        openFor={openAccess}
        onAdd={() => setDialog({ nonce: Date.now() })}
      />
      <Card className="rounded-3xl">
        <CardHeader>
          <CardTitle className="flex flex-wrap items-center gap-2">
            <Network className="size-4" /> WebServers sur le réseau de ce
            serveur
            {!showSearch ? null : saved ? (
              <Badge>{saved}</Badge>
            ) : effective ? (
              <Badge variant="secondary">{effective} · détecté</Badge>
            ) : (
              <Badge variant="destructive">Non défini</Badge>
            )}
          </CardTitle>
          <CardDescription>
            Les WebServers Airzone que ce serveur Breezone joint directement (un
            WebServer = une installation).
            {!showSearch
              ? ""
              : saved
                ? " Enregistré dans les réglages."
                : effective
                  ? " Déduit de l'adresse du serveur ; enregistrez-le pour le fixer."
                  : discovery.data?.in_container
                    ? " Le serveur tourne dans un conteneur et ne peut pas le deviner : indiquez-le (ex. 192.168.1.0/24)."
                    : " Indiquez-le (ex. 192.168.1.0/24)."}
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-4">
          {(configs.data ?? [])
            .filter((c) => c.node_id === null)
            .map((c) => {
              const live = status.data?.find((s) => s.id === c.id)
              return (
                <div
                  key={c.id}
                  className="flex items-center gap-3 rounded-2xl border p-3"
                >
                  <ConnectionDot state={live?.connection.state ?? "unknown"} />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="truncate font-medium">{c.name}</span>
                      {!c.enabled && (
                        <Badge variant="secondary">Désactivée</Badge>
                      )}
                    </div>
                    <div className="truncate text-xs text-muted-foreground">
                      {c.host}:{c.port}
                      {c.mac ? ` · ${c.mac}` : ""} · {live?.zones.length ?? 0}{" "}
                      zones
                    </div>
                  </div>
                  <Button
                    size="icon"
                    variant="ghost"
                    onClick={() => setDialog({ initial: c, nonce: Date.now() })}
                    aria-label="Modifier"
                  >
                    <Pencil />
                  </Button>
                  <Confirm
                    title={`Supprimer « ${c.name} » ?`}
                    description="Son historique et ses réglages Breezone seront supprimés."
                    onConfirm={() => remove.mutate(c.id)}
                  >
                    <Button size="icon" variant="ghost" aria-label="Supprimer">
                      <Trash2 />
                    </Button>
                  </Confirm>
                </div>
              )
            })}
          {showSearch && (
            <div className="grid gap-3 rounded-2xl border border-dashed p-3">
              <p className="text-sm text-muted-foreground">
                Cherchez le WebServer sur le réseau du serveur (port 3000), ou
                saisissez son adresse.
              </p>
              <div className="flex flex-wrap gap-2">
                <Input
                  className="max-w-56"
                  placeholder="ex. 192.168.1.0/24"
                  value={subnetValue}
                  onChange={(e) => setSubnet(e.target.value)}
                />
                <Button
                  variant={
                    dirty || (!saved && subnetValue) ? "default" : "ghost"
                  }
                  onClick={() => saveSubnet.mutate()}
                  disabled={saveSubnet.isPending || (!dirty && !!saved)}
                >
                  Enregistrer
                </Button>
                <Button
                  variant="outline"
                  onClick={() => scan.mutate()}
                  disabled={scan.isPending || !subnetValue.trim()}
                >
                  {scan.isPending ? (
                    <Loader2 className="animate-spin" />
                  ) : (
                    <Radar />
                  )}{" "}
                  Rechercher
                </Button>
                <Button
                  variant="ghost"
                  onClick={() => setDialog({ nonce: Date.now() })}
                >
                  Saisir l'adresse
                </Button>
                {locals.length > 0 && (
                  <Button variant="ghost" onClick={() => setAdding(false)}>
                    Annuler
                  </Button>
                )}
              </div>
              {scan.data && (
                <div className="grid gap-2">
                  <p className="text-sm text-muted-foreground">
                    {scan.data.found.length} WebServer(s) trouvé(s) sur{" "}
                    {scan.data.subnet ?? "le réseau"}.
                  </p>
                  {scan.data.found.map((f: Found) => (
                    <div
                      key={`${f.host}:${f.port}`}
                      className="flex items-center gap-3 rounded-2xl border p-3"
                    >
                      <div className="min-w-0 flex-1">
                        <div className="font-medium">{f.host}</div>
                        <div className="text-xs text-muted-foreground">
                          {[
                            f.mac,
                            f.firmware && `firmware ${f.firmware}`,
                            f.zones !== null && `${f.zones} zones`,
                          ]
                            .filter(Boolean)
                            .join(" · ")}
                        </div>
                      </div>
                      {known.has(`${f.host}:${f.port}`) ? (
                        <Badge variant="secondary">Déjà ajouté</Badge>
                      ) : (
                        <Button
                          size="sm"
                          onClick={() =>
                            setDialog({
                              initial: { name: "", host: f.host, port: f.port },
                              nonce: Date.now(),
                            })
                          }
                        >
                          Ajouter
                        </Button>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </CardContent>
        {!showSearch && (
          <CardFooter>
            <Button variant="outline" onClick={() => setAdding(true)}>
              <Plus /> Ajouter un WebServer
            </Button>
          </CardFooter>
        )}
      </Card>

      {dialog && (
        <InstallationDialog
          key={dialog.nonce}
          initial={dialog.initial}
          onClose={() => setDialog(null)}
        />
      )}
    </div>
  )
}

// ---------------------------------------------------------------------------------------
// Airzone Cloud
// ---------------------------------------------------------------------------------------

function ReportAlert({ report }: { report: SyncReport }) {
  return (
    <Alert>
      <CheckCircle2 />
      <AlertTitle>Synchronisation terminée</AlertTitle>
      <AlertDescription>
        <ul className="list-disc pl-4">
          {report.created.length > 0 && (
            <li>Créée(s) automatiquement : {report.created.join(", ")}</li>
          )}
          {report.updated.length > 0 && (
            <li>Mise(s) à jour : {report.updated.join(", ")}</li>
          )}
          {report.unreachable.length > 0 && (
            <li>
              Non jointe(s) depuis ce serveur : {report.unreachable.join(", ")}
            </li>
          )}
          {report.notes?.map((n) => (
            <li key={n} className="text-muted-foreground">
              {n}
            </li>
          ))}
          {report.created.length +
            report.updated.length +
            report.unreachable.length ===
            0 && <li>Aucune installation dans ce compte.</li>}
        </ul>
      </AlertDescription>
    </Alert>
  )
}

function CloudTab({ admin }: { admin: boolean }) {
  const qc = useQueryClient()
  const cloud = useQuery({ queryKey: ["cloud"], queryFn: api.cloud })
  const [email, setEmail] = useState("")
  const [password, setPassword] = useState("")
  const [remember, setRemember] = useState(true)
  const [report, setReport] = useState<SyncReport | null>(null)
  const [changing, setChanging] = useState(false)
  const done = (r: { report: SyncReport }) => {
    setReport(r.report)
    setPassword("")
    setChanging(false)
    qc.invalidateQueries({ queryKey: ["cloud"] })
    qc.invalidateQueries({ queryKey: ["status"] })
    qc.invalidateQueries({ queryKey: ["installations"] })
  }
  const connect = useMutation({
    mutationFn: () =>
      api.cloudConnect(
        email.trim() || (changing ? "" : (cloud.data?.email ?? "")),
        password,
        remember
      ),
    onSuccess: done,
    onError: (e) => toast.error(errorText(e)),
  })
  const sync = useMutation({
    mutationFn: api.cloudSync,
    onSuccess: done,
    onError: (e) => toast.error(errorText(e)),
  })
  const forget = useMutation({
    mutationFn: api.cloudForget,
    onSuccess: () => {
      setReport(null)
      qc.invalidateQueries({ queryKey: ["cloud"] })
    },
  })
  const s = cloud.data

  const submit = (e: FormEvent) => {
    e.preventDefault()
    connect.mutate()
  }

  return (
    <div className="grid gap-6">
      <Card className="rounded-3xl">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            {s?.auto_sync ? (
              <Cloud className="size-5 text-primary" />
            ) : (
              <CloudOff className="size-5 text-muted-foreground" />
            )}{" "}
            Compte Airzone
          </CardTitle>
          <CardDescription>
            Facultatif. Le pilotage n'utilise jamais le cloud : Breezone y lit
            seulement vos installations (nom, ville), vos groupes de zones et
            leur ordre, crée automatiquement les installations trouvées sur ce
            réseau, puis resynchronise chaque jour sans écraser vos
            personnalisations.
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-4">
          {s?.email && (
            <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm">
              <span>
                Compte : <strong>{s.email}</strong>
              </span>
              <span className="text-muted-foreground">
                Dernière synchro :{" "}
                {s.last_sync ? dateTime(s.last_sync) : "jamais"}
              </span>
              {s.auto_sync ? (
                <Badge>Synchro quotidienne</Badge>
              ) : (
                <Badge variant="secondary">Identifiants non mémorisés</Badge>
              )}
            </div>
          )}
          {s?.last_error && (
            <Alert variant="destructive">
              <AlertTitle>Dernière synchronisation en échec</AlertTitle>
              <AlertDescription>{s.last_error}</AlertDescription>
            </Alert>
          )}
          {report && <ReportAlert report={report} />}
          {admin && s?.auto_sync && !changing && (
            <div className="flex flex-wrap gap-2">
              <Button
                variant="outline"
                onClick={() => sync.mutate()}
                disabled={sync.isPending}
              >
                <RefreshCw className={sync.isPending ? "animate-spin" : ""} />{" "}
                Synchroniser maintenant
              </Button>
              <Button variant="ghost" onClick={() => setChanging(true)}>
                Changer de compte
              </Button>
              <Confirm
                title="Oublier le compte Airzone ?"
                description="La synchronisation s'arrête. Vos installations et groupes restent dans Breezone."
                onConfirm={() => forget.mutate()}
              >
                <Button variant="ghost" className="text-destructive">
                  Oublier
                </Button>
              </Confirm>
            </div>
          )}
          {admin && (!s?.auto_sync || changing) && (
            <form onSubmit={submit} className="grid gap-3 sm:max-w-md">
              {s?.email && !s.auto_sync && (
                <p className="text-sm text-muted-foreground">
                  Le mot de passe n'est pas mémorisé : saisissez-le pour
                  synchroniser.
                </p>
              )}
              <div className="grid gap-2">
                <Label htmlFor="ce">E-mail Airzone</Label>
                <Input
                  id="ce"
                  type="email"
                  autoComplete="off"
                  value={email}
                  placeholder={changing ? "" : (s?.email ?? "")}
                  onChange={(e) => setEmail(e.target.value)}
                  required={changing || !s?.email}
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="cp">Mot de passe Airzone</Label>
                <Input
                  id="cp"
                  type="password"
                  autoComplete="off"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  required
                />
              </div>
              <label className="flex items-center gap-2 text-sm">
                <Checkbox
                  checked={remember}
                  onCheckedChange={(v) => setRemember(v === true)}
                />{" "}
                Mémoriser (chiffré) pour la synchro quotidienne
              </label>
              <div className="flex flex-wrap gap-2">
                <Button type="submit" disabled={connect.isPending}>
                  {connect.isPending && <Loader2 className="animate-spin" />}{" "}
                  Connecter et importer
                </Button>
                {changing && (
                  <Button
                    type="button"
                    variant="ghost"
                    onClick={() => setChanging(false)}
                  >
                    Annuler
                  </Button>
                )}
                {s?.email && !s.auto_sync && (
                  <Confirm
                    title="Oublier le compte Airzone ?"
                    description="Vos installations et groupes restent dans Breezone."
                    onConfirm={() => forget.mutate()}
                  >
                    <Button type="button" variant="ghost">
                      Oublier
                    </Button>
                  </Confirm>
                )}
              </div>
            </form>
          )}
        </CardContent>
      </Card>
    </div>
  )
}

// ---------------------------------------------------------------------------------------
// Users
// ---------------------------------------------------------------------------------------

/** Installations a user can see, and whether they can drive them. */
function AccessEditor({
  value,
  onChange,
}: {
  value: AccessEntry[]
  onChange: (v: AccessEntry[]) => void
}) {
  const installations = useQuery({
    queryKey: ["installations"],
    queryFn: api.installations,
  })
  const list = installations.data ?? []
  const level = (id: number) =>
    value.find((a) => a.installation_id === id)?.permission ?? "none"
  const set = (id: number, l: string) =>
    onChange([
      ...value.filter((a) => a.installation_id !== id),
      ...(l === "none"
        ? []
        : [
            { installation_id: id, permission: l as AccessEntry["permission"] },
          ]),
    ])
  return (
    <div className="grid gap-2">
      {list.length === 0 && (
        <p className="text-sm text-muted-foreground">Aucune installation.</p>
      )}
      {list.map((i) => (
        <div
          key={i.id}
          className="flex flex-wrap items-center gap-3 rounded-xl border p-2.5 text-sm"
        >
          <span className="min-w-0 flex-1">
            {i.name}
            <span className="block text-xs text-muted-foreground">
              {i.node_id !== null
                ? "via un autre serveur Breezone"
                : (i.location ?? `${i.host}:${i.port}`)}
            </span>
          </span>
          <Select value={level(i.id)} onValueChange={(l) => set(i.id, l)}>
            <SelectTrigger className="w-44">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none">Aucun accès</SelectItem>
              <SelectItem value="read">Lecture seule</SelectItem>
              <SelectItem value="control">Lecture et pilotage</SelectItem>
            </SelectContent>
          </Select>
        </div>
      ))}
      <p className="text-xs text-muted-foreground">
        Les installations ajoutées plus tard ne sont données à personne :
        revenez ici pour les attribuer.
      </p>
    </div>
  )
}

function accessSummary(access: AccessEntry[]) {
  if (access.length === 0) return "Aucune installation"
  const control = access.filter((a) => a.permission === "control").length
  const n = access.length
  return `${n} installation${n > 1 ? "s" : ""}${control ? ` · ${control} en pilotage` : " · lecture seule"}`
}

function UsersTab() {
  const qc = useQueryClient()
  const me = currentUser()
  const users = useQuery({ queryKey: ["users"], queryFn: api.users })
  const [open, setOpen] = useState(false)
  const [form, setForm] = useState({ username: "", password: "", role: "user" })
  const [newAccess, setNewAccess] = useState<AccessEntry[]>([])
  const [editing, setEditing] = useState<{
    id: number
    username: string
    access: AccessEntry[]
  } | null>(null)
  const [reset, setReset] = useState<{ id: number; username: string } | null>(
    null
  )
  const [newPassword, setNewPassword] = useState("")
  const refresh = () => qc.invalidateQueries({ queryKey: ["users"] })
  const create = useMutation({
    mutationFn: () =>
      api.createUser(
        form.username,
        form.password,
        form.role,
        form.role === "user" ? newAccess : []
      ),
    onSuccess: () => {
      toast.success("Utilisateur créé")
      setOpen(false)
      setForm({ username: "", password: "", role: "user" })
      setNewAccess([])
      refresh()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const update = useMutation({
    mutationFn: ({
      id,
      data,
    }: {
      id: number
      data: { role?: string; password?: string; access?: AccessEntry[] }
    }) => api.updateUser(id, data),
    onSuccess: () => {
      toast.success("Utilisateur modifié")
      setReset(null)
      setEditing(null)
      setNewPassword("")
      refresh()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const remove = useMutation({
    mutationFn: api.deleteUser,
    onSuccess: refresh,
    onError: (e) => toast.error(errorText(e)),
  })

  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>Utilisateurs</CardTitle>
        <CardDescription>
          Les administrateurs voient et pilotent tout, et configurent le
          serveur. Un utilisateur ne voit que les installations que vous lui
          donnez, en lecture seule ou avec le pilotage.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-2">
        {(users.data ?? []).map((u) => (
          <div
            key={u.id}
            className="flex flex-wrap items-center gap-3 rounded-2xl border p-3"
          >
            <span className="inline-flex size-9 items-center justify-center rounded-full bg-primary/15 font-semibold text-primary uppercase">
              {u.username[0]}
            </span>
            <div className="min-w-0 flex-1">
              <div className="font-medium">
                {u.username}{" "}
                {u.id === me?.id && (
                  <span className="text-xs text-muted-foreground">(vous)</span>
                )}
              </div>
              <div className="text-xs text-muted-foreground">
                {u.role === "admin"
                  ? "Toutes les installations"
                  : accessSummary(u.access)}{" "}
                · créé le {dateTime(u.created_at)}
              </div>
            </div>
            {u.role === "user" && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() =>
                  setEditing({
                    id: u.id,
                    username: u.username,
                    access: u.access,
                  })
                }
              >
                Installations
              </Button>
            )}
            <Select
              value={u.role}
              onValueChange={(role) =>
                update.mutate({ id: u.id, data: { role } })
              }
              disabled={u.id === me?.id}
            >
              <SelectTrigger className="w-40">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="user">Utilisateur</SelectItem>
                <SelectItem value="admin">Administrateur</SelectItem>
              </SelectContent>
            </Select>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setReset({ id: u.id, username: u.username })}
            >
              Mot de passe
            </Button>
            {u.id !== me?.id && (
              <Confirm
                title={`Supprimer ${u.username} ?`}
                description="Tous ses appareils seront déconnectés."
                onConfirm={() => remove.mutate(u.id)}
              >
                <Button size="icon" variant="ghost" aria-label="Supprimer">
                  <Trash2 />
                </Button>
              </Confirm>
            )}
          </div>
        ))}
      </CardContent>
      <CardFooter>
        <Button onClick={() => setOpen(true)}>
          <UserPlus /> Ajouter un utilisateur
        </Button>
      </CardFooter>

      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Nouvel utilisateur</DialogTitle>
          </DialogHeader>
          <div className="grid gap-4">
            <div className="grid gap-2">
              <Label htmlFor="nu">Identifiant</Label>
              <Input
                id="nu"
                value={form.username}
                onChange={(e) => setForm({ ...form, username: e.target.value })}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="np">Mot de passe (8 caractères min.)</Label>
              <Input
                id="np"
                type="password"
                value={form.password}
                onChange={(e) => setForm({ ...form, password: e.target.value })}
              />
            </div>
            <div className="grid gap-2">
              <Label>Rôle</Label>
              <Select
                value={form.role}
                onValueChange={(role) => setForm({ ...form, role })}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="user">Utilisateur</SelectItem>
                  <SelectItem value="admin">Administrateur</SelectItem>
                </SelectContent>
              </Select>
            </div>
            {form.role === "user" ? (
              <div className="grid gap-2">
                <Label>Installations</Label>
                <AccessEditor value={newAccess} onChange={setNewAccess} />
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">
                Un administrateur voit et pilote toutes les installations.
              </p>
            )}
          </div>
          <DialogFooter>
            <Button
              onClick={() => create.mutate()}
              disabled={
                create.isPending || !form.username || form.password.length < 8
              }
            >
              Créer
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={editing !== null}
        onOpenChange={(o) => !o && setEditing(null)}
      >
        <DialogContent className="max-h-[90vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Installations de {editing?.username}</DialogTitle>
            <DialogDescription>
              Ce qu'il voit dans le web et l'application, et ce qu'il peut
              piloter. Le changement s'applique aussitôt sur ses appareils.
            </DialogDescription>
          </DialogHeader>
          {editing && (
            <AccessEditor
              value={editing.access}
              onChange={(access) => setEditing({ ...editing, access })}
            />
          )}
          <DialogFooter>
            <Button variant="ghost" onClick={() => setEditing(null)}>
              Annuler
            </Button>
            <Button
              onClick={() =>
                editing &&
                update.mutate({
                  id: editing.id,
                  data: { access: editing.access },
                })
              }
              disabled={update.isPending}
            >
              Enregistrer
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={reset !== null} onOpenChange={(o) => !o && setReset(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              Nouveau mot de passe pour {reset?.username}
            </DialogTitle>
            <DialogDescription>
              Tous ses appareils seront déconnectés.
            </DialogDescription>
          </DialogHeader>
          <Input
            type="password"
            value={newPassword}
            onChange={(e) => setNewPassword(e.target.value)}
            placeholder="8 caractères minimum"
          />
          <DialogFooter>
            <Button
              onClick={() =>
                reset &&
                update.mutate({ id: reset.id, data: { password: newPassword } })
              }
              disabled={newPassword.length < 8}
            >
              Enregistrer
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Card>
  )
}

// ---------------------------------------------------------------------------------------
// Devices & account
// ---------------------------------------------------------------------------------------

function DevicesTab() {
  const qc = useQueryClient()
  const sessions = useQuery({ queryKey: ["sessions"], queryFn: api.sessions })
  const revoke = useMutation({
    mutationFn: api.revokeSession,
    onSuccess: () => {
      toast.success("Appareil déconnecté")
      qc.invalidateQueries({ queryKey: ["sessions"] })
    },
  })
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>Appareils connectés</CardTitle>
        <CardDescription>
          Chaque appareil reste connecté sans redemander le mot de passe.
          Déconnectez ici un téléphone perdu : il est coupé immédiatement.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-2">
        {(sessions.data ?? []).map((s) => (
          <div
            key={s.id}
            className="flex items-center gap-3 rounded-2xl border p-3"
          >
            <Smartphone className="size-5 text-muted-foreground" />
            <div className="min-w-0 flex-1">
              <div className="font-medium">
                {s.device_name}{" "}
                {s.current && <Badge className="ml-1">Cet appareil</Badge>}
              </div>
              <div className="text-xs text-muted-foreground">
                Connecté le {dateTime(s.created_at)} · actif{" "}
                {relativeTime(s.last_used_at)}
              </div>
            </div>
            {!s.current && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => revoke.mutate(s.id)}
              >
                <LogOut /> Déconnecter
              </Button>
            )}
          </div>
        ))}
      </CardContent>
    </Card>
  )
}

function AccountTab() {
  const [current, setCurrent] = useState("")
  const [next, setNext] = useState("")
  const change = useMutation({
    mutationFn: () => api.changePassword(current, next),
    onSuccess: () => {
      toast.success(
        "Mot de passe modifié — vos autres appareils ont été déconnectés"
      )
      setCurrent("")
      setNext("")
    },
    onError: (e) =>
      toast.error(
        errorText(e) === "current password is incorrect"
          ? "Mot de passe actuel incorrect"
          : errorText(e)
      ),
  })
  return (
    <Card className="rounded-3xl sm:max-w-lg">
      <CardHeader>
        <CardTitle>Mot de passe</CardTitle>
      </CardHeader>
      <CardContent className="grid gap-4">
        <div className="grid gap-2">
          <Label htmlFor="pc">Mot de passe actuel</Label>
          <Input
            id="pc"
            type="password"
            autoComplete="current-password"
            value={current}
            onChange={(e) => setCurrent(e.target.value)}
          />
        </div>
        <div className="grid gap-2">
          <Label htmlFor="pn">Nouveau mot de passe</Label>
          <Input
            id="pn"
            type="password"
            autoComplete="new-password"
            value={next}
            onChange={(e) => setNext(e.target.value)}
          />
        </div>
      </CardContent>
      <CardFooter>
        <Button
          onClick={() => change.mutate()}
          disabled={!current || next.length < 8 || change.isPending}
        >
          Modifier
        </Button>
      </CardFooter>
    </Card>
  )
}

export function SettingsPage() {
  const [params, setParams] = useSearchParams()
  const admin = currentUser()?.role === "admin"
  const tabs = [
    { id: "cloud", label: "Compte Airzone" },
    ...(admin ? [{ id: "installations", label: "Installations" }] : []),
    ...(admin
      ? [
          { id: "nodes", label: "Nœuds" },
          { id: "shares", label: "Partages" },
        ]
      : []),
    ...(admin ? [{ id: "users", label: "Utilisateurs" }] : []),
    { id: "devices", label: "Appareils" },
    { id: "account", label: "Mon compte" },
  ]
  const tab = tabs.some((t) => t.id === params.get("tab"))
    ? params.get("tab")!
    : tabs[0].id

  return (
    <>
      <PageHeader title="Réglages" />
      {params.get("welcome") && (
        <Alert className="mb-6 rounded-2xl">
          <CheckCircle2 />
          <AlertTitle>Compte créé 🎉</AlertTitle>
          <AlertDescription>
            Ajoutez maintenant vos installations : le plus simple est de
            connecter votre compte Airzone (onglet « Compte Airzone »), Breezone
            trouvera vos WebServers sur le réseau et importera vos groupes.
          </AlertDescription>
        </Alert>
      )}
      <Tabs value={tab} onValueChange={(t) => setParams({ tab: t })}>
        <TabsList className="mb-6 flex-wrap">
          {tabs.map((t) => (
            <TabsTrigger key={t.id} value={t.id}>
              {t.label}
            </TabsTrigger>
          ))}
        </TabsList>
        {admin && (
          <TabsContent value="installations">
            <InstallationsTab openAccess={params.get("access")} />
          </TabsContent>
        )}
        <TabsContent value="cloud">
          <CloudTab admin={admin} />
        </TabsContent>
        {admin && (
          <TabsContent value="nodes">
            <NodesTab />
          </TabsContent>
        )}
        {admin && (
          <TabsContent value="shares">
            <SharesTab />
          </TabsContent>
        )}
        {admin && (
          <TabsContent value="users">
            <UsersTab />
          </TabsContent>
        )}
        <TabsContent value="devices">
          <DevicesTab />
        </TabsContent>
        <TabsContent value="account">
          <AccountTab />
        </TabsContent>
      </Tabs>
    </>
  )
}
