import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Bot, Copy, KeyRound, Plus, Trash2 } from "lucide-react"
import { toast } from "sonner"
import { api } from "@/lib/api"
import { dateTime, errorText, relativeTime } from "@/lib/format"
import { t } from "@/lib/i18n"
import { Confirm } from "@/components/confirm"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

function copy(text: string, done: string) {
  navigator.clipboard.writeText(text).then(() => toast.success(done))
}

/** A block of text to copy as it is (a token, a command). */
function CopyBlock({ label, text }: { label: string; text: string }) {
  return (
    <div className="grid gap-1.5">
      <Label>{label}</Label>
      <div className="flex gap-2">
        <pre className="min-w-0 flex-1 overflow-x-auto rounded-xl border bg-muted/40 px-3 py-2 font-mono text-xs whitespace-pre">
          {text}
        </pre>
        <Button
          size="icon"
          variant="outline"
          aria-label={t("Copier")}
          onClick={() => copy(text, t("Copié"))}
        >
          <Copy />
        </Button>
      </div>
    </div>
  )
}

/**
 * The access tokens of the user: an assistant (MCP) or a script signs in with one instead of
 * the password. A token is shown once, when it is created.
 */
export function AccessTokens() {
  const qc = useQueryClient()
  const tokens = useQuery({ queryKey: ["tokens"], queryFn: api.tokens })
  const identity = useQuery({
    queryKey: ["node-identity"],
    queryFn: api.identity,
  })
  const [open, setOpen] = useState(false)
  const [name, setName] = useState("")
  const [created, setCreated] = useState<string | null>(null)
  const refresh = () => qc.invalidateQueries({ queryKey: ["tokens"] })
  const create = useMutation({
    mutationFn: () => api.createToken(name.trim()),
    onSuccess: (r) => {
      setCreated(r.token)
      refresh()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const revoke = useMutation({
    mutationFn: api.revokeToken,
    onSuccess: () => {
      toast.success(t("Jeton révoqué"))
      refresh()
    },
    onError: (e) => toast.error(errorText(e)),
  })
  const server = (identity.data?.public_url ?? window.location.origin).replace(
    /\/+$/,
    ""
  )
  const endpoint = `${server}/mcp`
  const close = () => {
    setOpen(false)
    setCreated(null)
    setName("")
  }

  return (
    <Card className="rounded-3xl">
      <CardHeader>
        <CardTitle>{t("Jetons d'accès (MCP)")}</CardTitle>
        <CardDescription>
          {t(
            "Un assistant (Claude, ChatGPT…) ou un script se connecte avec un jeton au lieu de votre mot de passe, avec vos droits : il lit l'état de la maison, pilote les appareils, écrit des routines, des automatismes et des programmations, et installe des modules si vous êtes administrateur. Révoquez un jeton pour le couper aussitôt."
          )}
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-2">
        {(tokens.data ?? []).length === 0 && (
          <p className="text-sm text-muted-foreground">
            {t("Aucun jeton pour l'instant.")}
          </p>
        )}
        {(tokens.data ?? []).map((tk) => (
          <div
            key={tk.id}
            className="flex items-center gap-3 rounded-2xl border p-3"
          >
            <KeyRound className="size-5 text-muted-foreground" />
            <div className="min-w-0 flex-1">
              <div className="font-medium">{tk.name}</div>
              <div className="text-xs text-muted-foreground">
                <span className="font-mono">{tk.hint}</span> ·{" "}
                {t("créé le {date}", { date: dateTime(tk.created_at) })} ·{" "}
                {tk.last_used_at
                  ? t("utilisé {when}", { when: relativeTime(tk.last_used_at) })
                  : t("jamais utilisé")}
              </div>
            </div>
            <Confirm
              title={t("Révoquer « {name} » ?", { name: tk.name })}
              description={t(
                "L'outil qui s'en sert est coupé aussitôt et ne pourra plus se connecter avec."
              )}
              onConfirm={() => revoke.mutate(tk.id)}
            >
              <Button size="icon" variant="ghost" aria-label={t("Révoquer")}>
                <Trash2 />
              </Button>
            </Confirm>
          </div>
        ))}
      </CardContent>
      <CardFooter>
        <Button variant="outline" onClick={() => setOpen(true)}>
          <Plus /> {t("Créer un jeton")}
        </Button>
      </CardFooter>

      <Dialog open={open} onOpenChange={(o) => !o && close()}>
        <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>
              {created ? t("Jeton créé") : t("Nouveau jeton d'accès")}
            </DialogTitle>
            <DialogDescription>
              {created
                ? t(
                    "Copiez-le maintenant : il ne sera plus affiché. Il donne vos droits à qui le possède."
                  )
                : t(
                    "Donnez-lui le nom de l'outil qui s'en servira, pour le reconnaître et le révoquer plus tard."
                  )}
            </DialogDescription>
          </DialogHeader>
          {created ? (
            <div className="grid gap-4">
              <CopyBlock label={t("Jeton")} text={created} />
              <CopyBlock label={t("Adresse du serveur MCP")} text={endpoint} />
              <CopyBlock
                label={t("Claude Code")}
                text={`claude mcp add --transport http breezone ${endpoint} --header "Authorization: Bearer ${created}"`}
              />
              <CopyBlock
                label={t("Autres clients MCP (configuration JSON)")}
                text={JSON.stringify(
                  {
                    mcpServers: {
                      breezone: {
                        type: "http",
                        url: endpoint,
                        headers: { Authorization: `Bearer ${created}` },
                      },
                    },
                  },
                  null,
                  2
                )}
              />
              <Alert>
                <Bot />
                <AlertDescription>
                  {t(
                    "L'assistant doit joindre cette adresse : depuis l'extérieur de la maison, mettez l'adresse publique du serveur (Réglages › Serveurs)."
                  )}
                </AlertDescription>
              </Alert>
            </div>
          ) : (
            <div className="grid gap-2">
              <Label htmlFor="token-name">{t("Nom")}</Label>
              <Input
                id="token-name"
                value={name}
                maxLength={64}
                placeholder={t("Claude sur mon ordinateur")}
                onChange={(e) => setName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && name.trim()) create.mutate()
                }}
              />
            </div>
          )}
          <DialogFooter>
            {created ? (
              <Button onClick={close}>{t("Terminé")}</Button>
            ) : (
              <Button
                onClick={() => create.mutate()}
                disabled={!name.trim() || create.isPending}
              >
                {t("Créer")}
              </Button>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Card>
  )
}
