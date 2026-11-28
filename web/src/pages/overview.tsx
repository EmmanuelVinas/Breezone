import { useMemo, useState } from "react"
import { t, tn } from "@/lib/i18n"
import { Board } from "@/components/overview/board"
import { useHomeData } from "@/components/overview/home-data"
import {
  generate,
  measuredFlows,
  worthShowing,
} from "@/components/overview/model"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

export function OverviewPage() {
  const home = useHomeData()
  const [by, setBy] = useState<"room" | "type">("room")
  const { mine, roles, surplusHere, priced, scenes } = home
  const generated = useMemo(
    () =>
      generate(by, {
        devices: mine,
        rooms: home.data?.rooms ?? [],
        roles: roles ?? {},
        surplus: surplusHere,
        scenes,
        priced,
      }).blocks,
    [by, mine, home.data?.rooms, roles, surplusHere, scenes, priced]
  )

  // Without an installation (one shared by another server only, say), the home of the modules.
  const { installation, page } = home
  return (
    <Board
      page="overview"
      title={installation?.name ?? t("Maison")}
      description={[
        installation?.location,
        tn(page.rooms.length, "{n} pièce", "{n} pièces"),
        tn(mine.length, "{n} appareil", "{n} appareils"),
      ]}
      data={page}
      generated={generated}
      loading={home.loading}
      // The blocks with nothing to show here are left out.
      shows={(b) =>
        home.energyLoading ||
        ((b.type !== "price" || priced) &&
          worthShowing(b, {
            flows: measuredFlows(roles),
            surplus: surplusHere,
          }))
      }
      actions={(source) =>
        source === "generated" && (
          <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            value={by}
            onValueChange={(v) => v && setBy(v as "room" | "type")}
          >
            <ToggleGroupItem value="room">{t("Par pièce")}</ToggleGroupItem>
            <ToggleGroupItem value="type">{t("Par type")}</ToggleGroupItem>
          </ToggleGroup>
        )
      }
    />
  )
}
