package com.breezone.app.ui.home

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.breezone.app.data.BlockTypes
import com.breezone.app.data.DashboardBlock
import com.breezone.app.data.DashboardLayout
import com.breezone.app.data.Hub
import com.breezone.app.data.Routine
import com.breezone.app.data.ServerDevices
import com.breezone.app.data.installation
import com.breezone.app.data.modulesHere
import com.breezone.app.resources.*
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.screens.ConfirmDialog
import com.breezone.app.ui.screens.FzPullRefresh
import com.breezone.app.ui.screens.FzTopBar
import com.breezone.app.ui.screens.LocalCardFillsHeight
import com.breezone.app.ui.screens.Muted
import com.breezone.app.ui.screens.rememberEnergy
import kotlinx.coroutines.delay
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject

/** The cards that read the scenes of the modules: they are read with the devices then. */
private val SCENE_BLOCKS = setOf(BlockTypes.LIGHT_SCENES)

/**
 * A page of the app as cards each user arranges, apart from the web's (the server keeps them as
 * [page], one of the [com.breezone.app.data.MobilePages]); without any, [generated] (the page as
 * it was before it could be arranged). Without an installation, there is nowhere to keep a layout:
 * the page as it comes. [header] sits above the cards (buttons for all the devices of the page).
 */
@Composable
fun PageBoard(
    serverId: String,
    installationId: Long?,
    page: String,
    title: String,
    subtitle: String?,
    onBack: (() -> Unit)?,
    nav: HomeNav,
    generated: (HomeContext) -> List<DashboardBlock>,
    everyMs: Long = 5_000,
    /** The page as it comes reads the energy of the home (its cards are chosen from it). */
    readsEnergy: Boolean = false,
    actions: @Composable () -> Unit = {},
    header: (LazyListScope.(HomeContext) -> Unit)? = null,
) {
    val hub = koinInject<Hub>()
    val servers by hub.servers.collectAsState()
    val all by hub.devices.collectAsState()
    val deviceStatus by hub.deviceStatus.collectAsState()
    val commandStatus by hub.commandStatus.collectAsState()
    val scope = rememberCoroutineScope()
    val server = servers.firstOrNull { it.account.id == serverId }
    val inst = installationId?.let { servers.installation(serverId, it) }
    val serverDevices = all[serverId] ?: ServerDevices()
    val here = server?.modulesHere(inst) ?: true

    var saved by remember(serverId, installationId, page) { mutableStateOf<DashboardLayout?>(null) }
    var loaded by remember(serverId, installationId, page) { mutableStateOf(installationId == null) }
    LaunchedEffect(serverId, installationId, page) {
        if (installationId == null) return@LaunchedEffect
        saved = runCatching { hub.api(serverId)?.dashboard(installationId, page) }.getOrNull()?.layout
        loaded = true
    }
    var draft by remember { mutableStateOf<List<DashboardBlock>?>(null) }
    var adding by remember { mutableStateOf(false) }
    var settingsOf by remember { mutableStateOf<String?>(null) }
    var resetting by remember { mutableStateOf(false) }

    val devices = serverDevices.devices.filter { d -> if (d.sourceId == null) d.installationId == installationId else here }
    var routines by remember(serverId) { mutableStateOf<List<Routine>?>(null) }
    val pages = AppPage.of(PageContext(inst, serverDevices, here))
    val shownBlocks = draft ?: saved?.blocks
    val needsScenes = shownBlocks?.any { it.type in SCENE_BLOCKS } ?: true
    LaunchedEffect(serverId, everyMs, needsScenes) { hub.followDevices(serverId, everyMs = everyMs, withScenes = needsScenes) }
    LaunchedEffect(serverId) {
        while (true) {
            runCatching { hub.api(serverId)?.routines() }.getOrNull()?.let { routines = it }
            delay(5 * 60_000L)
        }
    }
    val bare = HomeContext(
        hub = hub, serverId = serverId, installation = inst, devices = devices, rooms = serverDevices.rooms,
        server = serverDevices, pages = pages, live = null, day = null, weather = null,
        routines = routines, deviceStatus = deviceStatus, commandStatus = commandStatus, nav = nav,
    )
    // The energy is read only while a card of the page shows it.
    val reads = { list: List<DashboardBlock> -> list.any { it.type in ENERGY_BLOCKS || it.type.startsWith("energy_") || it.type.startsWith("solar_") } }
    val needsEnergy = here && (draft != null || (shownBlocks?.let(reads) ?: (readsEnergy || reads(generated(bare)))))
    val (live, day) = if (needsEnergy) rememberEnergy(hub, serverId) else null to null
    val ctx = bare.copy(live = live, day = day)
    val blocks = shownBlocks ?: generated(ctx)
    val editing = draft != null
    val canArrange = installationId != null && inst != null && loaded

    if (adding) {
        AddBlockSheet(ctx, onDismiss = { adding = false }, first = page) { b ->
            draft = draft.orEmpty() + b
            adding = false
            if (BlockKind.of(b) in setOf(BlockKind.Room, BlockKind.Kind)) settingsOf = b.id
        }
    }
    draft?.firstOrNull { it.id == settingsOf }?.let { b ->
        BlockSettingsSheet(b, ctx, onDismiss = { settingsOf = null }) { next ->
            draft = draft.orEmpty().map { if (it.id == next.id) next else it }
            settingsOf = null
        }
    }
    if (resetting && installationId != null) {
        ConfirmDialog(
            title = stringResource(Res.string.page_reset_title),
            text = stringResource(Res.string.page_reset_text),
            confirm = stringResource(Res.string.home_reset),
            onConfirm = {
                resetting = false
                draft = null
                hub.launchAttempt(scope) {
                    hub.api(serverId)?.resetDashboard(installationId, page)
                    saved = null
                }
            },
            onDismiss = { resetting = false },
        )
    }

    Scaffold(topBar = {
        FzTopBar(title, subtitle, onBack) {
            if (editing && installationId != null) {
                IconButton(onClick = { resetting = true }) { Icon(FzIcons.RotateCcw, stringResource(Res.string.home_reset)) }
                IconButton(onClick = {
                    val next = DashboardLayout(blocks = draft.orEmpty())
                    draft = null
                    saved = next
                    hub.launchAttempt(scope) { hub.api(serverId)?.saveDashboard(installationId, page, next) }
                }) { Icon(FzIcons.Check, stringResource(Res.string.home_done), tint = MaterialTheme.colorScheme.primary) }
            } else {
                actions()
                if (canArrange) IconButton(onClick = { draft = blocks }) { Icon(FzIcons.Pencil, stringResource(Res.string.home_arrange)) }
            }
        }
    }) { padding ->
        FzPullRefresh({ hub.reload(serverId) }, Modifier.fillMaxSize(), top = padding.calculateTopPadding(), enabled = !editing) {
            LazyColumn(
                Modifier.fillMaxSize(),
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = padding.calculateTopPadding() + 4.dp, bottom = 24.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                if (editing) {
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
                    header?.invoke(this, ctx)
                    items(rows(blocks), key = { row -> row.first().id }, contentType = { row -> if (row.size == 2) "pair" else row.first().type }) { row ->
                        if (row.size == 1) {
                            BlockContent(row.first(), ctx, editing = false)
                        } else {
                            CompositionLocalProvider(LocalCardFillsHeight provides true) {
                                EqualPair(12.dp, { BlockContent(row[0], ctx, editing = false) }, { BlockContent(row[1], ctx, editing = false) })
                            }
                        }
                    }
                }
            }
        }
    }
}
