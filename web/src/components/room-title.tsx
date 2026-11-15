import type { Room } from "@/lib/api"
import { t } from "@/lib/i18n"
import { roomIcon } from "@/lib/room-icons"

/** The title of a room's group on a page: its icon and its name. */
export function RoomTitle({ room }: { room: Room | null }) {
  const Icon = room ? roomIcon(room) : null
  return (
    <h2 className="flex items-center gap-2 font-heading text-lg font-semibold">
      {Icon && <Icon className="size-5 text-muted-foreground" aria-hidden />}
      {room?.name ?? t("Sans pièce")}
    </h2>
  )
}
