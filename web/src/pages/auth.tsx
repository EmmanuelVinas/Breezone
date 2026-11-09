import { useState } from "react"
import type { FormEvent, ReactNode } from "react"
import { useLocation, useNavigate } from "react-router"
import { useQueryClient } from "@tanstack/react-query"
import { Loader2 } from "lucide-react"
import { api } from "@/lib/api"
import { Brand } from "@/components/brand"
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
import { Alert, AlertDescription } from "@/components/ui/alert"
import { t } from "@/lib/i18n"

function AuthLayout({
  title,
  description,
  children,
}: {
  title: string
  description: ReactNode
  children: ReactNode
}) {
  return (
    <div className="fz-backdrop flex min-h-svh flex-col items-center justify-center gap-8 p-4">
      <Brand className="scale-125" />
      <Card className="w-full max-w-sm rounded-3xl border-border/60 bg-card/80 shadow-2xl backdrop-blur-xl">
        <CardHeader>
          <CardTitle className="text-xl">{title}</CardTitle>
          <CardDescription>{description}</CardDescription>
        </CardHeader>
        <CardContent>{children}</CardContent>
      </Card>
      <p className="max-w-sm text-center text-xs text-muted-foreground">
        {t(
          "Breezone pilote vos installations Airzone en local, sans abonnement ni cloud."
        )}
      </p>
    </div>
  )
}

function useSubmit(action: () => Promise<void>) {
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const onSubmit = async (e: FormEvent) => {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      await action()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(false)
    }
  }
  return { error, busy, onSubmit }
}

export function LoginPage() {
  const [username, setUsername] = useState("")
  const [password, setPassword] = useState("")
  const navigate = useNavigate()
  const location = useLocation()
  const from = (location.state as { from?: string } | null)?.from ?? "/"
  const { error, busy, onSubmit } = useSubmit(async () => {
    await api.login(username, password)
    navigate(from, { replace: true })
  })
  return (
    <AuthLayout
      title={t("Connexion")}
      description={t("Vous resterez connecté sur cet appareil.")}
    >
      <form onSubmit={onSubmit} className="flex flex-col gap-4">
        <div className="grid gap-2">
          <Label htmlFor="username">{t("Identifiant")}</Label>
          <Input
            id="username"
            autoComplete="username"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            required
            autoFocus
          />
        </div>
        <div className="grid gap-2">
          <Label htmlFor="password">{t("Mot de passe")}</Label>
          <Input
            id="password"
            type="password"
            autoComplete="current-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
          />
        </div>
        {error && (
          <Alert variant="destructive">
            <AlertDescription>
              {error === "invalid username or password"
                ? t("Identifiant ou mot de passe incorrect")
                : error}
            </AlertDescription>
          </Alert>
        )}
        <Button type="submit" size="lg" disabled={busy}>
          {busy && <Loader2 className="animate-spin" />} {t("Se connecter")}
        </Button>
      </form>
    </AuthLayout>
  )
}

export function SetupPage() {
  const [username, setUsername] = useState("admin")
  const [password, setPassword] = useState("")
  const [confirm, setConfirm] = useState("")
  const qc = useQueryClient()
  const navigate = useNavigate()
  const { error, busy, onSubmit } = useSubmit(async () => {
    if (password !== confirm)
      throw new Error(t("Les mots de passe ne correspondent pas"))
    await api.setup(username, password)
    await qc.invalidateQueries({ queryKey: ["setup"] })
    navigate("/settings?tab=cloud&welcome=1", { replace: true })
  })
  return (
    <AuthLayout
      title={t("Bienvenue 👋")}
      description={t("Créez le compte administrateur de ce serveur Breezone.")}
    >
      <form onSubmit={onSubmit} className="flex flex-col gap-4">
        <div className="grid gap-2">
          <Label htmlFor="username">{t("Identifiant")}</Label>
          <Input
            id="username"
            autoComplete="username"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            required
          />
        </div>
        <div className="grid gap-2">
          <Label htmlFor="password">
            {t("Mot de passe (8 caractères min.)")}
          </Label>
          <Input
            id="password"
            type="password"
            autoComplete="new-password"
            minLength={8}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
          />
        </div>
        <div className="grid gap-2">
          <Label htmlFor="confirm">{t("Confirmation")}</Label>
          <Input
            id="confirm"
            type="password"
            autoComplete="new-password"
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            required
          />
        </div>
        {error && (
          <Alert variant="destructive">
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        )}
        <Button type="submit" size="lg" disabled={busy}>
          {busy && <Loader2 className="animate-spin" />} {t("Créer le compte")}
        </Button>
      </form>
    </AuthLayout>
  )
}
