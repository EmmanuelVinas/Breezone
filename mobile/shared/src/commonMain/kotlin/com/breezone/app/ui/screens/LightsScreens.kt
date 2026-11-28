package com.breezone.app.ui.screens

import com.breezone.app.data.Room
import com.breezone.app.data.BlockTypes
import com.breezone.app.data.MobilePages
import com.breezone.app.data.fixedBlock
import com.breezone.app.ui.home.HomeNav
import com.breezone.app.ui.home.PageBoard
import kotlinx.serialization.json.JsonNull
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
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
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Switch
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
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.CommandStatus
import com.breezone.app.data.DeviceView
import com.breezone.app.data.Hub
import com.breezone.app.data.LightMode
import com.breezone.app.data.LightState
import com.breezone.app.data.ServerDevices
import com.breezone.app.data.TraitIds
import com.breezone.app.resources.*
import com.breezone.app.ui.components.CommandIndicator
import com.breezone.app.ui.components.DeviceGroupCard
import com.breezone.app.ui.components.FzSlider
import com.breezone.app.ui.components.IconBadge
import com.breezone.app.ui.components.groupByRoom
import com.breezone.app.ui.components.hexColor
import com.breezone.app.ui.components.kelvinColor
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.theme.Fz
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.jetbrains.compose.resources.pluralStringResource
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject

/** Colors offered to the lights that have colors (as on the web). */
private val SWATCHES = listOf("#ffb35c", "#ffd59a", "#fff3e0", "#e7f0ff", "#ff6b6b", "#ffa94d", "#69db7c", "#4dabf7", "#9775fa", "#f783ac")

private fun LightState.glow(fallback: Color): Color = when {
    mode == LightMode.Color -> hexColor(color) ?: fallback
    colorTemp != null -> kelvinColor(colorTemp)
    else -> fallback
}

@Composable
private fun describe(s: LightState): String = when {
    s.reachable == false -> stringResource(Res.string.light_unreachable)
    !s.on -> stringResource(Res.string.light_off)
    else -> listOfNotNull(
        s.brightness?.let { stringResource(Res.string.common_percent, it.toInt()) },
        when {
            s.mode == LightMode.Color -> stringResource(Res.string.light_color)
            s.colorTemp == null -> null
            s.colorTemp < 3300 -> stringResource(Res.string.light_warm)
            s.colorTemp < 5000 -> stringResource(Res.string.light_neutral)
            else -> stringResource(Res.string.light_cool)
        },
    ).joinToString(" · ").ifEmpty { stringResource(Res.string.light_on) }
}

/** Sets a light: what is asked shows at once. */
internal fun Hub.setLight(serverId: String, d: DeviceView, params: JsonObject) = act(serverId, d, "light", "set", params, optimistic = params)

/** The scenes of the lights, and all of them off at once. */
@Composable
internal fun LightScenesRow(serverId: String, data: ServerDevices) {
    val hub = koinInject<Hub>()
    val on = data.withTrait(TraitIds.LIGHT).filter { it.available && it.stateAs(LightState.serializer())?.on == true }
    LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        items(data.scenes, key = { "${it.sourceId}/${it.id}" }) { scene ->
            AssistChip(
                onClick = { hub.activateScene(serverId, scene) },
                label = { Text(scene.name) },
                leadingIcon = { Icon(FzIcons.Palette, null, Modifier.size(16.dp)) },
            )
        }
        item(key = "all-off") {
            AssistChip(
                onClick = { on.filter { it.usable }.forEach { hub.setLight(serverId, it, buildJsonObject { put("on", false) }) } },
                enabled = on.any { it.usable },
                label = { Text(stringResource(Res.string.lights_all_off)) },
                leadingIcon = { Icon(FzIcons.Power, null, Modifier.size(16.dp)) },
            )
        }
    }
}

/** The lights of a room (`null`: without a room): all on or off, and each one. */
@Composable
internal fun LightRoomCard(serverId: String, room: Room?, list: List<DeviceView>, onOpen: (Long) -> Unit) {
    val hub = koinInject<Hub>()
    val status by hub.deviceStatus.collectAsState()
    val lit = list.count { it.available && it.stateAs(LightState.serializer())?.on == true }
    // The whole room at once: on when any of its lights is.
    val reachable = list.filter { it.usable && it.stateAs(LightState.serializer())?.reachable != false }
    DeviceGroupCard(
        room?.name ?: stringResource(Res.string.room_none),
        pluralStringResource(Res.plurals.lights_on_of, lit, lit, list.size),
        trailing = {
            Switch(
                checked = lit > 0,
                enabled = reachable.isNotEmpty(),
                onCheckedChange = { on -> reachable.forEach { hub.setLight(serverId, it, buildJsonObject { put("on", on) }) } },
                modifier = Modifier.padding(start = 10.dp),
            )
        },
    ) {
        list.forEach { d -> LightRow(d, status["$serverId/${d.id}"], onOpen = { onOpen(d.id) }, onSet = { hub.setLight(serverId, d, it) }) }
    }
}

@Composable
fun LightsScreen(serverId: String, installationId: Long?, nav: HomeNav, onBack: (() -> Unit)?) {
    val hub = koinInject<Hub>()
    val all by hub.devices.collectAsState()
    val data = all[serverId] ?: ServerDevices()
    val lights = data.withTrait(TraitIds.LIGHT)
    val on = lights.filter { it.available && it.stateAs(LightState.serializer())?.on == true }
    PageBoard(
        serverId = serverId,
        installationId = installationId,
        page = MobilePages.LIGHTS,
        title = stringResource(Res.string.house_lights),
        subtitle = pluralStringResource(Res.plurals.lights_on_of, on.size, on.size, lights.size),
        onBack = onBack,
        nav = nav,
        generated = { ctx ->
            val list = ctx.server.withTrait(TraitIds.LIGHT)
            listOf(fixedBlock("scenes", BlockTypes.LIGHT_SCENES)) +
                groupByRoom(list, ctx.rooms).map { (room, _) -> fixedBlock("room-${room?.id}", BlockTypes.LIGHT_ROOM, config = mapOf("room_id" to (room?.id?.let(::JsonPrimitive) ?: JsonNull))) }
        },
        header = { if (data.loaded && lights.isEmpty()) item(key = "empty", contentType = "note") { SectionCard { Muted(stringResource(Res.string.lights_empty)) } } },
    )
}

@Composable
internal fun LightRow(d: DeviceView, status: CommandStatus?, onOpen: () -> Unit, onSet: (JsonObject) -> Unit) {
    val s = d.stateAs(LightState.serializer()) ?: LightState()
    val lit = d.available && s.on
    Column(Modifier.fillMaxWidth().clickable(onClick = onOpen).padding(vertical = 8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconBadge(FzIcons.Lightbulb, s.glow(Fz.colors.light), active = lit)
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(d.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(if (d.available) describe(s) else stringResource(Res.string.device_unavailable), fontSize = 12.sp, color = Fz.colors.muted)
            }
            CommandIndicator(status, Modifier.padding(end = 6.dp))
            Switch(checked = lit, enabled = d.usable && s.reachable != false, onCheckedChange = { onSet(buildJsonObject { put("on", it) }) })
        }
        if (lit && s.brightness != null) {
            FzSlider(
                value = s.brightness.toFloat(),
                onCommit = { v -> onSet(buildJsonObject { put("brightness", v.toInt().coerceIn(1, 100)) }) },
                range = 1f..100f,
                colors = SliderDefaults.colors(thumbColor = Fz.colors.light, activeTrackColor = Fz.colors.light),
                modifier = Modifier.height(28.dp),
            )
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun LightScreen(serverId: String, deviceId: Long, onBack: () -> Unit) {
    val hub = koinInject<Hub>()
    val all by hub.devices.collectAsState()
    val status by hub.deviceStatus.collectAsState()
    LaunchedEffect(serverId) { hub.followDevices(serverId) }
    val data = all[serverId] ?: ServerDevices()
    val d = data.devices.firstOrNull { it.id == deviceId }
    val room = data.rooms.firstOrNull { it.id == d?.roomId }
    Scaffold(topBar = {
        FzTopBar(d?.name ?: stringResource(Res.string.house_lights), listOfNotNull(room?.name, d?.source?.let(::brandOf)).joinToString(" · ").ifEmpty { null }, onBack) {
            CommandIndicator(status["$serverId/$deviceId"], Modifier.padding(end = 16.dp))
        }
    }) { padding ->
        if (d == null) return@Scaffold
        val s = d.stateAs(LightState.serializer()) ?: LightState()
        val set: (JsonObject) -> Unit = { hub.setLight(serverId, d, it) }
        val glow = s.glow(Fz.colors.light)
        FzPullRefresh({ hub.reload(serverId) }, Modifier.fillMaxSize(), top = padding.calculateTopPadding()) {
            LazyColumn(
                Modifier.fillMaxSize(),
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = padding.calculateTopPadding() + 4.dp, bottom = 24.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                item(key = "main", contentType = "main") {
                    SectionCard {
                        Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(14.dp)) {
                            if (s.brightness != null) BrightnessBar(s.brightness.toInt(), s.on, glow, enabled = d.usable) { v ->
                                set(buildJsonObject { put("on", true); put("brightness", v) })
                            }
                            Button(
                                onClick = { set(buildJsonObject { put("on", !s.on) }) },
                                enabled = d.usable && s.reachable != false,
                                shape = Fz.shapes.button,
                                colors = if (s.on) ButtonDefaults.buttonColors() else ButtonDefaults.outlinedButtonColors(),
                                modifier = Modifier.width(200.dp),
                            ) {
                                Icon(FzIcons.Power, null, Modifier.size(18.dp))
                                Spacer(Modifier.width(8.dp))
                                Text(stringResource(if (s.on) Res.string.light_on else Res.string.light_off))
                            }
                        }
                    }
                }
                if (s.colorTemp != null || s.colorTempMin != null) {
                    item(key = "white", contentType = "white") {
                        SectionCard {
                            SectionTitle(stringResource(Res.string.light_white))
                            WhiteSlider(s, d.usable) { k -> set(buildJsonObject { put("on", true); put("color_temp", k) }) }
                        }
                    }
                }
                if (s.color != null) {
                    item(key = "colors", contentType = "colors") {
                        SectionCard {
                            SectionTitle(stringResource(Res.string.light_colors))
                            ColorSwatches(s, d.usable, 40.dp) { hex -> hub.setLightColor(serverId, d, hex) }
                        }
                    }
                }
            }
        }
    }
}

/** A light turned to a colour: its colour mode shows at once. */
internal fun Hub.setLightColor(serverId: String, d: DeviceView, hex: String) {
    val params = buildJsonObject { put("on", true); put("color", JsonPrimitive(hex)) }
    act(serverId, d, "light", "set", params, optimistic = JsonObject(params + ("mode" to JsonPrimitive("color"))))
}

/** The white of a light, from warm to cool, over its gradient. */
@Composable
internal fun WhiteSlider(s: LightState, enabled: Boolean, onSet: (Int) -> Unit) {
    val min = (s.colorTempMin ?: 2000).toFloat()
    val max = (s.colorTempMax ?: 6500).toFloat().coerceAtLeast(min + 100)
    val k = s.colorTemp?.toFloat() ?: ((min + max) / 2)
    Box(
        Modifier.fillMaxWidth().height(18.dp).clip(CircleShape)
            .background(Brush.horizontalGradient(listOf(kelvinColor(min.toLong()), kelvinColor(((min + max) / 2).toLong()), kelvinColor(max.toLong())))),
    )
    FzSlider(
        value = k.coerceIn(min, max),
        onCommit = { v -> onSet(v.toInt()) },
        range = min..max,
        enabled = enabled,
        colors = SliderDefaults.colors(thumbColor = kelvinColor(k.toLong()), activeTrackColor = Color.Transparent, inactiveTrackColor = Color.Transparent),
    )
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
        Caption(stringResource(Res.string.light_warm_short))
        Caption(stringResource(Res.string.light_kelvin, k.toInt()))
        Caption(stringResource(Res.string.light_cool_short))
    }
}

/** The colours offered, the light's own one ringed. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun ColorSwatches(s: LightState, enabled: Boolean, size: Dp, onPick: (String) -> Unit) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        SWATCHES.forEach { hex ->
            val chosen = s.mode == LightMode.Color && s.color.equals(hex, ignoreCase = true)
            Box(
                Modifier.size(size).clip(CircleShape).background(hexColor(hex)!!)
                    .then(if (chosen) Modifier.border(3.dp, MaterialTheme.colorScheme.onSurface, CircleShape) else Modifier)
                    .clickable(enabled = enabled) { onPick(hex) },
            )
        }
    }
}

/**
 * One light, or the lights of a room together, as on a light's page: its brightness bar, its
 * white and its colours (both can be left out). A room shows its first lit light and sets them all.
 */
@Composable
fun LightPanel(serverId: String, lights: List<DeviceView>, title: String, subtitle: String?, temperature: Boolean, colors: Boolean, modifier: Modifier = Modifier) {
    val hub = koinInject<Hub>()
    val usable = lights.filter { it.usable && it.stateAs(LightState.serializer())?.reachable != false }
    val lit = lights.filter { it.available && it.stateAs(LightState.serializer())?.on == true }
    val s = (lit.firstOrNull() ?: lights.firstOrNull())?.stateAs(LightState.serializer()) ?: LightState()
    val on = lit.isNotEmpty()
    val glow = s.glow(Fz.colors.light)
    val withWhite = usable.filter { it.stateAs(LightState.serializer())?.let { l -> l.colorTemp != null || l.colorTempMin != null } == true }
    val withColor = usable.filter { it.stateAs(LightState.serializer())?.color != null }
    val dimmable = usable.filter { it.stateAs(LightState.serializer())?.brightness != null }
    SectionCard(modifier) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(title, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                subtitle?.let { Caption(it, maxLines = 1) }
            }
            Switch(checked = on, enabled = usable.isNotEmpty(), onCheckedChange = { v -> usable.forEach { hub.setLight(serverId, it, buildJsonObject { put("on", v) }) } })
        }
        if (dimmable.isNotEmpty()) {
            Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                BrightnessBar(s.brightness?.toInt() ?: 100, on, glow, enabled = true, width = 110.dp, height = 210.dp) { v ->
                    dimmable.forEach { hub.setLight(serverId, it, buildJsonObject { put("on", true); put("brightness", v) }) }
                }
            }
        }
        if (temperature && withWhite.isNotEmpty()) {
            WhiteSlider(s, true) { k -> withWhite.forEach { hub.setLight(serverId, it, buildJsonObject { put("on", true); put("color_temp", k) }) } }
        }
        if (colors && withColor.isNotEmpty()) {
            ColorSwatches(s, true, 32.dp) { hex -> withColor.forEach { hub.setLightColor(serverId, it, hex) } }
        }
    }
}

/** The big brightness bar: filled with the light's color; drag or tap to set it. */
@Composable
private fun BrightnessBar(value: Int, on: Boolean, color: Color, enabled: Boolean, width: Dp = 140.dp, height: Dp = 300.dp, onSet: (Int) -> Unit) {
    var draft by remember { mutableStateOf<Int?>(null) }
    val lit = on || draft != null
    val shown = if (lit) draft ?: value else 0
    fun at(y: Float, height: Int) = ((1f - y / height) * 100).toInt().coerceIn(1, 100)
    Box(
        Modifier
            .width(width)
            .height(height)
            .clip(RoundedCornerShape(width * 0.31f))
            .background(MaterialTheme.colorScheme.surfaceVariant)
            .pointerInput(enabled) {
                if (!enabled) return@pointerInput
                detectTapGestures { onSet(at(it.y, size.height)) }
            }
            .pointerInput(enabled) {
                if (!enabled) return@pointerInput
                detectDragGestures(
                    onDragEnd = { draft?.let(onSet); draft = null },
                    onDragCancel = { draft = null },
                ) { change, _ -> draft = at(change.position.y, size.height) }
            },
        contentAlignment = Alignment.BottomCenter,
    ) {
        Box(Modifier.fillMaxWidth().fillMaxHeight(shown / 100f).background(Brush.verticalGradient(listOf(color, color.copy(alpha = 0.8f)))))
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            if (lit) {
                Text(stringResource(Res.string.common_percent, shown), fontSize = 32.sp, fontWeight = FontWeight.Bold, color = if (shown > 56) Color.Black.copy(alpha = 0.75f) else MaterialTheme.colorScheme.onSurface)
            } else {
                Icon(FzIcons.LightbulbOff, stringResource(Res.string.light_off), Modifier.size(40.dp), tint = Fz.colors.muted)
            }
        }
    }
}

/** The maker of a module's device, from its driver. */
fun brandOf(source: String): String? = when (source) {
    "hue" -> "Philips Hue"
    "homekit" -> "HomeKit"
    "shelly" -> "Shelly"
    "renson" -> "Renson"
    "trydan" -> "V2C Trydan"
    "hoymiles" -> "Hoymiles"
    "anker", "anker_cloud" -> "Anker SOLIX"
    "deye" -> "Deye"
    "tydom" -> "Delta Dore Tydom"
    else -> null
}
