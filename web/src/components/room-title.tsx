import type { Room } from "@/lib/api"
import { t } from "@/lib/i18n"
import { RoomIcon } from "@/lib/room-icons"

/** The title of a room's group on a page: its icon and its name. */
export function RoomTitle({ room }: { room: Room | null }) {
  return (
    <h2 className="flex items-center gap-2 font-heading text-lg font-semibold">
      {room && (
        <RoomIcon
          room={room}
          className="size-5 text-muted-foreground"
          aria-hidden
        />
      )}
      {room?.name ?? t("Sans pièce")}
    </h2>
  )
}
