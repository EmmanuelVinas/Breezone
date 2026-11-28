package com.breezone.app.ui.screens

import com.breezone.app.data.Room
import com.breezone.app.data.BlockTypes
import com.breezone.app.data.MobilePages
import com.breezone.app.data.fixedBlock
import com.breezone.app.ui.home.HomeNav
import com.breezone.app.ui.home.PageBoard
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonPrimitive
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
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
import androidx.compose.runtime.State
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.CommandStatus
import com.breezone.app.data.CoverMotions
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
import kotlin.math.abs
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

/**
 * How open the shutter is drawn. While it moves, one animation to its target at the speed of a shutter:
 * restarting a short one at each estimate of the position made it jerk.
 */
@Composable
private fun animatedOpen(s: CoverPositionState): State<Float> {
    val open = remember { Animatable(s.position / 100f) }
    val goal = s.target?.takeIf { s.moving() }
    LaunchedEffect(goal, if (goal == null) s.position else null) {
        if (goal != null) {
            val ms = (abs(goal / 100f - open.value) * 100 * CoverMotions.MS_PER_PERCENT).toInt()
            open.animateTo(goal / 100f, tween(ms, easing = LinearEasing))
        } else {
            open.animateTo(s.position / 100f, tween(300, easing = LinearEasing))
        }
    }
    return open.asState()
}

// Unknown counts as stoppable: only a driver that knows it cannot stop says so.
private fun CoverPositionState.stoppable() = stoppable != false

@Composable
private fun coverText(s: CoverPositionState): String = when {
    s.moving == CoverPositionMoving.Opening -> s.target?.let { stringResource(Res.string.shutter_opening_to, it) } ?: stringResource(Res.string.shutter_opening)
    s.moving == CoverPositionMoving.Closing -> s.target?.let { stringResource(Res.string.shutter_closing_to, it) } ?: stringResource(Res.string.shutter_closing)
    else -> positionText(s.position)
}

/**
 * What the shutter shows: the position being dragged to, else its state. One call whatever the case:
 * branches of composable calls in the arguments of [ShutterControl] would recreate it mid-gesture.
 */
@Composable
private fun shutterLabel(draft: Int?, s: CoverPositionState, available: Boolean): String = when {
    draft != null -> positionText(draft.toLong())
    !available -> stringResource(Res.string.device_unavailable)
    else -> coverText(s)
}

@Composable
private fun positionText(position: Long): String = when {
    position <= 0 -> stringResource(Res.string.shutter_closed)
    position >= 100 -> stringResource(Res.string.shutter_open)
    else -> stringResource(Res.string.shutter_open_at, position)
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

/** Every shutter open or closed at once. */
@Composable
internal fun ShuttersAllRow(serverId: String, covers: List<DeviceView>) {
    val hub = koinInject<Hub>()
    val usable = covers.filter { it.usable }
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

/** The shutters of a room (`null`: without a room), each with its buttons. */
@Composable
internal fun ShutterRoomCard(serverId: String, room: Room?, list: List<DeviceView>, onOpen: (Long) -> Unit) {
    val hub = koinInject<Hub>()
    val status by hub.deviceStatus.collectAsState()
    DeviceGroupCard(room?.name ?: stringResource(Res.string.room_none), pluralStringResource(Res.plurals.shutters_open_of, list.count { it.isOpen() }, list.count { it.isOpen() }, list.size)) {
        list.forEach { d -> ShutterRow(d, status["$serverId/${d.id}"], onOpen = { onOpen(d.id) }, hub = hub, serverId = serverId) }
    }
}

@Composable
fun ShuttersScreen(serverId: String, installationId: Long?, nav: HomeNav, onBack: (() -> Unit)?) {
    val hub = koinInject<Hub>()
    val all by hub.devices.collectAsState()
    val data = all[serverId] ?: ServerDevices()
    val covers = data.withTrait(TraitIds.COVER)
    // Faster while one moves: its position shows as it goes.
    val anyMoving = covers.any { hub.coverView(serverId, it).moving() }
    PageBoard(
        serverId = serverId,
        installationId = installationId,
        page = MobilePages.SHUTTERS,
        title = stringResource(Res.string.house_shutters),
        subtitle = pluralStringResource(Res.plurals.shutters_open_of, covers.count { it.isOpen() }, covers.count { it.isOpen() }, covers.size),
        onBack = onBack,
        nav = nav,
        everyMs = if (anyMoving) 1_500 else 5_000,
        generated = { ctx ->
            groupByRoom(ctx.server.withTrait(TraitIds.COVER), ctx.rooms).map { (room, _) ->
                fixedBlock("room-${room?.id}", BlockTypes.SHUTTER_ROOM, config = mapOf("room_id" to (room?.id?.let(::JsonPrimitive) ?: JsonNull)))
            }
        },
        header = {
            item(key = "all", contentType = "all") { ShuttersAllRow(serverId, covers) }
            if (data.loaded && covers.isEmpty()) item(key = "empty", contentType = "note") { SectionCard { Muted(stringResource(Res.string.shutters_empty)) } }
        },
    )
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
    val open by animatedOpen(s)
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
        val open by animatedOpen(s)
        FzPullRefresh({ hub.reload(serverId) }, Modifier.fillMaxSize(), top = padding.calculateTopPadding()) {
            LazyColumn(
                Modifier.fillMaxSize(),
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = padding.calculateTopPadding() + 4.dp, bottom = 24.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                item(key = "main", contentType = "main") {
                    SectionCard {
                        ShutterControl(
                            open = { draft?.let { it / 100f } ?: open },
                            label = shutterLabel(draft, s, d.available),
                            moving = draft == null && s.moving(),
                            enabled = d.usable,
                            onDraft = { draft = it },
                        ) { v ->
                            draft = null
                            hub.setCover(serverId, d, v)
                        }
                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            OutlinedButton(onClick = { hub.setCover(serverId, d, 100) }, enabled = d.usable, modifier = Modifier.weight(1f), shape = Fz.shapes.button, contentPadding = PaddingValues(horizontal = 8.dp)) {
                                Icon(FzIcons.ChevronUp, null, Modifier.size(18.dp))
                                Text(stringResource(Res.string.shutter_up), maxLines = 1, overflow = TextOverflow.Ellipsis)
                            }
                            if (s.stoppable()) {
                                OutlinedButton(onClick = { hub.stopCover(serverId, d) }, enabled = d.usable, modifier = Modifier.weight(1f), shape = Fz.shapes.button, contentPadding = PaddingValues(horizontal = 8.dp)) {
                                    Icon(FzIcons.Square, null, Modifier.size(14.dp))
                                    Spacer(Modifier.width(4.dp))
                                    Text(stringResource(Res.string.shutter_stop), maxLines = 1, overflow = TextOverflow.Ellipsis)
                                }
                            }
                            OutlinedButton(onClick = { hub.setCover(serverId, d, 0) }, enabled = d.usable, modifier = Modifier.weight(1f), shape = Fz.shapes.button, contentPadding = PaddingValues(horizontal = 8.dp)) {
                                Icon(FzIcons.ChevronDown, null, Modifier.size(18.dp))
                                Text(stringResource(Res.string.shutter_down), maxLines = 1, overflow = TextOverflow.Ellipsis)
                            }
                        }
                    }
                }
            }
        }
    }
}

/** The shutter, large: a drag moves its edge under the finger, a tap sets it there; its state is written on it. */
@Composable
private fun ShutterControl(open: () -> Float, label: String, moving: Boolean, enabled: Boolean, onDraft: (Int?) -> Unit, onSet: (Int) -> Unit) {
    val draftTo by rememberUpdatedState(onDraft)
    val setTo by rememberUpdatedState(onSet)
    // One gesture: the edge follows the finger from the touch on, the position is sent when it lifts (a
    // tap included). Every change is consumed, or the list around would scroll.
    val touch = remember(enabled) {
        Modifier.pointerInput(enabled) {
            if (!enabled) return@pointerInput
            fun at(y: Float) = ((1f - y / size.height) * 100).toInt().coerceIn(0, 100)
            awaitEachGesture {
                val down = awaitFirstDown()
                down.consume()
                var value = at(down.position.y)
                draftTo(value)
                while (true) {
                    val change = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                    change.consume()
                    if (!change.pressed) {
                        setTo(value)
                        return@awaitEachGesture
                    }
                    value = at(change.position.y)
                    draftTo(value)
                }
                draftTo(null)
            }
        }
    }
    Box(Modifier.fillMaxWidth().height(240.dp).semantics { stateDescription = label }.then(touch)) {
        ShutterWindow(open, Modifier.fillMaxSize())
        Text(
            label,
            style = MaterialTheme.typography.labelMedium,
            fontWeight = FontWeight.SemiBold,
            color = if (moving) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.align(Alignment.BottomEnd).padding(8.dp).clip(RoundedCornerShape(50)).background(Fz.colors.card.copy(alpha = 0.85f)).padding(horizontal = 10.dp, vertical = 4.dp),
        )
    }
}
