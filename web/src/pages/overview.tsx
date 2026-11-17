import { useMemo, useState } from "react"
import { t, tn } from "@/lib/i18n"
import { PageHeader } from "@/components/app-shell"
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

  if (!home.installation || !home.data)
    return (
      <PageHeader
        title={t("Vue d'ensemble")}
        description={t(
          "Ajoutez une installation pour composer sa vue d'ensemble."
        )}
      />
    )

  const { installation } = home
  return (
    <Board
      page="overview"
      title={installation.name}
      description={[
        installation.location,
        tn(home.data.rooms.length, "{n} pièce", "{n} pièces"),
        tn(mine.length, "{n} appareil", "{n} appareils"),
      ]}
      data={home.data}
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
