import { useRef, useState } from "react"
import { useQuery } from "@tanstack/react-query"
import { QRCodeCanvas } from "qrcode.react"
import { Copy, Download, Share2, TriangleAlert } from "lucide-react"
import { toast } from "sonner"
import { api } from "@/lib/api"
import { appOrigin } from "@/lib/base"
import { t } from "@/lib/i18n"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

/** The link the app opens: the server and the user, never a password. */
export function connectLink(server: string, username: string) {
  const url = server.trim().replace(/\/+$/, "")
  return `breezone://connect?url=${encodeURIComponent(url)}&username=${encodeURIComponent(username)}`
}

/** An address only this computer reaches. */
function localOnly(server: string) {
  try {
    const host = new URL(server).hostname
    return host === "localhost" || host.startsWith("127.") || host === "[::1]"
  } catch {
    return true
  }
}

/**
 * The QR code that opens the app on this server with the user filled in: scanned with the
 * phone's camera, or shared as an image or a link.
 */
export function ConnectPhoneDialog({
  username,
  open,
  onOpenChange,
}: {
  username: string
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const identity = useQuery({
    queryKey: ["node-identity"],
    queryFn: api.identity,
    enabled: open,
  })
  const [edited, setEdited] = useState<string | null>(null)
  const server = edited ?? identity.data?.public_url ?? appOrigin()
  const valid = /^https?:\/\/[^/\s]+/.test(server.trim())
  const link = connectLink(server, username)
  const canvas = useRef<HTMLCanvasElement>(null)

  const image = () =>
    new Promise<Blob | null>((resolve) =>
      canvas.current
        ? canvas.current.toBlob(resolve, "image/png")
        : resolve(null)
    )
  const fileName = `breezone-${username}.png`
  const download = async () => {
    const blob = await image()
    if (!blob) return
    const a = document.createElement("a")
    a.href = URL.createObjectURL(blob)
    a.download = fileName
    a.click()
    URL.revokeObjectURL(a.href)
  }
  const share = async () => {
    const blob = await image()
    const files = blob
      ? [new File([blob], fileName, { type: "image/png" })]
      : []
    const text = t(
      "Scannez ce QR code avec l'appareil photo du téléphone pour ouvrir Breezone sur le serveur {server} avec l'identifiant {name}.",
      { server: server.trim(), name: username }
    )
    try {
      await navigator.share(
        files.length && navigator.canShare?.({ files })
          ? { title: "Breezone", text, files }
          : { title: "Breezone", text: `${text}\n${link}` }
      )
    } catch (e) {
      if ((e as Error).name !== "AbortError")
        toast.error(t("Partage impossible"))
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) setEdited(null)
        onOpenChange(o)
      }}
    >
      <DialogContent className="max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>
            {t("Connecter un téléphone pour {name}", { name: username })}
          </DialogTitle>
          <DialogDescription>
            {t(
              "Scannez ce QR code avec l'appareil photo du téléphone : l'application Breezone s'ouvre avec le serveur et l'identifiant remplis. Il ne reste qu'à saisir le mot de passe, qui n'est pas dans le QR code."
            )}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-2">
          <Label htmlFor="connect-server">{t("Adresse du serveur")}</Label>
          <Input
            id="connect-server"
            value={server}
            onChange={(e) => setEdited(e.target.value)}
            placeholder="https://breezone.example.org"
          />
          <p className="text-xs text-muted-foreground">
            {t(
              "L'adresse que le téléphone joint : celle du réseau local à la maison, ou l'adresse publique pour l'utiliser de partout."
            )}
          </p>
        </div>
        {valid && localOnly(server) && (
          <Alert>
            <TriangleAlert />
            <AlertDescription>
              {t(
                "Cette adresse ne marche que sur cet ordinateur : mettez celle que le téléphone joint (par exemple http://192.168.1.20:8427)."
              )}
            </AlertDescription>
          </Alert>
        )}
        {valid ? (
          <div className="flex justify-center rounded-2xl bg-white p-4">
            <QRCodeCanvas
              ref={canvas}
              value={link}
              size={240}
              marginSize={2}
              aria-label={t("QR code de connexion")}
            />
          </div>
        ) : (
          <p className="text-sm text-destructive">
            {t("Adresse attendue : http://… ou https://…")}
          </p>
        )}
        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            disabled={!valid}
            onClick={() =>
              navigator.clipboard
                .writeText(link)
                .then(() => toast.success(t("Lien copié")))
            }
          >
            <Copy /> {t("Copier le lien")}
          </Button>
          <Button variant="outline" disabled={!valid} onClick={download}>
            <Download /> {t("Télécharger l'image")}
          </Button>
          {"share" in navigator && (
            <Button variant="outline" disabled={!valid} onClick={share}>
              <Share2 /> {t("Partager")}
            </Button>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}
