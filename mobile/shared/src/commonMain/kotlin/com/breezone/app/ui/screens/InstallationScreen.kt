package com.breezone.app.ui.screens

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.breezone.app.data.CommandStatus
import com.breezone.app.data.Hub
import com.breezone.app.data.installation
import com.breezone.app.data.Installation
import com.breezone.app.data.Mode
import com.breezone.app.data.Zone
import com.breezone.app.data.ZoneCommand
import com.breezone.app.data.commandKey
import com.breezone.app.resources.*
import com.breezone.app.ui.color
import com.breezone.app.ui.components.StatusDot
import com.breezone.app.ui.components.ZoneCard
import com.breezone.app.ui.groupZones
import com.breezone.app.ui.icon
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.installationPlace
import com.breezone.app.ui.label
import com.breezone.app.ui.labelRes
import com.breezone.app.ui.systemModePlacement
import com.breezone.app.ui.theme.Fz
import org.jetbrains.compose.resources.getString
import org.jetbrains.compose.resources.pluralStringResource
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject

private const val TYPE_MODES = "modes"
private const val TYPE_MESSAGE = "message"
private const val TYPE_GROUP = "group"
private const val TYPE_ZONE = "zone"

@Composable
fun InstallationScreen(
    serverId: String,
    installationId: Long,
    section: Int,
    onBack: () -> Unit,
    onOpenZone: (Zone) -> Unit,
) {
    val hub = koinInject<Hub>()
    val servers by hub.servers.collectAsState()
    val commandStatus by hub.commandStatus.collectAsState()
    val inst = servers.installation(serverId, installationId)

    Scaffold(
        topBar = {
            FzTopBar(
                title = inst?.name ?: stringResource(Res.string.installation_title),
                subtitle = inst?.let { installationPlace(it) },
                onBack = onBack,
                actions = {
                    if (inst != null) {
                        val connection = inst.connection
                        Row(Modifier.padding(end = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                            StatusDot(connection.online, connection.state == "unknown")
                            // Online needs no words: the dot says it. Offline is worth spelling out.
                            if (!connection.online && connection.state != "unknown") {
                                Spacer(Modifier.size(6.dp))
                                Caption(stringResource(Res.string.connection_offline))
                            }
                        }
                    }
                },
            )
        },
    ) { padding ->
        if (inst == null) {
            Muted(stringResource(Res.string.installation_not_found), Modifier.padding(padding).padding(24.dp))
            return@Scaffold
        }
        val content = Modifier.fillMaxSize().padding(padding)
        when (section) {
            0 -> InstallationZones(serverId, inst, commandStatus, onOpenZone, content)
            1 -> InstallationRoutines(serverId, inst, canEdit = inst.controllable && inst.rights.routines, content)
            else -> InstallationSchedules(serverId, inst, canEdit = inst.controllable && inst.rights.schedules, content)
        }
    }
}

val SectionListPadding = PaddingValues(16.dp)

internal val INSTALLATION_SECTIONS = listOf(
    Res.string.installation_tab_zones to FzIcons.Thermometer,
    Res.string.installation_tab_routines to FzIcons.Sparkles,
    Res.string.installation_tab_schedules to FzIcons.CalendarClock,
)

@Composable
private fun InstallationZones(
    serverId: String,
    inst: Installation,
    commandStatus: Map<String, CommandStatus>,
    onOpenZone: (Zone) -> Unit,
    modifier: Modifier = Modifier,
) {
    val hub = koinInject<Hub>()
    val modes = remember(inst.zones) { systemModePlacement(inst.zones) }
    val labelled = modes.systems > 1
    var pendingMode by remember { mutableStateOf<Pair<Zone, Mode>?>(null) }
    val onMode: (Zone, Mode) -> Unit = { master, mode -> pendingMode = master to mode }
    pendingMode?.let { (master, mode) ->
        ModeConfirmation(
            mode = mode,
            onConfirm = {
                hub.control(serverId, master, ZoneCommand(mode = mode)) {
                    getString(Res.string.hub_mode_applied, getString(mode.labelRes).lowercase())
                }
            },
            onDismiss = { pendingMode = null },
        )
    }
    LazyVerticalGrid(
            columns = GridCells.Adaptive(minSize = 160.dp),
            modifier = modifier,
            contentPadding = SectionListPadding,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (inst.controllable && modes.header.isNotEmpty()) {
                item(key = TYPE_MODES, span = { GridItemSpan(maxLineSpan) }, contentType = TYPE_MODES) {
                    SystemModes(modes.header, labelled, onMode)
                }
            }
            if (inst.zones.isEmpty()) {
                item(key = TYPE_MESSAGE, span = { GridItemSpan(maxLineSpan) }, contentType = TYPE_MESSAGE) {
                    Muted(
                        if (inst.connection.state == "offline") {
                            stringResource(Res.string.installation_offline, inst.connection.lastError.orEmpty())
                        } else {
                            stringResource(Res.string.installation_waiting)
                        },
                    )
                }
            }
            groupZones(inst.zones).forEach { (group, zones) ->
                item(key = "group-${group.orEmpty()}", span = { GridItemSpan(maxLineSpan) }, contentType = TYPE_GROUP) {
                    val on = zones.count { it.on }
                    Column(Modifier.fillMaxWidth().padding(top = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Column(Modifier.weight(1f)) {
                                Text(group ?: stringResource(Res.string.installation_group_default), style = MaterialTheme.typography.titleMedium)
                                Caption(pluralStringResource(Res.plurals.installation_group_on, on, on, zones.size))
                            }
                            TextButton(onClick = { hub.setPower(serverId, zones, true, group) }, enabled = on < zones.size) {
                                Text(stringResource(Res.string.installation_all_on))
                            }
                            TextButton(onClick = { hub.setPower(serverId, zones, false, group) }, enabled = on > 0) {
                                Text(stringResource(Res.string.installation_all_off))
                            }
                        }
                        val masters = modes.byGroup[group].orEmpty()
                        if (inst.controllable && masters.isNotEmpty()) SystemModes(masters, labelled, onMode)
                    }
                }
                items(zones, key = { "zone-${it.systemId}-${it.zoneId}" }, contentType = { TYPE_ZONE }) { zone ->
                    ZoneCard(
                        zone = zone,
                        status = commandStatus[commandKey(serverId, zone)],
                        onToggle = { hub.control(serverId, zone, ZoneCommand(on = it)) },
                        onSetpoint = { hub.control(serverId, zone, ZoneCommand(setpoint = it)) },
                        onClick = { onOpenZone(zone) },
                    )
                }
            }
    }
}

/** Mode of each given system, set on its master zone. A handful of chips: plain Row, not lazy. */
@Composable
private fun SystemModes(masters: List<Zone>, labelled: Boolean, onMode: (Zone, Mode) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        masters.forEach { master ->
            if (labelled) Caption(stringResource(Res.string.installation_system, master.systemId.toInt()))
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                master.modes.forEach { mode ->
                    val selected = master.mode == mode
                    val color = mode.color()
                    FilterChip(
                        selected = selected,
                        onClick = { if (!selected) onMode(master, mode) },
                        label = { Text(mode.label()) },
                        leadingIcon = { Icon(mode.icon, null, Modifier.size(16.dp), tint = if (selected) color else Fz.colors.muted) },
                        shape = Fz.shapes.button,
                        colors = FilterChipDefaults.filterChipColors(
                            selectedContainerColor = color.copy(alpha = 0.16f),
                            selectedLabelColor = color,
                        ),
                    )
                }
            }
        }
    }
}
