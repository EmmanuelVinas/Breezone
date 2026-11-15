package com.breezone.app.ui

import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.navigation.NavBackStackEntry
import androidx.navigation.NavDestination
import androidx.navigation.NavDestination.Companion.hasRoute
import androidx.navigation.NavHostController
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.toRoute
import com.breezone.app.data.Hub
import com.breezone.app.data.installation
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.screens.AddServerScreen
import com.breezone.app.ui.screens.fzNavigationColors
import com.breezone.app.ui.screens.HomeScreen
import com.breezone.app.ui.screens.INSTALLATION_SECTIONS
import com.breezone.app.ui.screens.InstallationScreen
import com.breezone.app.ui.screens.HouseScreen
import com.breezone.app.ui.screens.EnergyScreen
import com.breezone.app.ui.screens.SolarScreen
import com.breezone.app.ui.screens.ChargingScreen
import com.breezone.app.ui.screens.HouseType
import com.breezone.app.ui.screens.LightScreen
import com.breezone.app.ui.screens.LightsScreen
import com.breezone.app.ui.screens.ShutterScreen
import com.breezone.app.ui.screens.ShuttersScreen
import com.breezone.app.ui.screens.VentilationRoomScreen
import com.breezone.app.ui.screens.VentilationScreen
import com.breezone.app.ui.screens.types
import com.breezone.app.data.modulesHere
import com.breezone.app.ui.screens.SettingsScreen
import com.breezone.app.ui.screens.ZoneScreen
import com.breezone.app.ui.theme.BreezoneTheme
import com.breezone.app.resources.Res
import com.breezone.app.resources.tab_installations
import com.breezone.app.resources.tab_settings
import kotlinx.serialization.Serializable
import org.jetbrains.compose.resources.StringResource
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject

@Serializable object HomeRoute
@Serializable object SettingsRoute
@Serializable data class AddServerRoute(val reauthServerId: String? = null)
@Serializable data class InstallationRoute(val serverId: String, val installationId: Long)
@Serializable data class HouseRoute(val serverId: String, val installationId: Long)
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

private data class Tab(val route: Any, val label: StringResource, val icon: ImageVector)

private val TABS = listOf(
    Tab(HomeRoute, Res.string.tab_installations, FzIcons.House),
    Tab(SettingsRoute, Res.string.tab_settings, FzIcons.Settings),
)

@Composable
fun App() {
    BreezoneTheme {
        val hub = koinInject<Hub>()
        val servers by hub.servers.collectAsState()
        val devices by hub.devices.collectAsState()
        val nav = rememberNavController()
        val snackbar = remember { SnackbarHostState() }
        val backStack by nav.currentBackStackEntryAsState()
        val destination = backStack?.destination
        val showTabs = TABS.any { tab -> destination?.hasRoute(tab.route::class) == true }
        // The section bar of an installation lives here too, not in the screen: during the
        // navigation animation, only the content slides and the bottom bar switches in place.
        val installationEntry = backStack?.takeIf { destination?.hasRoute<InstallationRoute>() == true }
        val showSections = installationEntry != null && installationEntry.toRoute<InstallationRoute>()
            .let { servers.installation(it.serverId, it.installationId) } != null
        // Computed once: the saved servers are loaded synchronously by the Hub.
        val start = remember { if (servers.isEmpty()) AddServerRoute() else HomeRoute }

        LaunchedEffect(Unit) { hub.messages.collect { snackbar.showSnackbar(it) } }
        LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { hub.refresh() }

        Scaffold(
            snackbarHost = { SnackbarHost(snackbar) },
            bottomBar = {
                when {
                    showTabs -> TabBar(nav, destination)
                    showSections -> SectionBar(installationEntry!!)
                }
            },
        ) { padding ->
            NavHost(
                navController = nav,
                startDestination = start,
                modifier = (if (showTabs || showSections) PaddingValues(bottom = padding.calculateBottomPadding()) else PaddingValues(0.dp))
                    .let { bar -> Modifier.padding(bar).consumeWindowInsets(bar) },
            ) {
                composable<HomeRoute> {
                    HomeScreen(
                        onOpenInstallation = { serverId, id ->
                            // The house screen when this server's modules go with the installation;
                            // otherwise straight to its climate, as before.
                            val server = servers.firstOrNull { it.account.id == serverId }
                            val withModules = server != null && server.modulesHere(servers.installation(serverId, id)) &&
                                devices[serverId]?.types().orEmpty().isNotEmpty()
                            nav.navigate(if (withModules) HouseRoute(serverId, id) else InstallationRoute(serverId, id))
                        },
                        onReauth = { nav.navigate(AddServerRoute(it)) },
                        onAddServer = { nav.navigate(AddServerRoute()) },
                    )
                }
                composable<InstallationRoute> { entry ->
                    val r = entry.toRoute<InstallationRoute>()
                    val section by entry.savedStateHandle.getStateFlow(SECTION_KEY, 0).collectAsState()
                    InstallationScreen(
                        serverId = r.serverId,
                        installationId = r.installationId,
                        section = section,
                        onBack = { nav.popBackStack() },
                        onOpenZone = { z -> nav.navigate(ZoneRoute(r.serverId, z.installationId, z.systemId, z.zoneId)) },
                    )
                }
                composable<HouseRoute> { entry ->
                    val r = entry.toRoute<HouseRoute>()
                    HouseScreen(r.serverId, r.installationId, onBack = { nav.popBackStack() }) { type ->
                        nav.navigate(
                            when (type) {
                                HouseType.Climate -> InstallationRoute(r.serverId, r.installationId)
                                HouseType.Energy -> EnergyRoute(r.serverId)
                                HouseType.Solar -> SolarRoute(r.serverId)
                                HouseType.Charging -> ChargingRoute(r.serverId)
                                HouseType.Lights -> LightsRoute(r.serverId)
                                HouseType.Shutters -> ShuttersRoute(r.serverId)
                                HouseType.Ventilation -> VentilationRoute(r.serverId)
                            },
                        )
                    }
                }
                composable<EnergyRoute> { entry -> EnergyScreen(entry.toRoute<EnergyRoute>().serverId, onBack = { nav.popBackStack() }) }
                composable<SolarRoute> { entry -> SolarScreen(entry.toRoute<SolarRoute>().serverId, onBack = { nav.popBackStack() }) }
                composable<ChargingRoute> { entry -> ChargingScreen(entry.toRoute<ChargingRoute>().serverId, onBack = { nav.popBackStack() }) }
                composable<LightsRoute> { entry ->
                    val r = entry.toRoute<LightsRoute>()
                    LightsScreen(r.serverId, onBack = { nav.popBackStack() }) { nav.navigate(LightRoute(r.serverId, it)) }
                }
                composable<LightRoute> { entry ->
                    val r = entry.toRoute<LightRoute>()
                    LightScreen(r.serverId, r.deviceId, onBack = { nav.popBackStack() })
                }
                composable<ShuttersRoute> { entry ->
                    val r = entry.toRoute<ShuttersRoute>()
                    ShuttersScreen(r.serverId, onBack = { nav.popBackStack() }) { nav.navigate(ShutterRoute(r.serverId, it)) }
                }
                composable<ShutterRoute> { entry ->
                    val r = entry.toRoute<ShutterRoute>()
                    ShutterScreen(r.serverId, r.deviceId, onBack = { nav.popBackStack() })
                }
                composable<VentilationRoute> { entry ->
                    val r = entry.toRoute<VentilationRoute>()
                    VentilationScreen(r.serverId, onBack = { nav.popBackStack() }) { nav.navigate(VentilationRoomRoute(r.serverId, it)) }
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
                    )
                }
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

private const val SECTION_KEY = "section"

@Composable
private fun TabBar(nav: NavHostController, destination: NavDestination?) {
    NavigationBar(containerColor = MaterialTheme.colorScheme.surfaceContainer) {
        TABS.forEach { tab ->
            NavigationBarItem(
                selected = destination?.hasRoute(tab.route::class) == true,
                onClick = {
                    nav.navigate(tab.route) {
                        popUpTo(nav.graph.findStartDestination().id) { saveState = true }
                        launchSingleTop = true
                        restoreState = true
                    }
                },
                icon = { Icon(tab.icon, null) },
                label = { Text(stringResource(tab.label)) },
                colors = fzNavigationColors(),
            )
        }
    }
}

@Composable
private fun SectionBar(entry: NavBackStackEntry) {
    val section by entry.savedStateHandle.getStateFlow(SECTION_KEY, 0).collectAsState()
    NavigationBar(containerColor = MaterialTheme.colorScheme.surfaceContainer) {
        INSTALLATION_SECTIONS.forEachIndexed { i, (label, icon) ->
            NavigationBarItem(
                selected = section == i,
                onClick = { entry.savedStateHandle[SECTION_KEY] = i },
                icon = { Icon(icon, null) },
                label = { Text(stringResource(label)) },
                colors = fzNavigationColors(),
            )
        }
    }
}
