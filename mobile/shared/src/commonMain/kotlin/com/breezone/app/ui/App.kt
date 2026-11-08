package com.breezone.app.ui

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
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.navigation.NavDestination.Companion.hasRoute
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.toRoute
import com.breezone.app.data.Hub
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.screens.AddServerScreen
import com.breezone.app.ui.screens.fzNavigationColors
import com.breezone.app.ui.screens.HomeScreen
import com.breezone.app.ui.screens.InstallationScreen
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
        val nav = rememberNavController()
        val snackbar = remember { SnackbarHostState() }
        val backStack by nav.currentBackStackEntryAsState()
        val destination = backStack?.destination
        val showTabs = TABS.any { tab -> destination?.hasRoute(tab.route::class) == true }
        // Computed once: the saved servers are loaded synchronously by the Hub.
        val start = remember { if (servers.isEmpty()) AddServerRoute() else HomeRoute }

        LaunchedEffect(Unit) { hub.messages.collect { snackbar.showSnackbar(it) } }
        // Coming back to the app: reconnect the push channels that dropped in background.
        LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { hub.refresh() }

        Scaffold(
            snackbarHost = { SnackbarHost(snackbar) },
            bottomBar = {
                if (showTabs) {
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
            },
        ) { padding ->
            NavHost(
                navController = nav,
                startDestination = start,
                modifier = Modifier.padding(bottom = if (showTabs) padding.calculateBottomPadding() else androidx.compose.ui.unit.Dp(0f)),
            ) {
                composable<HomeRoute> {
                    HomeScreen(
                        onOpenInstallation = { serverId, id -> nav.navigate(InstallationRoute(serverId, id)) },
                        onReauth = { nav.navigate(AddServerRoute(it)) },
                        onAddServer = { nav.navigate(AddServerRoute()) },
                    )
                }
                composable<InstallationRoute> { entry ->
                    val r = entry.toRoute<InstallationRoute>()
                    InstallationScreen(
                        serverId = r.serverId,
                        installationId = r.installationId,
                        onBack = { nav.popBackStack() },
                        onOpenZone = { z -> nav.navigate(ZoneRoute(r.serverId, z.installationId, z.systemId, z.zoneId)) },
                    )
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
