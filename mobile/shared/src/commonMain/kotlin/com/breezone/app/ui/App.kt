package com.breezone.app.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.background
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.navigation.NavDestination
import androidx.navigation.NavDestination.Companion.hasRoute
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.toRoute
import com.breezone.app.data.CurrentInstallation
import com.breezone.app.data.DeviceView
import com.breezone.app.data.Hub
import com.breezone.app.data.ServerDevices
import com.breezone.app.data.installation
import com.breezone.app.data.modulesHere
import com.breezone.app.data.zoneRef
import com.breezone.app.resources.Res
import com.breezone.app.resources.tab_installations
import com.breezone.app.ui.home.AppPage
import com.breezone.app.ui.home.DeviceKind
import com.breezone.app.ui.home.HomeBoard
import com.breezone.app.ui.home.HomeNav
import com.breezone.app.ui.home.PageContext
import com.breezone.app.ui.home.deviceKind
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.screens.AddServerScreen
import com.breezone.app.ui.screens.ChargingScreen
import com.breezone.app.ui.screens.EnergyScreen
import com.breezone.app.ui.screens.LicensesScreen
import com.breezone.app.ui.screens.HomeScreen
import com.breezone.app.ui.screens.InstallationScreen
import com.breezone.app.ui.screens.LightScreen
import com.breezone.app.ui.screens.LightsScreen
import com.breezone.app.ui.screens.SettingsScreen
import com.breezone.app.ui.screens.ShutterScreen
import com.breezone.app.ui.screens.ShuttersScreen
import com.breezone.app.ui.screens.SolarScreen
import com.breezone.app.ui.screens.VentilationRoomScreen
import com.breezone.app.ui.screens.VentilationScreen
import com.breezone.app.ui.screens.ZoneScreen
import com.breezone.app.ui.theme.BreezoneTheme
import com.breezone.app.ui.theme.Fz
import kotlinx.serialization.Serializable
import org.jetbrains.compose.resources.StringResource
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject

/** The list of the installations, to choose the one the app shows. */
@Serializable object HomeRoute
@Serializable object SettingsRoute
@Serializable object LicensesRoute
@Serializable data class AddServerRoute(val reauthServerId: String? = null)
@Serializable data class HouseRoute(val serverId: String, val installationId: Long)
@Serializable data class InstallationRoute(val serverId: String, val installationId: Long)
@Serializable data class RoutinesRoute(val serverId: String, val installationId: Long)
@Serializable data class SchedulesRoute(val serverId: String, val installationId: Long)
@Serializable data class LightsRoute(val serverId: String)
@Serializable data class EnergyRoute(val serverId: String)
@Serializable data class SolarRoute(val serverId: String)
@Serializable data class ChargingRoute(val serverId: String)
@Serializable data class LightRoute(val serverId: String, val deviceId: Long)
@Serializable data class ShuttersRoute(val serverId: String)
@Serializable data class ShutterRoute(val serverId: String, val deviceId: Long)
@Serializable data class VentilationRoute(val serverId: String)
@Serializable data class VentilationRoomRoute(val serverId: String, val deviceId: Long)
@Serializable data class ZoneRoute(val serverId: String, val installationId: Long, val systemId: Long, val zoneId: Long)

/** The screen of a page of the bar, for the installation shown. */
private fun AppPage.route(c: CurrentInstallation): Any = when (this) {
    AppPage.Home -> HouseRoute(c.serverId, c.installationId)
    AppPage.Climate -> InstallationRoute(c.serverId, c.installationId)
    AppPage.Routines -> RoutinesRoute(c.serverId, c.installationId)
    AppPage.Schedules -> SchedulesRoute(c.serverId, c.installationId)
    AppPage.Energy -> EnergyRoute(c.serverId)
    AppPage.Solar -> SolarRoute(c.serverId)
    AppPage.Charging -> ChargingRoute(c.serverId)
    AppPage.Lights -> LightsRoute(c.serverId)
    AppPage.Shutters -> ShuttersRoute(c.serverId)
    AppPage.Ventilation -> VentilationRoute(c.serverId)
    AppPage.Settings -> SettingsRoute
}

/** An entry of the bottom bar. */
private data class BarItem(val route: Any, val label: StringResource, val icon: ImageVector)

@Composable
fun App() {
    BreezoneTheme {
        val hub = koinInject<Hub>()
        val servers by hub.servers.collectAsState()
        val devices by hub.devices.collectAsState()
        val current by hub.current.collectAsState()
        val nav = rememberNavController()
        val snackbar = remember { SnackbarHostState() }
        val backStack by nav.currentBackStackEntryAsState()
        val destination = backStack?.destination
        // Computed once: the saved servers are loaded synchronously by the Hub.
        val start: Any = remember {
            val c = hub.current.value
            when {
                servers.isEmpty() -> AddServerRoute()
                c != null && servers.any { it.account.id == c.serverId } -> HouseRoute(c.serverId, c.installationId)
                else -> HomeRoute
            }
        }

        // The pages of the installation shown; without one, its list and the settings.
        val items = current?.let { c ->
            val server = servers.firstOrNull { it.account.id == c.serverId }
            val inst = servers.installation(c.serverId, c.installationId)
            val ctx = PageContext(inst, devices[c.serverId] ?: ServerDevices(), server?.modulesHere(inst) ?: false)
            AppPage.of(ctx).map { BarItem(it.route(c), it.label, it.icon) }
        } ?: listOf(BarItem(HomeRoute, Res.string.tab_installations, FzIcons.House), BarItem(SettingsRoute, AppPage.Settings.label, AppPage.Settings.icon))
        val onRoot = items.any { destination?.hasRoute(it.route::class) == true } && !(current != null && destination?.hasRoute<HomeRoute>() == true)

        LaunchedEffect(Unit) { hub.messages.collect { snackbar.showSnackbar(it) } }
        LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { hub.refresh() }

        val choose: (String, Long) -> Unit = { serverId, id ->
            hub.select(serverId, id)
            nav.navigate(HouseRoute(serverId, id)) {
                popUpTo(0) { inclusive = true }
                launchSingleTop = true
            }
        }
        val openDevice: (String, DeviceView) -> Unit = { serverId, d ->
            val c = current
            when (d.deviceKind) {
                DeviceKind.Light -> nav.navigate(LightRoute(serverId, d.id))
                DeviceKind.Shutter, DeviceKind.Window, DeviceKind.Door -> nav.navigate(ShutterRoute(serverId, d.id))
                DeviceKind.RoomAir -> nav.navigate(VentilationRoomRoute(serverId, d.id))
                DeviceKind.Climate -> d.zoneRef()?.let { (sys, zone) -> d.installationId?.let { nav.navigate(ZoneRoute(serverId, it, sys, zone)) } }
                else -> d.deviceKind.page?.let { page -> if (c != null) switchTo(nav, page.route(c)) }
            }
        }

        Scaffold(
            snackbarHost = { SnackbarHost(snackbar) },
            bottomBar = { if (onRoot) PageBar(nav, destination, items) },
        ) { padding ->
            NavHost(
                navController = nav,
                startDestination = start,
                modifier = (if (onRoot) PaddingValues(bottom = padding.calculateBottomPadding()) else PaddingValues(0.dp))
                    .let { bar -> Modifier.padding(bar).consumeWindowInsets(bar) },
            ) {
                composable<HomeRoute> {
                    HomeScreen(
                        onOpenInstallation = choose,
                        onReauth = { nav.navigate(AddServerRoute(it)) },
                        onAddServer = { nav.navigate(AddServerRoute()) },
                        onBack = if (current != null && nav.previousBackStackEntry != null) ({ nav.popBackStack() }) else null,
                    )
                }
                composable<HouseRoute> { entry ->
                    val r = entry.toRoute<HouseRoute>()
                    HomeBoard(
                        serverId = r.serverId,
                        installationId = r.installationId,
                        nav = HomeNav(
                            openPage = { page -> current?.let { switchTo(nav, page.route(it)) } },
                            openDevice = { d -> openDevice(r.serverId, d) },
                            openZone = { z -> nav.navigate(ZoneRoute(r.serverId, z.installationId, z.systemId, z.zoneId)) },
                        ),
                        onInstallations = { nav.navigate(HomeRoute) },
                    )
                }
                composable<InstallationRoute> { entry ->
                    val r = entry.toRoute<InstallationRoute>()
                    InstallationScreen(r.serverId, r.installationId, section = 0, onBack = null) { z ->
                        nav.navigate(ZoneRoute(r.serverId, z.installationId, z.systemId, z.zoneId))
                    }
                }
                composable<RoutinesRoute> { entry ->
                    val r = entry.toRoute<RoutinesRoute>()
                    InstallationScreen(r.serverId, r.installationId, section = 1, onBack = null) {}
                }
                composable<SchedulesRoute> { entry ->
                    val r = entry.toRoute<SchedulesRoute>()
                    InstallationScreen(r.serverId, r.installationId, section = 2, onBack = null) {}
                }
                composable<EnergyRoute> { entry -> EnergyScreen(entry.toRoute<EnergyRoute>().serverId) }
                composable<SolarRoute> { entry -> SolarScreen(entry.toRoute<SolarRoute>().serverId) }
                composable<ChargingRoute> { entry -> ChargingScreen(entry.toRoute<ChargingRoute>().serverId) }
                composable<LightsRoute> { entry ->
                    val r = entry.toRoute<LightsRoute>()
                    LightsScreen(r.serverId, onBack = null) { nav.navigate(LightRoute(r.serverId, it)) }
                }
                composable<LightRoute> { entry ->
                    val r = entry.toRoute<LightRoute>()
                    LightScreen(r.serverId, r.deviceId, onBack = { nav.popBackStack() })
                }
                composable<ShuttersRoute> { entry ->
                    val r = entry.toRoute<ShuttersRoute>()
                    ShuttersScreen(r.serverId, onBack = null) { nav.navigate(ShutterRoute(r.serverId, it)) }
                }
                composable<ShutterRoute> { entry ->
                    val r = entry.toRoute<ShutterRoute>()
                    ShutterScreen(r.serverId, r.deviceId, onBack = { nav.popBackStack() })
                }
                composable<VentilationRoute> { entry ->
                    val r = entry.toRoute<VentilationRoute>()
                    VentilationScreen(r.serverId, onBack = null) { nav.navigate(VentilationRoomRoute(r.serverId, it)) }
                }
                composable<VentilationRoomRoute> { entry ->
                    val r = entry.toRoute<VentilationRoomRoute>()
                    VentilationRoomScreen(r.serverId, r.deviceId, onBack = { nav.popBackStack() })
                }
                composable<ZoneRoute> { entry ->
                    val r = entry.toRoute<ZoneRoute>()
                    ZoneScreen(r.serverId, r.installationId, r.systemId, r.zoneId, onBack = { nav.popBackStack() })
                }
                composable<SettingsRoute> {
                    SettingsScreen(
                        onAddServer = { nav.navigate(AddServerRoute()) },
                        onReauth = { nav.navigate(AddServerRoute(it)) },
                        onLicenses = { nav.navigate(LicensesRoute) },
                    )
                }
                composable<LicensesRoute> { LicensesScreen(onBack = { nav.popBackStack() }) }
                composable<AddServerRoute> { entry ->
                    val r = entry.toRoute<AddServerRoute>()
                    AddServerScreen(
                        reauthServerId = r.reauthServerId,
                        canGoBack = servers.isNotEmpty(),
                        onBack = { nav.popBackStack() },
                        onDone = {
                            nav.navigate(HomeRoute) {
                                popUpTo(0) { inclusive = true }
                                launchSingleTop = true
                            }
                        },
                    )
                }
            }
        }
    }
}

/**
 * Goes to a page of the bar: the home stays at the root, and each page keeps where it was (its
 * scroll, an open detail) when it is left and found again.
 */
private fun switchTo(nav: NavHostController, route: Any) {
    // The root is the home of the installation, or the list of the installations before one is chosen.
    val home = runCatching { nav.getBackStackEntry<HouseRoute>() }.isSuccess
    nav.navigate(route) {
        if (home) popUpTo<HouseRoute> { saveState = true } else popUpTo<HomeRoute> { saveState = true }
        launchSingleTop = true
        restoreState = true
    }
}

/**
 * The bottom bar: every page of the installation, in a row that scrolls sideways when they do not
 * all fit, the one shown highlighted.
 */
@Composable
private fun PageBar(nav: NavHostController, destination: NavDestination?, items: List<BarItem>) {
    val scroll = rememberScrollState()
    Surface(Modifier.fillMaxWidth(), color = MaterialTheme.colorScheme.surfaceContainer) {
        // As wide as the screen at least: the entries share it when they all fit, else scroll.
        BoxWithConstraints(Modifier.windowInsetsPadding(WindowInsets.navigationBars)) {
        Row(
            Modifier
                .horizontalScroll(scroll)
                .widthIn(min = maxWidth)
                .padding(horizontal = 8.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.SpaceAround,
        ) {
            items.forEach { item ->
                val selected = destination?.hasRoute(item.route::class) == true
                val color = if (selected) MaterialTheme.colorScheme.primary else Fz.colors.muted
                Column(
                    Modifier
                        .widthIn(min = 68.dp)
                        .clip(RoundedCornerShape(16.dp))
                        .clickable(role = Role.Tab) { if (!selected) switchTo(nav, item.route) }
                        .padding(horizontal = 8.dp, vertical = 4.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Row(
                        Modifier
                            .clip(RoundedCornerShape(16.dp))
                            .background(if (selected) MaterialTheme.colorScheme.primary.copy(alpha = 0.18f) else MaterialTheme.colorScheme.surfaceContainer)
                            .padding(horizontal = 16.dp, vertical = 4.dp),
                    ) {
                        Icon(item.icon, null, Modifier.size(22.dp), tint = color)
                    }
                    Text(stringResource(item.label), fontSize = 12.sp, color = color, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(top = 4.dp))
                }
            }
        }
        }
    }
}
