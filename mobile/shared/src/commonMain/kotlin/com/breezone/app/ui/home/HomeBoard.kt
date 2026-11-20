package com.breezone.app.ui.home

import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.breezone.app.data.BlockSize
import com.breezone.app.data.BlockTypes
import com.breezone.app.data.DashboardBlock
import com.breezone.app.data.DashboardLayout
import com.breezone.app.data.EnergyRoles
import com.breezone.app.data.HomeFacts
import com.breezone.app.data.Hub
import com.breezone.app.data.MOBILE_PAGE
import com.breezone.app.data.Routine
import com.breezone.app.data.ServerDevices
import com.breezone.app.data.WeatherView
import com.breezone.app.data.generateMobileLayout
import com.breezone.app.data.installation
import com.breezone.app.data.modulesHere
import com.breezone.app.resources.*
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.screens.ConfirmDialog
import com.breezone.app.ui.screens.LoadingWave
import com.breezone.app.ui.screens.Muted
import com.breezone.app.ui.screens.SectionCard
import com.breezone.app.ui.screens.rememberEnergy
import com.breezone.app.ui.theme.Fz
import kotlinx.coroutines.delay
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject

/** The cards that read the energy of the home now: it is read only while one is on the home. */
private val ENERGY_BLOCKS = setOf(BlockTypes.ENERGY, BlockTypes.ENERGY_FLOW, BlockTypes.ENERGY_CURVE, BlockTypes.ENERGY_BREAKDOWN, BlockTypes.SOLAR_TODAY)

/**
 * The home of an installation: cards each user arranges for the phone, apart from the web's
 * overview (the server keeps them as the page [MOBILE_PAGE]). Without any, the app composes one.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HomeBoard(serverId: String, installationId: Long, nav: HomeNav, onInstallations: () -> Unit) {
    val hub = koinInject<Hub>()
    val servers by hub.servers.collectAsState()
    val all by hub.devices.collectAsState()
    val deviceStatus by hub.deviceStatus.collectAsState()
    val commandStatus by hub.commandStatus.collectAsState()
    val scope = rememberCoroutineScope()
    val server = servers.firstOrNull { it.account.id == serverId }
    val inst = servers.installation(serverId, installationId)
    val serverDevices = all[serverId] ?: ServerDevices()
    val here = server?.modulesHere(inst) ?: false
    LaunchedEffect(serverId) { hub.followDevices(serverId, everyMs = 10_000) }

    // The layout: null while it is read, `generated` when nobody arranged one.
    var saved by remember(serverId, installationId) { mutableStateOf<DashboardLayout?>(null) }
    var loaded by remember(serverId, installationId) { mutableStateOf(false) }
    LaunchedEffect(serverId, installationId) {
        val view = runCatching { hub.api(serverId)?.dashboard(installationId, MOBILE_PAGE) }.getOrNull()
        saved = view?.layout
        loaded = true
    }
    var draft by remember { mutableStateOf<List<DashboardBlock>?>(null) }
    var adding by remember { mutableStateOf(false) }
    var settingsOf by remember { mutableStateOf<String?>(null) }
    var resetting by remember { mutableStateOf(false) }

    // The devices of this installation: its own zones, and the modules' when they go with it.
    val devices = serverDevices.devices.filter { d ->
        if (d.sourceId == null) d.installationId == installationId else here
    }
    var routines by remember(serverId) { mutableStateOf<List<Routine>?>(null) }
    var weather by remember { mutableStateOf<WeatherView?>(null) }
    LaunchedEffect(serverId, installationId) {
        weather = null
        while (true) {
            weather = runCatching { hub.api(serverId)?.weather(installationId) }.getOrNull() ?: weather
            delay(10 * 60_000L)
        }
    }
    LaunchedEffect(serverId) {
        while (true) {
            runCatching { hub.api(serverId)?.routines() }.getOrNull()?.let { routines = it }
            delay(5 * 60_000L)
        }
    }
    val pages = AppPage.of(PageContext(inst, serverDevices, here))
    val measured = serverDevices.modules.mapNotNull { it.role }.toSet()
    val roles = EnergyRoles(grid = "grid" in measured, solar = "solar" in measured, battery = "battery" in measured, consumption = "consumption" in measured)
    val generated = generateMobileLayout(
        HomeFacts(devices, serverDevices.rooms, roles.takeIf { here }, hasZones = inst?.zones?.isNotEmpty() == true, hasRoutines = routines?.isNotEmpty() == true),
    ).blocks
    val blocks = draft ?: saved?.blocks ?: generated
    val needsEnergy = here && (draft != null || blocks.any { it.type in ENERGY_BLOCKS })
    val (live, day) = if (needsEnergy) rememberEnergy(hub, serverId) else null to null

    val ctx = HomeContext(
        hub = hub, serverId = serverId, installation = inst, devices = devices, rooms = serverDevices.rooms,
        server = serverDevices, pages = pages, live = live, day = day, weather = if (here) weather else null,
        routines = routines, deviceStatus = deviceStatus, commandStatus = commandStatus, nav = nav,
    )
    val editing = draft != null

    if (adding) {
        AddBlockSheet(ctx, onDismiss = { adding = false }) { b ->
            draft = draft.orEmpty() + b
            adding = false
            // A room or a type: say at once which of its devices the card shows.
            if (BlockKind.of(b) in setOf(BlockKind.Room, BlockKind.Kind)) settingsOf = b.id
        }
    }
    draft?.firstOrNull { it.id == settingsOf }?.let { b ->
        BlockSettingsSheet(b, ctx, onDismiss = { settingsOf = null }) { next ->
            draft = draft.orEmpty().map { if (it.id == next.id) next else it }
            settingsOf = null
        }
    }
    if (resetting) {
        ConfirmDialog(
            title = stringResource(Res.string.home_reset_title),
            text = stringResource(Res.string.home_reset_text),
            confirm = stringResource(Res.string.home_reset),
            onConfirm = {
                resetting = false
                draft = null
                hub.launchAttempt(scope) {
                    hub.api(serverId)?.resetDashboard(installationId, MOBILE_PAGE)
                    saved = null
                }
            },
            onDismiss = { resetting = false },
        )
    }

    Scaffold(topBar = {
        TopAppBar(
            title = {
                Row(Modifier.clickable(onClick = onInstallations), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f, fill = false)) {
                        Text(inst?.name ?: stringResource(Res.string.house_title), style = MaterialTheme.typography.titleLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        inst?.location?.takeIf(String::isNotBlank)?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = Fz.colors.muted, maxLines = 1) }
                    }
                    Icon(FzIcons.ChevronDown, stringResource(Res.string.home_switch_installation), Modifier.padding(start = 4.dp).size(18.dp), tint = Fz.colors.muted)
                }
            },
            actions = {
                if (editing) {
                    IconButton(onClick = { resetting = true }) { Icon(FzIcons.RotateCcw, stringResource(Res.string.home_reset)) }
                    IconButton(onClick = {
                        val next = DashboardLayout(blocks = draft.orEmpty())
                        draft = null
                        saved = next
                        hub.launchAttempt(scope) { hub.api(serverId)?.saveDashboard(installationId, MOBILE_PAGE, next) }
                    }) { Icon(FzIcons.Check, stringResource(Res.string.home_done), tint = MaterialTheme.colorScheme.primary) }
                } else if (loaded && inst != null) {
                    IconButton(onClick = { draft = blocks }) { Icon(FzIcons.Pencil, stringResource(Res.string.home_arrange)) }
                }
            },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
        )
    }) { padding ->
        // The server or the layout still being read: the wave, not an empty or missing home.
        val waiting = if (inst == null) server != null && !server.loaded && !server.authRequired && server.error == null else !loaded
        if (waiting) {
            Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) { LoadingWave() }
            return@Scaffold
        }
        LazyColumn(
            Modifier.fillMaxSize(),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = padding.calculateTopPadding() + 4.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (inst == null) {
                item(key = "missing", contentType = "note") {
                    Muted(server?.error?.takeIf { !server.loaded } ?: stringResource(Res.string.installation_not_found))
                }
            } else if (editing) {
                item(key = "hint", contentType = "note") { Muted(stringResource(Res.string.home_arrange_hint)) }
                val list = draft.orEmpty()
                items(list, key = { "edit-${it.id}" }, contentType = { "edit" }) { b ->
                    val i = list.indexOf(b)
                    EditFrame(
                        b = b,
                        ctx = ctx,
                        first = i == 0,
                        last = i == list.lastIndex,
                        onMove = { by -> draft = list.toMutableList().apply { add((i + by).coerceIn(0, lastIndex), removeAt(i)) } },
                        onSize = { size -> draft = list.map { if (it.id == b.id) it.copy(size = size.wire) else it } },
                        onRemove = { draft = list.filterNot { it.id == b.id } },
                        onSettings = { settingsOf = b.id },
                    )
                }
                item(key = "add", contentType = "add") {
                    OutlinedButton(onClick = { adding = true }, modifier = Modifier.fillMaxWidth()) {
                        Icon(FzIcons.Plus, null, Modifier.size(18.dp))
                        Spacer(Modifier.width(8.dp))
                        Text(stringResource(Res.string.home_add_card))
                    }
                }
            } else {
                if (!here && server?.installations?.any { it.access == "local" } == true) {
                    item(key = "elsewhere", contentType = "note") { SectionCard { Muted(stringResource(Res.string.house_other_server, inst.name)) } }
                }
                // Two half cards side by side; a full card, or a half one alone, takes the row.
                items(rows(blocks), key = { row -> row.first().id }, contentType = { row -> if (row.size == 2) "pair" else row.first().type }) { row ->
                    if (row.size == 1) {
                        BlockContent(row.first(), ctx, editing = false)
                    } else {
                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            row.forEach { b -> Column(Modifier.weight(1f)) { BlockContent(b, ctx, editing = false) } }
                        }
                    }
                }
            }
        }
    }
}

/** The rows of the home: two half cards that follow each other share one. */
private fun rows(blocks: List<DashboardBlock>): List<List<DashboardBlock>> {
    val out = mutableListOf<List<DashboardBlock>>()
    var i = 0
    while (i < blocks.size) {
        val b = blocks[i]
        val next = blocks.getOrNull(i + 1)
        if (b.phoneSize == BlockSize.Half && next?.phoneSize == BlockSize.Half) {
            out += listOf(b, next)
            i += 2
        } else {
            out += listOf(b)
            i++
        }
    }
    return out
}

/** The cards that can sit two by two: those whose content holds in half the width. */
private val HALVES = setOf(BlockKind.Pages, BlockKind.Energy, BlockKind.Presence, BlockKind.Device, BlockKind.Chart, BlockKind.Spacer)

/** A card while the home is arranged: its name and its handles above what it shows. */
@Composable
private fun EditFrame(
    b: DashboardBlock,
    ctx: HomeContext,
    first: Boolean,
    last: Boolean,
    onMove: (Int) -> Unit,
    onSize: (BlockSize) -> Unit,
    onRemove: () -> Unit,
    onSettings: () -> Unit,
) {
    val kind = BlockKind.of(b)
    Column(
        Modifier
            .fillMaxWidth()
            .border(1.5.dp, MaterialTheme.colorScheme.primary.copy(alpha = 0.45f), RoundedCornerShape(28.dp))
            .padding(8.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(kind?.icon ?: FzIcons.Box, null, Modifier.padding(start = 8.dp).size(16.dp), tint = MaterialTheme.colorScheme.primary)
            Spacer(Modifier.width(8.dp))
            Text(blockTitle(b, ctx), style = MaterialTheme.typography.titleSmall, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
            IconButton(onClick = { onMove(-1) }, enabled = !first) { Icon(FzIcons.ArrowUp, stringResource(Res.string.home_move_up), Modifier.size(18.dp)) }
            IconButton(onClick = { onMove(1) }, enabled = !last) { Icon(FzIcons.ArrowDown, stringResource(Res.string.home_move_down), Modifier.size(18.dp)) }
            if (kind in HALVES) {
                val half = b.phoneSize == BlockSize.Half
                IconButton(onClick = { onSize(if (half) BlockSize.Full else BlockSize.Half) }) {
                    Icon(
                        if (half) FzIcons.Maximize2 else FzIcons.Minimize2,
                        stringResource(if (half) Res.string.home_size_full else Res.string.home_size_half),
                        Modifier.size(18.dp),
                    )
                }
            }
            if (kind?.settable == true) {
                IconButton(onClick = onSettings) { Icon(FzIcons.Settings, stringResource(Res.string.block_settings), Modifier.size(18.dp)) }
            }
            IconButton(onClick = onRemove) { Icon(FzIcons.X, stringResource(Res.string.home_remove), Modifier.size(18.dp), tint = MaterialTheme.colorScheme.error) }
        }
        if (b.phoneSize == BlockSize.Half && kind in HALVES) {
            Row { Column(Modifier.weight(1f)) { BlockContent(b, ctx, editing = true) }; Spacer(Modifier.weight(1f)) }
        } else {
            BlockContent(b, ctx, editing = true)
        }
    }
}
