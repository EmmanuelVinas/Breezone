import { useState } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Plus } from "lucide-react"
import { toast } from "sonner"
import type { Room } from "@/lib/api"
import { api } from "@/lib/api"
import { t } from "@/lib/i18n"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e))
const NEW = "new"

/** The room of a device, with a new room created on the spot. */
export function RoomSelect({
  deviceId,
  roomId,
  rooms,
  className,
}: {
  deviceId: number
  roomId: number | null
  rooms: Room[]
  className?: string
}) {
  const qc = useQueryClient()
  const [naming, setNaming] = useState(false)
  const [name, setName] = useState("")
  const place = useMutation({
    mutationFn: (room_id: number | null) =>
      api.updateDevice(deviceId, { room_id }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["devices"] }),
    onError: (e) => toast.error(errorText(e)),
  })
  const create = useMutation({
    mutationFn: (name: string) =>
      api.createRoom({ name, position: rooms.length }),
    onSuccess: (room) => {
      qc.invalidateQueries({ queryKey: ["rooms"] })
      place.mutate(room.id)
      setNaming(false)
      setName("")
    },
    onError: (e) => toast.error(errorText(e)),
  })
  return (
    <>
      <Select
        value={roomId ? String(roomId) : "none"}
        onValueChange={(v) =>
          v === NEW
            ? setNaming(true)
            : place.mutate(v === "none" ? null : Number(v))
        }
      >
        <SelectTrigger className={className} aria-label={t("Pièce")}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="none">{t("Sans pièce")}</SelectItem>
          {rooms.map((r) => (
            <SelectItem key={r.id} value={String(r.id)}>
              {r.name}
            </SelectItem>
          ))}
          <SelectSeparator />
          <SelectItem value={NEW}>
            <Plus /> {t("Nouvelle pièce…")}
          </SelectItem>
        </SelectContent>
      </Select>
      <Dialog open={naming} onOpenChange={setNaming}>
        <DialogContent>
          <form
            className="grid gap-4"
            onSubmit={(e) => {
              e.preventDefault()
              if (name.trim()) create.mutate(name.trim())
            }}
          >
            <DialogHeader>
              <DialogTitle>{t("Nouvelle pièce")}</DialogTitle>
              <DialogDescription>
                {t("L'appareil y est rangé dès sa création.")}
              </DialogDescription>
            </DialogHeader>
            <Input
              autoFocus
              value={name}
              maxLength={60}
              placeholder={t("Salon, Chambre parents…")}
              aria-label={t("Nom de la pièce")}
              onChange={(e) => setName(e.target.value)}
            />
            <DialogFooter>
              <Button
                type="button"
                variant="ghost"
                onClick={() => setNaming(false)}
              >
                {t("Annuler")}
              </Button>
              <Button type="submit" disabled={!name.trim() || create.isPending}>
                {t("Créer")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  )
}
