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
  Languages,
} from "lucide-react"
import { toast } from "sonner"
import type {
  AccessEntry,
  Found,
  InstallationConfig,
  SyncReport,
  UserRights,
} from "@/lib/api"
import { ALL_RIGHTS, api, currentUser } from "@/lib/api"
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
import { browserLang, LANG_NAMES, setLang, t, tn, type Lang } from "@/lib/i18n"

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
          ? t("Réseau du serveur : {subnet}", { subnet: d.subnet })
          : t("Réseau du serveur : automatique")
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
            <Network className="size-4" />{" "}
            {t("WebServers sur le réseau de ce serveur")}
            {!showSearch ? null : saved ? (
              <Badge>{saved}</Badge>
            ) : effective ? (
              <Badge variant="secondary">
                {t("{subnet} · détecté", { subnet: effective })}
              </Badge>
            ) : (
              <Badge variant="destructive">{t("Non défini")}</Badge>
            )}
          </CardTitle>
          <CardDescription>
            {t(
              "Les WebServers Airzone que ce serveur Breezone joint directement (un WebServer = une installation)."
            )}
            {!showSearch
              ? ""
              : " " +
                (saved
                  ? t("Enregistré dans les réglages.")
                  : effective
                    ? t(
                        "Déduit de l'adresse du serveur ; enregistrez-le pour le fixer."
                      )
                    : discovery.data?.in_container
                      ? t(
                          "Le serveur tourne dans un conteneur et ne peut pas le deviner : indiquez-le (ex. 192.168.1.0/24)."
                        )
                      : t("Indiquez-le (ex. 192.168.1.0/24)."))}
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
                        <Badge variant="secondary">{t("Désactivée")}</Badge>
                      )}
                    </div>
                    <div className="truncate text-xs text-muted-foreground">
                      {c.host}:{c.port}
                      {c.mac ? ` · ${c.mac}` : ""} ·{" "}
                      {tn(live?.zones.length ?? 0, "{n} zone", "{n} zones")}
                    </div>
                  </div>
                  <Button
                    size="icon"
                    variant="ghost"
                    onClick={() => setDialog({ initial: c, nonce: Date.now() })}
                    aria-label={t("Modifier")}
                  >
                    <Pencil />
                  </Button>
                  <Confirm
                    title={t("Supprimer « {name} » ?", { name: c.name })}
                    description={t(
                      "Son historique et ses réglages Breezone seront supprimés."
                    )}
                    onConfirm={() => remove.mutate(c.id)}
                  >
                    <Button
                      size="icon"
                      variant="ghost"
                      aria-label={t("Supprimer")}
                    >
                      <Trash2 />
                    </Button>
                  </Confirm>
                </div>
              )
            })}
          {showSearch && (
            <div className="grid gap-3 rounded-2xl border border-dashed p-3">
              <p className="text-sm text-muted-foreground">
                {t(
                  "Cherchez le WebServer sur le réseau du serveur (port 3000), ou saisissez son adresse."
                )}
              </p>
              <div className="flex flex-wrap gap-2">
                <Input
                  className="max-w-56"
                  placeholder={t("ex. 192.168.1.0/24")}
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
                  {t("Enregistrer")}
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
                  {t("Rechercher")}
                </Button>
                <Button
                  variant="ghost"
                  onClick={() => setDialog({ nonce: Date.now() })}
                >
                  {t("Saisir l'adresse")}
                </Button>
                {locals.length > 0 && (
                  <Button variant="ghost" onClick={() => setAdding(false)}>
                    {t("Annuler")}
                  </Button>
                )}
              </div>
              {scan.data && (
                <div className="grid gap-2">
                  <p className="text-sm text-muted-foreground">
                    {t("{n} WebServer(s) trouvé(s) sur {subnet}.", {
                      n: scan.data.found.length,
                      subnet: scan.data.subnet ?? t("le réseau"),
                    })}
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
                            f.firmware && t("firmware {v}", { v: f.firmware }),
                            f.zones !== null &&
                              tn(f.zones, "{n} zone", "{n} zones"),
                          ]
                            .filter(Boolean)
                            .join(" · ")}
                        </div>
                      </div>
                      {known.has(`${f.host}:${f.port}`) ? (
                        <Badge variant="secondary">{t("Déjà ajouté")}</Badge>
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
                          {t("Ajouter")}
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
              <Plus /> {t("Ajouter un WebServer")}
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
      <AlertTitle>{t("Synchronisation terminée")}</AlertTitle>
      <AlertDescription>
        <ul className="list-disc pl-4">
          {report.created.length > 0 && (
            <li>
              {t("Créée(s) automatiquement : {list}", {
                list: report.created.join(", "),
              })}
            </li>
          )}
          {report.updated.length > 0 && (
            <li>
              {t("Mise(s) à jour : {list}", {
                list: report.updated.join(", "),
              })}
            </li>
          )}
          {report.unreachable.length > 0 && (
            <li>
              {t("Non jointe(s) depuis ce serveur : {list}", {
                list: report.unreachable.join(", "),
              })}
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
            0 && <li>{t("Aucune installation dans ce compte.")}</li>}
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
            {t("Compte Airzone")}
          </CardTitle>
          <CardDescription>
            {t(
              "Facultatif. Le pilotage n'utilise jamais le cloud : Breezone y lit seulement vos installations (nom, ville), vos groupes de zones et leur ordre, crée automatiquement les installations trouvées sur ce réseau, puis resynchronise chaque jour sans écraser vos personnalisations."
            )}
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-4">
          {s?.email && (
            <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm">
              <span>
                {t("Compte :")} <strong>{s.email}</strong>
              </span>
              <span className="text-muted-foreground">
                {t("Dernière synchro : {when}", {
                  when: s.last_sync ? dateTime(s.last_sync) : t("jamais"),
                })}
              </span>
              {s.auto_sync ? (
                <Badge>{t("Synchro quotidienne")}</Badge>
              ) : (
                <Badge variant="secondary">
                  {t("Identifiants non mémorisés")}
                </Badge>
              )}
            </div>
          )}
          {s?.last_error && (
            <Alert variant="destructive">
              <AlertTitle>{t("Dernière synchronisation en échec")}</AlertTitle>
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
                {t("Synchroniser maintenant")}
              </Button>
              <Button variant="ghost" onClick={() => setChanging(true)}>
                {t("Changer de compte")}
              </Button>
              <Confirm
                title={t("Oublier le compte Airzone ?")}
                description={t(
                  "La synchronisation s'arrête. Vos installations et groupes restent dans Breezone."
                )}
                onConfirm={() => forget.mutate()}
              >
                <Button variant="ghost" className="text-destructive">
                  {t("Oublier")}
                </Button>
              </Confirm>
            </div>
          )}
          {admin && (!s?.auto_sync || changing) && (
            <form onSubmit={submit} className="grid gap-3 sm:max-w-md">
              {s?.email && !s.auto_sync && (
                <p className="text-sm text-muted-foreground">
                  {t(
                    "Le mot de passe n'est pas mémorisé : saisissez-le pour synchroniser."
                  )}
                </p>
              )}
              <div className="grid gap-2">
                <Label htmlFor="ce">{t("E-mail Airzone")}</Label>
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
                <Label htmlFor="cp">{t("Mot de passe Airzone")}</Label>
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
                {t("Mémoriser (chiffré) pour la synchro quotidienne")}
              </label>
              <div className="flex flex-wrap gap-2">
                <Button type="submit" disabled={connect.isPending}>
                  {connect.isPending && <Loader2 className="animate-spin" />}{" "}
                  {t("Connecter et importer")}
                </Button>
                {changing && (
                  <Button
                    type="button"
                    variant="ghost"
                    onClick={() => setChanging(false)}
                  >
                    {t("Annuler")}
                  </Button>
                )}
                {s?.email && !s.auto_sync && (
                  <Confirm
                    title={t("Oublier le compte Airzone ?")}
                    description={t(
                      "Vos installations et groupes restent dans Breezone."
                    )}
                    onConfirm={() => forget.mutate()}
                  >
                    <Button type="button" variant="ghost">
                      {t("Oublier")}
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
        <p className="text-sm text-muted-foreground">
          {t("Aucune installation.")}
        </p>
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
                ? t("via un autre serveur Breezone")
                : (i.location ?? `${i.host}:${i.port}`)}
            </span>
          </span>
          <Select value={level(i.id)} onValueChange={(l) => set(i.id, l)}>
            <SelectTrigger className="w-44">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none">{t("Aucun accès")}</SelectItem>
              <SelectItem value="read">{t("Lecture seule")}</SelectItem>
              <SelectItem value="control">
                {t("Lecture et pilotage")}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
      ))}
      <p className="text-xs text-muted-foreground">
        {t(
          "Les installations ajoutées plus tard ne sont données à personne : revenez ici pour les attribuer."
        )}
      </p>
    </div>
  )
}

const RIGHTS: { key: keyof UserRights; label: string; hint: string }[] = [
  {
    key: "zones",
    label: t("Renommer et regrouper les zones"),
    hint: t("Nom, groupe et ordre des zones."),
  },
  {
    key: "routines",
    label: t("Créer et modifier les routines"),
    hint: t("Lancer une routine reste possible avec le pilotage."),
  },
  {
    key: "schedules",
    label: t("Créer et modifier les programmations Airzone"),
    hint: t("Les activer ou les désactiver reste possible avec le pilotage."),
  },
]

/** What a user may change on the installations they drive, besides driving them. */
function RightsEditor({
  value,
  onChange,
}: {
  value: UserRights
  onChange: (v: UserRights) => void
}) {
  return (
    <div className="grid gap-2">
      {RIGHTS.map((r) => (
        <label
          key={r.key}
          className="flex cursor-pointer items-center gap-3 rounded-xl border p-2.5 text-sm"
        >
          <span className="min-w-0 flex-1">
            {r.label}
            <span className="block text-xs text-muted-foreground">
              {r.hint}
            </span>
          </span>
          <Switch
            checked={value[r.key]}
            onCheckedChange={(on) => onChange({ ...value, [r.key]: on })}
          />
        </label>
      ))}
      <p className="text-xs text-muted-foreground">
        {t(
          "Sur les installations qu'il pilote. Sans ces droits, il pilote sans rien modifier."
        )}
      </p>
    </div>
  )
}

function accessSummary(access: AccessEntry[], rights: UserRights) {
  if (access.length === 0) return t("Aucune installation")
  const control = access.filter((a) => a.permission === "control").length
  const n = access.length
  const limited = control > 0 && RIGHTS.some((r) => !rights[r.key])
  return [
    tn(n, "{n} installation", "{n} installations"),
    control ? t("{n} en pilotage", { n: control }) : t("lecture seule"),
    ...(limited ? [t("droits limités")] : []),
  ].join(" · ")
}

function UsersTab() {
  const qc = useQueryClient()
  const me = currentUser()
  const users = useQuery({ queryKey: ["users"], queryFn: api.users })
  const [open, setOpen] = useState(false)
  const [form, setForm] = useState({ username: "", password: "", role: "user" })
  const [newAccess, setNewAccess] = useState<AccessEntry[]>([])
  const [newRights, setNewRights] = useState<UserRights>(ALL_RIGHTS)
  const [editing, setEditing] = useState<{
    id: number
    username: string
    access: AccessEntry[]
    rights: UserRights
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
        form.role === "user" ? newAccess : [],
        form.role === "user" ? newRights : ALL_RIGHTS
      ),
    onSuccess: () => {
      toast.success(t("Utilisateur créé"))
      setOpen(false)
      setForm({ username: "", password: "", role: "user" })
      setNewAccess([])
      setNewRights(ALL_RIGHTS)
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
      data: {
        role?: string
        password?: string
        access?: AccessEntry[]
        rights?: UserRights
      }
    }) => api.updateUser(id, data),
    onSuccess: () => {
      toast.success(t("Utilisateur modifié"))
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
        <CardTitle>{t("Utilisateurs")}</CardTitle>
        <CardDescription>
          {t(
            "Les administrateurs voient et pilotent tout, et configurent le serveur. Un utilisateur ne voit que les installations que vous lui donnez, en lecture seule ou avec le pilotage, et vous choisissez ce qu'il peut y modifier."
          )}
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
                  <span className="text-xs text-muted-foreground">
                    {t("(vous)")}
                  </span>
                )}
              </div>
              <div className="text-xs text-muted-foreground">
                {u.role === "admin"
                  ? t("Toutes les installations")
                  : accessSummary(u.access, u.rights)}{" "}
                · {t("créé le {date}", { date: dateTime(u.created_at) })}
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
                    rights: u.rights,
                  })
                }
              >
                {t("Accès et droits")}
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
                <SelectItem value="user">{t("Utilisateur")}</SelectItem>
                <SelectItem value="admin">{t("Administrateur")}</SelectItem>
              </SelectContent>
            </Select>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setReset({ id: u.id, username: u.username })}
            >
              {t("Mot de passe")}
            </Button>
            {u.id !== me?.id && (
              <Confirm
                title={t("Supprimer {name} ?", { name: u.username })}
                description={t("Tous ses appareils seront déconnectés.")}
                onConfirm={() => remove.mutate(u.id)}
              >
                <Button size="icon" variant="ghost" aria-label={t("Supprimer")}>
                  <Trash2 />
                </Button>
              </Confirm>
            )}
          </div>
        ))}
      </CardContent>
      <CardFooter>
        <Button onClick={() => setOpen(true)}>
          <UserPlus /> {t("Ajouter un utilisateur")}
        </Button>
      </CardFooter>

      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("Nouvel utilisateur")}</DialogTitle>
          </DialogHeader>
          <div className="grid gap-4">
            <div className="grid gap-2">
              <Label htmlFor="nu">{t("Identifiant")}</Label>
              <Input
                id="nu"
                value={form.username}
                onChange={(e) => setForm({ ...form, username: e.target.value })}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="np">
                {t("Mot de passe (8 caractères min.)")}
              </Label>
              <Input
                id="np"
                type="password"
                value={form.password}
                onChange={(e) => setForm({ ...form, password: e.target.value })}
              />
            </div>
            <div className="grid gap-2">
              <Label>{t("Rôle")}</Label>
              <Select
                value={form.role}
                onValueChange={(role) => setForm({ ...form, role })}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="user">{t("Utilisateur")}</SelectItem>
                  <SelectItem value="admin">{t("Administrateur")}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            {form.role === "user" ? (
              <div className="grid gap-2">
                <Label>{t("Installations")}</Label>
                <AccessEditor value={newAccess} onChange={setNewAccess} />
                <Label className="mt-2">{t("Droits")}</Label>
                <RightsEditor value={newRights} onChange={setNewRights} />
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">
                {t(
                  "Un administrateur voit et pilote toutes les installations."
                )}
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
              {t("Créer")}
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
            <DialogTitle>
              {t("Accès de {name}", { name: editing?.username ?? "" })}
            </DialogTitle>
            <DialogDescription>
              {t(
                "Ce qu'il voit dans le web et l'application, ce qu'il peut piloter et modifier. Le changement s'applique aussitôt sur ses appareils."
              )}
            </DialogDescription>
          </DialogHeader>
          {editing && (
            <div className="grid gap-4">
              <div className="grid gap-2">
                <Label>{t("Installations")}</Label>
                <AccessEditor
                  value={editing.access}
                  onChange={(access) => setEditing({ ...editing, access })}
                />
              </div>
              <div className="grid gap-2">
                <Label>{t("Droits")}</Label>
                <RightsEditor
                  value={editing.rights}
                  onChange={(rights) => setEditing({ ...editing, rights })}
                />
              </div>
            </div>
          )}
          <DialogFooter>
            <Button variant="ghost" onClick={() => setEditing(null)}>
              {t("Annuler")}
            </Button>
            <Button
              onClick={() =>
                editing &&
                update.mutate({
                  id: editing.id,
                  data: { access: editing.access, rights: editing.rights },
                })
              }
              disabled={update.isPending}
            >
              {t("Enregistrer")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={reset !== null} onOpenChange={(o) => !o && setReset(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {t("Nouveau mot de passe pour {name}", {
                name: reset?.username ?? "",
              })}
            </DialogTitle>
            <DialogDescription>
              {t("Tous ses appareils seront déconnectés.")}
            </DialogDescription>
          </DialogHeader>
          <Input
            type="password"
            value={newPassword}
            onChange={(e) => setNewPassword(e.target.value)}
            placeholder={t("8 caractères minimum")}
          />
          <DialogFooter>
            <Button
              onClick={() =>
                reset &&
                update.mutate({ id: reset.id, data: { password: newPassword } })
              }
              disabled={newPassword.length < 8}
            >
              {t("Enregistrer")}
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
      toast.success(t("Appareil déconnecté"))
      qc.invalidateQueries({ queryKey: ["sessions"] })
    },
  })
  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{t("Appareils connectés")}</CardTitle>
        <CardDescription>
          {t(
            "Chaque appareil reste connecté sans redemander le mot de passe. Déconnectez ici un téléphone perdu : il est coupé immédiatement."
          )}
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
                {s.current && (
                  <Badge className="ml-1">{t("Cet appareil")}</Badge>
                )}
              </div>
              <div className="text-xs text-muted-foreground">
                {t("Connecté le {date} · actif {when}", {
                  date: dateTime(s.created_at),
                  when: relativeTime(s.last_used_at),
                })}
              </div>
            </div>
            {!s.current && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => revoke.mutate(s.id)}
              >
                <LogOut /> {t("Déconnecter")}
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
        t("Mot de passe modifié — vos autres appareils ont été déconnectés")
      )
      setCurrent("")
      setNext("")
    },
    onError: (e) => toast.error(errorText(e)),
  })
  return (
    <div className="grid gap-6">
      <LanguageCard />
      <Card className="rounded-3xl sm:max-w-lg">
        <CardHeader>
          <CardTitle>{t("Mot de passe")}</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor="pc">{t("Mot de passe actuel")}</Label>
            <Input
              id="pc"
              type="password"
              autoComplete="current-password"
              value={current}
              onChange={(e) => setCurrent(e.target.value)}
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="pn">{t("Nouveau mot de passe")}</Label>
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
            {t("Modifier")}
          </Button>
        </CardFooter>
      </Card>
    </div>
  )
}

/** Language of the web UI and of the server messages, kept in the profile. */
function LanguageCard() {
  const me = useQuery({ queryKey: ["me"], queryFn: api.me })
  const save = useMutation({
    mutationFn: (language: Lang | null) => api.setLanguage(language),
    onSuccess: (_, language) => setLang(language),
    onError: (e) => toast.error(errorText(e)),
  })
  return (
    <Card className="rounded-3xl sm:max-w-lg">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Languages className="size-4" /> {t("Langue")}
        </CardTitle>
        <CardDescription>
          {t(
            "Celle de l'interface web et des messages du serveur. L'application mobile suit la langue du téléphone."
          )}
        </CardDescription>
      </CardHeader>
      <CardContent>
        <Select
          value={me.data ? (me.data.language ?? "auto") : undefined}
          onValueChange={(v) => save.mutate(v === "auto" ? null : (v as Lang))}
          disabled={!me.data || save.isPending}
        >
          <SelectTrigger className="w-64">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="auto">
              {t("Automatique ({name})", { name: LANG_NAMES[browserLang()] })}
            </SelectItem>
            {(Object.keys(LANG_NAMES) as Lang[]).map((l) => (
              <SelectItem key={l} value={l}>
                {LANG_NAMES[l]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </CardContent>
    </Card>
  )
}

export function SettingsPage() {
  const [params, setParams] = useSearchParams()
  const admin = currentUser()?.role === "admin"
  const tabs = [
    { id: "cloud", label: t("Compte Airzone") },
    ...(admin ? [{ id: "installations", label: t("Installations") }] : []),
    ...(admin
      ? [
          { id: "nodes", label: t("Nœuds") },
          { id: "shares", label: t("Partages") },
        ]
      : []),
    ...(admin ? [{ id: "users", label: t("Utilisateurs") }] : []),
    { id: "devices", label: t("Appareils") },
    { id: "account", label: t("Mon compte") },
  ]
  const tab = tabs.some((x) => x.id === params.get("tab"))
    ? params.get("tab")!
    : tabs[0].id

  return (
    <>
      <PageHeader title={t("Réglages")} />
      {params.get("welcome") && (
        <Alert className="mb-6 rounded-2xl">
          <CheckCircle2 />
          <AlertTitle>{t("Compte créé 🎉")}</AlertTitle>
          <AlertDescription>
            {t(
              "Ajoutez maintenant vos installations : le plus simple est de connecter votre compte Airzone (onglet « Compte Airzone »), Breezone trouvera vos WebServers sur le réseau et importera vos groupes."
            )}
          </AlertDescription>
        </Alert>
      )}
      <Tabs value={tab} onValueChange={(v) => setParams({ tab: v })}>
        <TabsList className="mb-6 flex-wrap">
          {tabs.map((x) => (
            <TabsTrigger key={x.id} value={x.id}>
              {x.label}
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
