import { Suspense, lazy, useEffect, useSyncExternalStore } from "react"
import {
  BrowserRouter,
  Navigate,
  Route,
  Routes,
  useLocation,
} from "react-router"
import { useQuery } from "@tanstack/react-query"
import { api, isSignedIn, onAuthChange } from "@/lib/api"
import { useLive } from "@/hooks/use-live"
import { AppShell } from "@/components/app-shell"
import { LoginPage, SetupPage } from "@/pages/auth"
import { DashboardPage } from "@/pages/dashboard"

// Secondary pages are loaded on demand (the charts library is heavy).
const ZonePage = lazy(() =>
  import("@/pages/zone").then((m) => ({ default: m.ZonePage }))
)
const RoutinesPage = lazy(() =>
  import("@/pages/routines").then((m) => ({ default: m.RoutinesPage }))
)
const SchedulesPage = lazy(() =>
  import("@/pages/routines").then((m) => ({ default: m.SchedulesPage }))
)
const ActivityPage = lazy(() =>
  import("@/pages/activity").then((m) => ({ default: m.ActivityPage }))
)
const SettingsPage = lazy(() =>
  import("@/pages/settings").then((m) => ({ default: m.SettingsPage }))
)

function useSignedIn() {
  return useSyncExternalStore((cb) => {
    const off = onAuthChange(cb)
    return () => {
      off()
    }
  }, isSignedIn)
}

function Protected() {
  const signedIn = useSignedIn()
  const live = useLive(signedIn)
  const location = useLocation()
  if (!signedIn)
    return <Navigate to="/login" replace state={{ from: location.pathname }} />
  return (
    <AppShell live={live}>
      <Suspense fallback={null}>
        <Routes>
          <Route path="/" element={<DashboardPage />} />
          <Route path="/installations/:id" element={<DashboardPage />} />
          <Route path="/zones/:inst/:sys/:zone" element={<ZonePage />} />
          <Route path="/programmations" element={<SchedulesPage />} />
          <Route path="/routines" element={<RoutinesPage />} />
          <Route path="/activity" element={<ActivityPage />} />
          <Route path="/settings" element={<SettingsPage />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </Suspense>
    </AppShell>
  )
}

function Gate() {
  const setup = useQuery({
    queryKey: ["setup"],
    queryFn: api.setupStatus,
    staleTime: Infinity,
  })
  const signedIn = useSignedIn()
  useEffect(() => {
    document.documentElement.lang = "fr"
  }, [])
  if (setup.isLoading) return null
  if (setup.data?.needs_setup) {
    return (
      <Routes>
        <Route path="*" element={<SetupPage />} />
      </Routes>
    )
  }
  return (
    <Routes>
      <Route
        path="/login"
        element={signedIn ? <Navigate to="/" replace /> : <LoginPage />}
      />
      <Route path="/*" element={<Protected />} />
    </Routes>
  )
}

export default function App() {
  return (
    <BrowserRouter>
      <Gate />
    </BrowserRouter>
  )
}
