import { useEffect, useRef, type ReactNode } from "react"
import { Link, NavLink, useLocation, useNavigate } from "react-router"
import {
  Activity,
  CalendarClock,
  LayoutGrid,
  Blinds,
  Lightbulb,
  LogOut,
  Moon,
  PlugZap,
  Puzzle,
  Settings,
  SolarPanel,
  Smartphone,
  Sun,
  Timer,
  Zap,
} from "lucide-react"
import { useQuery, useQueryClient } from "@tanstack/react-query"
import type { LiveState } from "@/hooks/use-live"
import { ANDROID_APP_URL, api, currentUser } from "@/lib/api"
import { cn } from "@/lib/utils"
import { useTheme } from "@/components/theme-provider"
import { Brand, BrandMark } from "@/components/brand"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { t } from "@/lib/i18n"
import { useProfileLanguage } from "@/hooks/use-profile-language"

type NavItem = {
  to: string
  label: string
  icon: typeof Zap
  end: boolean
  /** Shown only when a module provides it, or to administrators. */
  when?: "energy" | "solar" | "charger" | "light" | "cover" | "admin"
}

/** The menu, in sections as in the mockups; a section without an item is not shown. */
const NAV: { title: string | null; items: NavItem[] }[] = [
  {
    title: null,
    items: [
      { to: "/", label: t("Installations"), icon: LayoutGrid, end: true },
    ],
  },
  {
    title: t("Maison"),
    items: [
      {
        to: "/energie",
        label: t("Énergie"),
        icon: Zap,
        end: false,
        when: "energy",
      },
      {
        to: "/lumieres",
        label: t("Lumières"),
        icon: Lightbulb,
        end: false,
        when: "light",
      },
      {
        to: "/volets",
        label: t("Volets"),
        icon: Blinds,
        end: false,
        when: "cover",
      },
      {
        to: "/solaire",
        label: t("Solaire"),
        icon: SolarPanel,
        end: false,
        when: "solar",
      },
      {
        to: "/recharge",
        label: t("Recharge"),
        icon: PlugZap,
        end: false,
        when: "charger",
      },
    ],
  },
  {
    title: t("Automatismes"),
    items: [
      {
        to: "/programmations",
        label: t("Programmations"),
        icon: CalendarClock,
        end: false,
      },
      { to: "/routines", label: t("Routines"), icon: Timer, end: false },
      { to: "/activity", label: t("Activité"), icon: Activity, end: false },
    ],
  },
  {
    title: t("Serveur"),
    items: [
      {
        to: "/settings?tab=modules",
        label: t("Modules"),
        icon: Puzzle,
        end: false,
        when: "admin",
      },
      { to: "/settings", label: t("Réglages"), icon: Settings, end: false },
    ],
  },
]

function LiveIndicator({ state }: { state: LiveState }) {
  const label =
    state === "live"
      ? t("Temps réel")
      : state === "connecting"
        ? t("Connexion…")
        : t("Reconnexion…")
  return (
    <span className="inline-flex items-center gap-2 text-xs text-muted-foreground">
      <span
        className={cn(
          "size-2 rounded-full",
          state === "live" ? "bg-success" : "animate-pulse bg-warning"
        )}
      />
      {label}
    </span>
  )
}

/** The energy page is shown once a measure has a role (Settings › Modules). */
function useEnergy() {
  const live = useQuery({
    queryKey: ["energy-live"],
    queryFn: api.energyLive,
    staleTime: 60_000,
  }).data
  return !!live && Object.values(live.roles).some(Boolean)
}

function useModules() {
  const devices = useQuery({
    queryKey: ["devices"],
    queryFn: api.devices,
    staleTime: 60_000,
  }).data
  return {
    charger: !!devices?.some((d) => d.traits.includes("ev.charger@1")),
    solar: !!devices?.some((d) => d.traits.includes("solar.inverter@1")),
    light: !!devices?.some((d) => d.traits.includes("light@1")),
    cover: !!devices?.some((d) => d.traits.includes("cover.position@1")),
  }
}

/** A newer Breezone, for administrators (the server checks once a day). */
function useAvailableUpdate(admin: boolean) {
  return useQuery({
    queryKey: ["system-update"],
    queryFn: api.update,
    enabled: admin,
    staleTime: 3_600_000,
  }).data
}

/**
 * Version of the Breezone server. The web UI comes with the server: when the server is updated
 * (from Settings › Breezone, or a new image), the page reloads to get the matching interface.
 */
function useServerVersion() {
  const version = useQuery({
    queryKey: ["server-version"],
    queryFn: api.version,
    refetchInterval: 60_000,
  }).data?.version
  const loaded = useRef<string | undefined>(undefined)
  useEffect(() => {
    if (!version) return
    if (loaded.current === undefined) loaded.current = version
    else if (version !== loaded.current) window.location.reload()
  }, [version])
  return version
}

export function AppShell({
  live,
  children,
}: {
  live: LiveState
  children: ReactNode
}) {
  const { theme, setTheme } = useTheme()
  const user = currentUser()
  const navigate = useNavigate()
  const qc = useQueryClient()
  const version = useServerVersion()
  const update = useAvailableUpdate(user?.role === "admin")
  useProfileLanguage()
  const energy = useEnergy()
  const modules = useModules()
  const location = useLocation()
  const shown = (n: NavItem) =>
    n.when === undefined ||
    (n.when === "energy" && energy) ||
    (n.when === "solar" && modules.solar) ||
    (n.when === "light" && modules.light) ||
    (n.when === "cover" && modules.cover) ||
    (n.when === "charger" && modules.charger) ||
    (n.when === "admin" && user?.role === "admin")
  const sections = NAV.map((s) => ({
    ...s,
    items: s.items.filter(shown),
  })).filter((s) => s.items.length > 0)
  // Modules is a tab of the settings: each of the two entries is active on its own.
  const onModules =
    location.pathname === "/settings" &&
    new URLSearchParams(location.search).get("tab") === "modules"
  const active = (n: NavItem, isActive: boolean) =>
    n.to.startsWith("/settings")
      ? isActive && (n.to.includes("tab=modules") ? onModules : !onModules)
      : isActive
  // The bottom bar of a phone: everything but Modules (reached from the settings).
  const nav = sections
    .flatMap((s) => s.items)
    .filter((n) => !n.to.includes("tab=modules"))
  const dark =
    theme === "dark" ||
    (theme === "system" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches)

  const logout = async () => {
    await api.logout()
    qc.clear()
    navigate("/login")
  }

  return (
    <div className="fz-backdrop min-h-svh">
      <aside className="fixed inset-y-0 left-0 hidden w-64 flex-col border-r bg-sidebar/70 p-4 backdrop-blur-xl md:flex">
        <div className="px-2 py-3">
          <Brand />
        </div>
        <nav className="mt-6 flex flex-col gap-1">
          {sections.map((section, i) => (
            <div key={section.title ?? i} className="flex flex-col gap-1">
              {section.title && (
                <div className="mt-4 px-3 pb-1 text-[11px] font-medium tracking-wide text-muted-foreground/80 uppercase">
                  {section.title}
                </div>
              )}
              {section.items.map((n) => (
                <NavLink
                  key={n.to}
                  to={n.to}
                  end={n.end}
                  className={({ isActive }) =>
                    cn(
                      "flex items-center gap-3 rounded-xl px-3 py-2.5 text-sm font-medium text-muted-foreground transition-colors hover:bg-sidebar-accent hover:text-foreground",
                      active(n, isActive) && "bg-sidebar-accent text-foreground"
                    )
                  }
                >
                  <n.icon className="size-4.5" />
                  {n.label}
                </NavLink>
              ))}
            </div>
          ))}
        </nav>
        <div className="mt-auto flex flex-col gap-1.5 px-3">
          <LiveIndicator state={live} />
          {version && (
            <span className="text-xs text-muted-foreground/70">
              Breezone {version}
            </span>
          )}
          {update?.available && update.latest && (
            <Link
              to="/settings?tab=breezone"
              className="text-xs font-medium text-primary hover:underline"
            >
              {t("Breezone {version} disponible", { version: update.latest })}
            </Link>
          )}
        </div>
      </aside>

      <div className="md:pl-64">
        <header className="sticky top-0 z-30 flex h-16 items-center gap-3 border-b bg-background/70 px-4 backdrop-blur-xl md:px-8">
          <div className="md:hidden">
            <BrandMark className="size-8" />
          </div>
          <div className="md:hidden">
            <LiveIndicator state={live} />
          </div>
          <div className="ml-auto flex items-center gap-1">
            <Button
              variant="ghost"
              size="icon"
              onClick={() => setTheme(dark ? "light" : "dark")}
              aria-label={t("Changer de thème")}
            >
              {dark ? <Sun /> : <Moon />}
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" className="gap-2 rounded-full px-2">
                  <span className="inline-flex size-8 items-center justify-center rounded-full bg-primary/15 text-sm font-semibold text-primary uppercase">
                    {user?.username.slice(0, 1) ?? "?"}
                  </span>
                  <span className="hidden text-sm sm:inline">
                    {user?.username}
                  </span>
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-52">
                <DropdownMenuLabel>
                  {user?.username}
                  <div className="text-xs font-normal text-muted-foreground">
                    {user?.role === "admin"
                      ? t("Administrateur")
                      : t("Utilisateur")}
                  </div>
                </DropdownMenuLabel>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  onClick={() => navigate("/settings?tab=account")}
                >
                  <Settings /> {t("Mon compte")}
                </DropdownMenuItem>
                <DropdownMenuItem onClick={logout} variant="destructive">
                  <LogOut /> {t("Se déconnecter")}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem asChild>
                  <a href={ANDROID_APP_URL} target="_blank" rel="noopener">
                    <Smartphone /> {t("Application Android")}
                  </a>
                </DropdownMenuItem>
                {version && (
                  <>
                    <DropdownMenuSeparator />
                    <div className="px-2 py-1.5 text-xs text-muted-foreground">
                      {t("Serveur Breezone {version}", { version })}
                    </div>
                  </>
                )}
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </header>
        <main className="mx-auto max-w-7xl px-4 pt-6 pb-28 md:px-8 md:pb-12">
          {children}
        </main>
      </div>

      <nav className="fixed inset-x-3 bottom-3 z-40 flex overflow-x-auto rounded-3xl border bg-card/80 p-1.5 shadow-2xl backdrop-blur-xl md:hidden">
        {nav.map(({ to, label, icon: Icon, end }) => (
          <NavLink
            key={to}
            to={to}
            end={end}
            className={({ isActive }) =>
              cn(
                "flex min-w-16 flex-1 flex-col items-center gap-1 rounded-2xl py-2 text-[11px] text-muted-foreground",
                isActive && "bg-primary/12 text-primary"
              )
            }
          >
            <Icon className="size-5" />
            <span className="max-w-full truncate px-0.5">{label}</span>
          </NavLink>
        ))}
      </nav>
    </div>
  )
}

export function PageHeader({
  title,
  description,
  actions,
}: {
  title: ReactNode
  description?: ReactNode
  actions?: ReactNode
}) {
  return (
    <div className="mb-6 flex flex-wrap items-end justify-between gap-4">
      <div className="min-w-0">
        <h1 className="font-heading text-2xl font-semibold tracking-tight md:text-3xl">
          {title}
        </h1>
        {description && (
          <div className="mt-1 text-sm text-muted-foreground">
            {description}
          </div>
        )}
      </div>
      {actions && (
        <div className="flex flex-wrap items-center gap-2">{actions}</div>
      )}
    </div>
  )
}
