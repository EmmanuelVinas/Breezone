import type { ReactNode } from "react"
import { Link } from "react-router"
import { Server } from "lucide-react"
import { t } from "@/lib/i18n"
import {
  useChosenInstallation,
  useModulesHere,
} from "@/lib/selected-installation"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"

/** A page of this server's modules: when the installation chosen is another server's, it says so. */
export function ThisServerOnly({ children }: { children: ReactNode }) {
  const here = useModulesHere()
  const chosen = useChosenInstallation()
  if (here) return children
  return (
    <Card className="mt-6 rounded-3xl">
      <CardContent className="flex flex-col items-center gap-3 py-12 text-center">
        <span className="grid size-12 place-items-center rounded-2xl bg-primary/15 text-primary">
          <Server className="size-6" />
        </span>
        <div className="font-heading text-lg font-semibold">
          {t("« {name} » est reliée par un autre serveur", {
            name: chosen?.name ?? "",
          })}
        </div>
        <p className="max-w-md text-sm text-muted-foreground">
          {t(
            "Les lumières, volets, énergie et autres modules de cette page sont ceux de ce serveur Breezone. Choisissez une de ses installations pour les voir ; ceux de « {name} » se gèrent sur son propre serveur.",
            { name: chosen?.name ?? "" }
          )}
        </p>
        <Button asChild>
          <Link to="/">{t("Mes installations")}</Link>
        </Button>
      </CardContent>
    </Card>
  )
}
