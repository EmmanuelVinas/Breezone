package com.breezone.app.ui.screens

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AssistChip
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.CommandStatus
import com.breezone.app.data.CoverPositionMoving
import com.breezone.app.data.CoverPositionState
import com.breezone.app.data.DeviceView
import com.breezone.app.data.Hub
import com.breezone.app.data.ServerDevices
import com.breezone.app.data.TraitIds
import com.breezone.app.resources.*
import com.breezone.app.ui.components.CommandIndicator
import com.breezone.app.ui.components.DeviceGroupCard
import com.breezone.app.ui.components.ShutterWindow
import com.breezone.app.ui.components.groupByRoom
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.theme.Fz
import kotlinx.coroutines.delay
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.jetbrains.compose.resources.pluralStringResource
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject

private fun DeviceView.cover() = stateAs(CoverPositionState.serializer()) ?: CoverPositionState()

private fun DeviceView.isOpen() = available && cover().position > 0

private fun CoverPositionState.moving() = moving == CoverPositionMoving.Opening || moving == CoverPositionMoving.Closing

/** The state to show: while it moves, where the shutter should be by now (see [com.breezone.app.data.CoverMotions]). */
private fun Hub.coverView(serverId: String, d: DeviceView) = coverMotions.view("$serverId/${d.id}", d.cover())

/** [coverView], drawn again a few times a second while the shutter moves. */
@Composable
private fun Hub.movingCover(serverId: String, d: DeviceView): CoverPositionState {
    var tick by remember { mutableStateOf(0) }
    tick
    val s = coverView(serverId, d)
    val moving = s.moving()
    LaunchedEffect(moving) {
        while (moving) {
            delay(250)
            tick++
        }
    }
    return s
}

@Composable
private fun animatedOpen(position: Long) = animateFloatAsState(position / 100f, tween(300, easing = LinearEasing))

// Unknown counts as stoppable: only a driver that knows it cannot stop says so.
private fun CoverPositionState.stoppable() = stoppable != false

@Composable
private fun coverText(s: CoverPositionState): String = when {
    s.moving == CoverPositionMoving.Opening -> s.target?.let { stringResource(Res.string.shutter_opening_to, it) } ?: stringResource(Res.string.shutter_opening)
    s.moving == CoverPositionMoving.Closing -> s.target?.let { stringResource(Res.string.shutter_closing_to, it) } ?: stringResource(Res.string.shutter_closing)
    s.position <= 0 -> stringResource(Res.string.shutter_closed)
    s.position >= 100 -> stringResource(Res.string.shutter_open)
    else -> stringResource(Res.string.shutter_open_at, s.position)
}

@Composable
private fun coverKind(d: DeviceView): String = stringResource(
    when (d.kind) {
        "window" -> Res.string.shutter_kind_window
        "door" -> Res.string.shutter_kind_door
        else -> Res.string.shutter_kind_shutter
    },
)

private fun Hub.setCover(serverId: String, d: DeviceView, position: Int) {
    val target = position.coerceIn(0, 100)
    coverMotions.start("$serverId/${d.id}", d.cover(), target.toLong())
    val moving = when {
        target > d.cover().position -> "opening"
        target < d.cover().position -> "closing"
        else -> "stopped"
    }
    act(serverId, d, "cover.position", "set", buildJsonObject { put("position", target) }, optimistic = buildJsonObject { put("target", target); put("moving", moving) })
}

private fun Hub.stopCover(serverId: String, d: DeviceView) {
    coverMotions.stop("$serverId/${d.id}")
    act(serverId, d, "cover.position", "stop", JsonObject(emptyMap()), optimistic = buildJsonObject { put("moving", "stopped") })
}

@Composable
fun ShuttersScreen(serverId: String, onBack: (() -> Unit)?, onOpen: (Long) -> Unit) {
    val hub = koinInject<Hub>()
    val all by hub.devices.collectAsState()
    val status by hub.deviceStatus.collectAsState()
    val data = all[serverId] ?: ServerDevices()
    val covers = data.withTrait(TraitIds.COVER)
    // Faster while one moves: its position shows as it goes.
    val anyMoving = covers.any { hub.coverView(serverId, it).moving() }
    LaunchedEffect(serverId, anyMoving) { hub.followDevices(serverId, everyMs = if (anyMoving) 1_500 else 5_000) }
    val usable = covers.filter { it.usable }
    val noRoom = stringResource(Res.string.room_none)

    Scaffold(topBar = {
        FzTopBar(stringResource(Res.string.house_shutters), pluralStringResource(Res.plurals.shutters_open_of, covers.count { it.isOpen() }, covers.count { it.isOpen() }, covers.size), onBack)
    }) { padding ->
        LazyColumn(
            Modifier.fillMaxSize(),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = padding.calculateTopPadding() + 4.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item(key = "all", contentType = "all") {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    AssistChip(
                        onClick = { usable.forEach { hub.setCover(serverId, it, 100) } },
                        enabled = usable.isNotEmpty(),
                        label = { Text(stringResource(Res.string.shutters_open_all)) },
                        leadingIcon = { Icon(FzIcons.ChevronUp, null, Modifier.size(16.dp)) },
                    )
                    AssistChip(
                        onClick = { usable.forEach { hub.setCover(serverId, it, 0) } },
                        enabled = usable.isNotEmpty(),
                        label = { Text(stringResource(Res.string.shutters_close_all)) },
                        leadingIcon = { Icon(FzIcons.ChevronDown, null, Modifier.size(16.dp)) },
                    )
                }
            }
            if (data.loaded && covers.isEmpty()) {
                item(key = "empty", contentType = "note") { SectionCard { Muted(stringResource(Res.string.shutters_empty)) } }
            }
            items(groupByRoom(covers, data.rooms), key = { it.first?.id ?: -1L }, contentType = { "room" }) { (room, list) ->
                DeviceGroupCard(room?.name ?: noRoom, pluralStringResource(Res.plurals.shutters_open_of, list.count { it.isOpen() }, list.count { it.isOpen() }, list.size)) {
                    list.forEach { d -> ShutterRow(d, status["$serverId/${d.id}"], onOpen = { onOpen(d.id) }, hub = hub, serverId = serverId) }
                }
            }
        }
    }
}

@Composable
private fun CoverButtons(enabled: Boolean, onUp: () -> Unit, onStop: (() -> Unit)?, onDown: () -> Unit) {
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        FilledTonalIconButton(onClick = onUp, enabled = enabled, modifier = Modifier.size(38.dp)) {
            Icon(FzIcons.ChevronUp, stringResource(Res.string.shutter_up), Modifier.size(18.dp))
        }
        if (onStop != null) {
            FilledTonalIconButton(onClick = onStop, enabled = enabled, modifier = Modifier.size(38.dp)) {
                Icon(FzIcons.Square, stringResource(Res.string.shutter_stop), Modifier.size(14.dp))
            }
        }
        FilledTonalIconButton(onClick = onDown, enabled = enabled, modifier = Modifier.size(38.dp)) {
            Icon(FzIcons.ChevronDown, stringResource(Res.string.shutter_down), Modifier.size(18.dp))
        }
    }
}

@Composable
internal fun ShutterRow(d: DeviceView, status: CommandStatus?, onOpen: () -> Unit, hub: Hub, serverId: String) {
    val s = hub.movingCover(serverId, d)
    val open by animatedOpen(s.position)
    Row(Modifier.fillMaxWidth().clickable(onClick = onOpen).padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        ShutterWindow({ open }, Modifier.width(56.dp).height(44.dp))
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(d.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                if (d.available) coverText(s) else stringResource(Res.string.device_unavailable),
                fontSize = 12.sp,
                color = if (s.moving()) MaterialTheme.colorScheme.primary else Fz.colors.muted,
                maxLines = 1,
            )
        }
        CommandIndicator(status, Modifier.padding(end = 6.dp))
        CoverButtons(d.usable, { hub.setCover(serverId, d, 100) }, { hub.stopCover(serverId, d) }.takeIf { d.cover().stoppable() }, { hub.setCover(serverId, d, 0) })
    }
}

@Composable
fun ShutterScreen(serverId: String, deviceId: Long, onBack: () -> Unit) {
    val hub = koinInject<Hub>()
    val all by hub.devices.collectAsState()
    val status by hub.deviceStatus.collectAsState()
    val data = all[serverId] ?: ServerDevices()
    val d = data.devices.firstOrNull { it.id == deviceId }
    val moving = d?.let { hub.coverView(serverId, it).moving() } == true
    LaunchedEffect(serverId, moving) { hub.followDevices(serverId, everyMs = if (moving) 1_500 else 5_000) }
    val room = data.rooms.firstOrNull { it.id == d?.roomId }
    Scaffold(topBar = {
        FzTopBar(d?.name ?: stringResource(Res.string.house_shutters), listOfNotNull(room?.name, d?.let { coverKind(it) }).joinToString(" · "), onBack) {
            CommandIndicator(status["$serverId/$deviceId"], Modifier.padding(end = 16.dp))
        }
    }) { padding ->
        if (d == null) return@Scaffold
        val s = hub.movingCover(serverId, d)
        var draft by remember { mutableStateOf<Int?>(null) }
        val open by animatedOpen(s.position)
        // The bar shows where the shutter goes, the window where it is.
        val shown = draft ?: (if (s.moving()) s.target ?: s.position else s.position).toInt()
        LazyColumn(
            Modifier.fillMaxSize(),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = padding.calculateTopPadding() + 4.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item(key = "main", contentType = "main") {
                SectionCard {
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(22.dp, Alignment.CenterHorizontally), verticalAlignment = Alignment.CenterVertically) {
                        ShutterWindow({ draft?.let { it / 100f } ?: open }, Modifier.width(170.dp).height(150.dp))
                        PositionBar(shown, enabled = d.usable, onDraft = { draft = it }) { v ->
                            draft = null
                            hub.setCover(serverId, d, v)
                        }
                    }
                    Text(
                        if (draft != null) stringResource(Res.string.shutter_open_at, shown) else if (d.available) coverText(s) else stringResource(Res.string.device_unavailable),
                        color = if (s.moving()) MaterialTheme.colorScheme.primary else Fz.colors.muted,
                        modifier = Modifier.align(Alignment.CenterHorizontally),
                    )
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedButton(onClick = { hub.setCover(serverId, d, 100) }, enabled = d.usable, modifier = Modifier.weight(1f), shape = Fz.shapes.button) {
                            Icon(FzIcons.ChevronUp, null, Modifier.size(18.dp))
                            Text(stringResource(Res.string.shutter_up))
                        }
                        if (s.stoppable()) {
                            OutlinedButton(onClick = { hub.stopCover(serverId, d) }, enabled = d.usable, modifier = Modifier.weight(1f), shape = Fz.shapes.button) {
                                Icon(FzIcons.Square, null, Modifier.size(14.dp))
                                Spacer(Modifier.width(4.dp))
                                Text(stringResource(Res.string.shutter_stop))
                            }
                        }
                        OutlinedButton(onClick = { hub.setCover(serverId, d, 0) }, enabled = d.usable, modifier = Modifier.weight(1f), shape = Fz.shapes.button) {
                            Icon(FzIcons.ChevronDown, null, Modifier.size(18.dp))
                            Text(stringResource(Res.string.shutter_down))
                        }
                    }
                }
            }
        }
    }
}

/** The vertical position bar: the shutter's part filled from the top; drag or tap to set it. */
@Composable
private fun PositionBar(open: Int, enabled: Boolean, onDraft: (Int) -> Unit, onSet: (Int) -> Unit) {
    var last by remember { mutableStateOf<Int?>(null) }
    fun at(y: Float, height: Int) = ((1f - y / height) * 100).toInt().coerceIn(0, 100)
    Box(
        Modifier
            .width(64.dp)
            .height(220.dp)
            .clip(RoundedCornerShape(26.dp))
            .background(MaterialTheme.colorScheme.surfaceVariant)
            .pointerInput(enabled) {
                if (!enabled) return@pointerInput
                detectTapGestures { onSet(at(it.y, size.height)) }
            }
            .pointerInput(enabled) {
                if (!enabled) return@pointerInput
                detectDragGestures(
                    onDragEnd = { last?.let(onSet); last = null },
                    onDragCancel = { last = null },
                ) { change, _ ->
                    val v = at(change.position.y, size.height)
                    last = v
                    onDraft(v)
                }
            },
        contentAlignment = Alignment.TopCenter,
    ) {
        Box(Modifier.fillMaxWidth().fillMaxHeight((100 - open) / 100f).background(Fz.colors.shutter.copy(alpha = 0.85f)))
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Text(stringResource(Res.string.common_percent, open), fontWeight = FontWeight.Bold, color = MaterialTheme.colorScheme.onSurface)
        }
    }
}
