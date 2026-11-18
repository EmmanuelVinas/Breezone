import { useEffect, useMemo } from "react"
import { useQuery } from "@tanstack/react-query"
import { BellRing, Cctv, ImageOff, RefreshCw } from "lucide-react"
import type { DeviceView } from "@/lib/api"
import { ApiError, api, devicePicture } from "@/lib/api"
import { relativeTime } from "@/lib/format"
import { t } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import { PageHeader } from "@/components/app-shell"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"

const isCamera = (d: DeviceView) => d.traits.includes("camera@1")
const num = (v: unknown) => (typeof v === "number" ? v : null)

/** The picture of the last event of a camera, fetched again when a newer one is told. */
function usePicture(device: DeviceView) {
  const imageAt = num(device.state?.image_at)
  const picture = useQuery({
    queryKey: ["picture", device.id, imageAt],
    queryFn: () => devicePicture(device.id),
    enabled: imageAt !== null,
    staleTime: Infinity,
    retry: false,
  })
  const url = useMemo(
    () => (picture.data ? URL.createObjectURL(picture.data) : null),
    [picture.data]
  )
  useEffect(() => () => void (url && URL.revokeObjectURL(url)), [url])
  return { url, picture, imageAt }
}

function CameraCard({ device }: { device: DeviceView }) {
  const s = device.state ?? {}
  const { url, picture, imageAt } = usePicture(device)
  const doorbell = device.kind === "doorbell"
  const online = s.online === true
  const lastAt = num(s.last_event_at)
  const rang = num(s.last_at)
  const missing =
    picture.error instanceof ApiError && picture.error.status === 409
      ? picture.error.message
      : picture.error
        ? t("L'image n'a pas pu être lue.")
        : imageAt === null
          ? t("Pas encore d'image aujourd'hui.")
          : null
  return (
    <Card className="overflow-hidden rounded-3xl py-0">
      <div className="relative aspect-video bg-muted">
        {url ? (
          <img
            src={url}
            alt={t("Dernière image de « {name} »", { name: device.name })}
            className="size-full object-cover"
          />
        ) : picture.isLoading ? (
          <Skeleton className="size-full rounded-none" />
        ) : (
          <div className="grid size-full place-items-center p-6 text-center text-sm text-muted-foreground">
            <div className="grid justify-items-center gap-2">
              <ImageOff className="size-6" />
              {missing}
            </div>
          </div>
        )}
        {s.motion === true && (
          <Badge className="absolute top-3 left-3 bg-warning text-white">
            {t("Mouvement")}
          </Badge>
        )}
      </div>
      <CardContent className="grid gap-2 pb-5">
        <div className="flex items-center gap-2">
          {doorbell ? (
            <BellRing className="size-4 text-primary" />
          ) : (
            <Cctv className="size-4 text-primary" />
          )}
          <h3 className="min-w-0 flex-1 truncate font-heading text-lg font-semibold">
            {device.name}
          </h3>
          <Badge variant="outline" className="gap-1.5">
            <span
              className={cn(
                "size-2 rounded-full",
                online ? "bg-success" : "bg-muted-foreground"
              )}
            />
            {online ? t("En ligne") : t("Hors ligne")}
          </Badge>
        </div>
        <div className="text-sm text-muted-foreground">
          {typeof s.last_event === "string" && lastAt
            ? t("{event}, {when}", {
                event: s.last_event,
                when: relativeTime(lastAt),
              })
            : t("Aucun événement aujourd'hui")}
        </div>
        {doorbell && (
          <div className="text-sm text-muted-foreground">
            {rang
              ? t("Dernier coup de sonnette {when}", {
                  when: relativeTime(rang),
                })
              : t("Pas de coup de sonnette depuis le démarrage du serveur")}
          </div>
        )}
      </CardContent>
    </Card>
  )
}

/** The cameras and doorbells: the picture of their last event, and what they saw. */
export function CamerasPage() {
  const devices = useQuery({
    queryKey: ["devices"],
    queryFn: api.devices,
    refetchInterval: 5_000,
  })
  const all = (devices.data ?? [])
    .filter(isCamera)
    .sort(
      (a, b) =>
        Number(b.kind === "doorbell") - Number(a.kind === "doorbell") ||
        a.name.localeCompare(b.name)
    )
  return (
    <>
      <PageHeader
        title={t("Caméras")}
        description={t(
          "La dernière image de chaque caméra et sonnette, et leur dernier événement. Le direct viendra plus tard."
        )}
        actions={
          <Button
            variant="outline"
            size="sm"
            onClick={() => devices.refetch()}
            disabled={devices.isFetching}
          >
            <RefreshCw className={devices.isFetching ? "animate-spin" : ""} />
            {t("Actualiser")}
          </Button>
        }
      />
      {devices.isLoading ? (
        <Skeleton className="h-64 rounded-3xl" />
      ) : all.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          {t(
            "Aucune caméra : connectez un compte EZVIZ dans Réglages › Comptes et services."
          )}
        </p>
      ) : (
        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          {all.map((d) => (
            <CameraCard key={d.id} device={d} />
          ))}
        </div>
      )}
    </>
  )
}
