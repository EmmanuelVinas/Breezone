package com.breezone.app.ui.screens

import com.breezone.app.data.BlockTypes
import com.breezone.app.data.MobilePages
import com.breezone.app.data.fixedBlock
import com.breezone.app.ui.home.HomeNav
import com.breezone.app.ui.home.PageBoard
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonPrimitive
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.DeviceHistory
import com.breezone.app.data.DeviceView
import com.breezone.app.data.Hub
import com.breezone.app.data.Periods
import com.breezone.app.data.ServerDevices
import com.breezone.app.data.TraitIds
import com.breezone.app.data.VentilationRoomProfile
import com.breezone.app.data.VentilationRoomState
import com.breezone.app.data.VentilationUnitMainPollutant
import com.breezone.app.data.VentilationUnitState
import com.breezone.app.resources.*
import com.breezone.app.ui.components.CommandIndicator
import com.breezone.app.ui.components.DeviceGroupCard
import com.breezone.app.ui.components.IconBadge
import com.breezone.app.ui.components.PeriodChips
import com.breezone.app.ui.components.Sparkline
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.theme.Fz
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.jetbrains.compose.resources.pluralStringResource
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject
import kotlin.math.roundToInt

private enum class Quality { Ok, Warn, Bad }

private fun DeviceView.valve() = stateAs(VentilationRoomState.serializer()) ?: VentilationRoomState()

/** How the unit judges the room, with its app's thresholds (up to 40 good, above 100 bad); without its index, the room's own measures (as on the web). */
private fun VentilationRoomState.quality(): Quality? {
    airQualityIndex?.let { return if (it > 100) Quality.Bad else if (it > 40) Quality.Warn else Quality.Ok }
    val levels = listOfNotNull(
        humidity?.let { if (it > 75) Quality.Bad else if (it > 60) Quality.Warn else Quality.Ok },
        co2?.let { if (it > 1200) Quality.Bad else if (it > 800) Quality.Warn else Quality.Ok },
    )
    return levels.maxByOrNull { it.ordinal }
}

@Composable
private fun Quality.color(): Color = when (this) {
    Quality.Ok -> Fz.colors.success
    Quality.Warn -> Fz.colors.warning
    Quality.Bad -> MaterialTheme.colorScheme.error
}

@Composable
private fun causeText(p: VentilationUnitMainPollutant?): String = stringResource(
    when (p) {
        VentilationUnitMainPollutant.Humidity -> Res.string.ventilation_cause_humidity
        VentilationUnitMainPollutant.Co2 -> Res.string.ventilation_cause_co2
        VentilationUnitMainPollutant.Voc -> Res.string.ventilation_cause_voc
        else -> Res.string.ventilation_cause_other
    },
)

@Composable
private fun profileLabel(p: VentilationRoomProfile): String = stringResource(
    when (p) {
        VentilationRoomProfile.Eco -> Res.string.ventilation_profile_eco
        VentilationRoomProfile.Health -> Res.string.ventilation_profile_health
        VentilationRoomProfile.Intense -> Res.string.ventilation_profile_intense
    },
)

private fun Hub.boost(serverId: String, d: DeviceView, trait: String, minutes: Int) =
    act(serverId, d, trait, "boost", buildJsonObject { put("minutes", minutes) }, optimistic = buildJsonObject { put("boost", true) })

private fun Hub.stopBoost(serverId: String, d: DeviceView, trait: String) =
    act(serverId, d, trait, "stop_boost", JsonObject(emptyMap()), optimistic = buildJsonObject { put("boost", false) })

@Composable
private fun LevelBar(level: Int, color: Color = Fz.colors.air) {
    Box(Modifier.fillMaxWidth().height(8.dp).clip(CircleShape).background(MaterialTheme.colorScheme.surfaceVariant)) {
        Box(Modifier.fillMaxHeight().fillMaxWidth(level.coerceIn(0, 100) / 100f).clip(CircleShape).background(color))
    }
}

@Composable
private fun rememberHistory(hub: Hub, serverId: String, deviceId: Long, hours: Int = Periods.DAY): DeviceHistory? {
    var history by remember(deviceId) { mutableStateOf<DeviceHistory?>(null) }
    LaunchedEffect(serverId, deviceId, hours) {
        history = runCatching { hub.api(serverId)?.deviceHistoryOver(deviceId, hours) }.getOrNull() ?: history
    }
    return history
}

/** The valves of the ventilation unit, each with its air: one opens its room. */
@Composable
internal fun ValvesCard(data: ServerDevices, onOpen: (Long) -> Unit) {
    val unit = data.withTrait(TraitIds.VENTILATION_UNIT).firstOrNull() ?: return
    val valves = data.withTrait(TraitIds.VENTILATION_ROOM).filter { it.sourceId == unit.sourceId }
    val u = unit.stateAs(VentilationUnitState.serializer()) ?: VentilationUnitState()
    DeviceGroupCard(stringResource(Res.string.ventilation_rooms), null) {
        valves.forEach { d -> ValveRow(d, demanding = u.worstRoom == d.name, cause = causeText(u.mainPollutant)) { onOpen(d.id) } }
    }
}

@Composable
fun VentilationScreen(serverId: String, installationId: Long?, nav: HomeNav, onBack: (() -> Unit)?) {
    val hub = koinInject<Hub>()
    val all by hub.devices.collectAsState()
    val status by hub.deviceStatus.collectAsState()
    val data = all[serverId] ?: ServerDevices()
    val unit = data.withTrait(TraitIds.VENTILATION_UNIT).firstOrNull()
    val valves = data.withTrait(TraitIds.VENTILATION_ROOM).filter { unit == null || it.sourceId == unit.sourceId }
    PageBoard(
        serverId = serverId,
        installationId = installationId,
        page = MobilePages.VENTILATION,
        title = stringResource(Res.string.house_ventilation),
        subtitle = pluralStringResource(Res.plurals.ventilation_valves, valves.size, valves.size),
        onBack = onBack,
        nav = nav,
        everyMs = 15_000,
        generated = { listOf(fixedBlock("house", BlockTypes.VENTILATION_HOUSE), fixedBlock("valves", BlockTypes.VENTILATION_VALVES)) },
        actions = { unit?.let { CommandIndicator(status["$serverId/${it.id}"], Modifier.padding(end = 4.dp)) } },
        header = { if (data.loaded && unit == null) item(key = "empty", contentType = "note") { SectionCard { Muted(stringResource(Res.string.ventilation_empty)) } } },
    )
}

@Composable
internal fun ValveRow(d: DeviceView, demanding: Boolean, cause: String, onClick: () -> Unit) {
    val s = d.valve()
    val q = s.quality()
    val color = q?.color() ?: if (demanding) Fz.colors.warning else Fz.colors.air
    val line = listOfNotNull(
        when {
            s.boost -> stringResource(Res.string.ventilation_boosting)
            q != null && q != Quality.Ok && s.humidity != null -> stringResource(Res.string.ventilation_humidity_value, s.humidity.toInt())
            demanding -> stringResource(Res.string.ventilation_asks_air, cause)
            else -> null
        },
        s.level?.let { stringResource(Res.string.ventilation_flow_level, it) },
    ).joinToString(" · ")
    Row(Modifier.fillMaxWidth().clickable(onClick = onClick).padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        IconBadge(FzIcons.Fan, color)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(d.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(line, fontSize = 12.sp, color = if (demanding || (q != null && q != Quality.Ok)) color else Fz.colors.muted, maxLines = 1)
        }
        Icon(FzIcons.ChevronRight, null, Modifier.size(18.dp), tint = Fz.colors.muted)
    }
}

@Composable
fun VentilationRoomScreen(serverId: String, deviceId: Long, onBack: () -> Unit) {
    val hub = koinInject<Hub>()
    val all by hub.devices.collectAsState()
    val status by hub.deviceStatus.collectAsState()
    LaunchedEffect(serverId) { hub.followDevices(serverId, everyMs = 15_000) }
    val data = all[serverId] ?: ServerDevices()
    val d = data.devices.firstOrNull { it.id == deviceId }
    val unit = data.withTrait(TraitIds.VENTILATION_UNIT).firstOrNull()?.stateAs(VentilationUnitState.serializer())
    var hours by remember { mutableIntStateOf(Periods.DAY) }
    val history = rememberHistory(hub, serverId, deviceId, hours)
    val room = data.rooms.firstOrNull { it.id == d?.roomId }
    Scaffold(topBar = {
        FzTopBar(d?.name ?: stringResource(Res.string.house_ventilation), room?.let { stringResource(Res.string.ventilation_breezone_room, it.name) }, onBack) {
            CommandIndicator(status["$serverId/$deviceId"], Modifier.padding(end = 16.dp))
        }
    }) { padding ->
        if (d == null) return@Scaffold
        val s = d.valve()
        val q = s.quality()
        val demanding = unit?.worstRoom == d.name
        val sentence = when {
            s.boost -> stringResource(Res.string.ventilation_boost_left, ((s.boostRemaining ?: 0) / 60).coerceAtLeast(1))
            q == Quality.Bad -> stringResource(Res.string.ventilation_bad_air)
            demanding -> stringResource(Res.string.ventilation_most_demand, causeText(unit.mainPollutant))
            (s.level ?: 0) > 50 -> stringResource(Res.string.ventilation_active)
            else -> stringResource(Res.string.ventilation_rest)
        }
        val co2 = stringResource(Res.string.ventilation_co2)
        val voc = stringResource(Res.string.ventilation_voc)
        val others = listOfNotNull(
            s.co2?.let { co2 to "${it.toInt()} ppm" },
            s.voc?.let { voc to (if (it < 10) "${(it * 10).roundToInt() / 10.0} ppm" else "${it.roundToInt()} ppm") },
        )
        FzPullRefresh({ hub.reload(serverId) }, Modifier.fillMaxSize(), top = padding.calculateTopPadding()) {
            LazyColumn(
                Modifier.fillMaxSize(),
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = padding.calculateTopPadding() + 4.dp, bottom = 24.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                item(key = "sentence", contentType = "sentence") {
                    SectionCard { Text(sentence, fontSize = 20.sp, fontWeight = FontWeight.SemiBold, lineHeight = 26.sp) }
                }
                item(key = "periods", contentType = "periods") {
                    PeriodChips(hours, history?.oldest) { hours = it }
                }
                item(key = "flow", contentType = "measure") {
                    SectionCard {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Icon(FzIcons.Fan, null, Modifier.size(18.dp), tint = Fz.colors.air)
                            Spacer(Modifier.width(8.dp))
                            Text(stringResource(Res.string.ventilation_air_flow), fontWeight = FontWeight.Medium, modifier = Modifier.weight(1f))
                            Text(s.level?.let { stringResource(Res.string.common_percent, it.toInt()) } ?: "–", fontWeight = FontWeight.SemiBold)
                        }
                        Caption(
                            if (s.nominal != null) stringResource(Res.string.ventilation_flow_of, s.flow.toInt(), s.nominal.toInt())
                            else stringResource(Res.string.ventilation_flow, s.flow.toInt()),
                        )
                        history?.series?.get("flow")?.takeIf { it.size > 1 }?.let { Sparkline(it, Fz.colors.air, Modifier.fillMaxWidth().height(72.dp)) }
                    }
                }
                if (s.humidity != null) {
                    item(key = "humidity", contentType = "measure") {
                        SectionCard {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Icon(FzIcons.Droplet, null, Modifier.size(18.dp), tint = Fz.colors.cool)
                                Spacer(Modifier.width(8.dp))
                                Text(stringResource(Res.string.ventilation_humidity), fontWeight = FontWeight.Medium, modifier = Modifier.weight(1f))
                                Text(stringResource(Res.string.common_percent, s.humidity.toInt()), fontWeight = FontWeight.SemiBold, color = q?.color() ?: MaterialTheme.colorScheme.onSurface)
                            }
                            history?.series?.get("humidity")?.takeIf { it.size > 1 }?.let { Sparkline(it, Fz.colors.cool, Modifier.fillMaxWidth().height(72.dp)) }
                        }
                    }
                }
                if (others.isNotEmpty()) {
                    item(key = "others", contentType = "measure") {
                        SectionCard {
                            others.forEach { (k, v) ->
                                Row(Modifier.fillMaxWidth()) {
                                    Muted(k, Modifier.weight(1f))
                                    Text(v, fontWeight = FontWeight.Medium)
                                }
                            }
                        }
                    }
                }
                item(key = "actions", contentType = "actions") {
                    SectionCard {
                        if (s.boost) {
                            OutlinedButton(onClick = { hub.stopBoost(serverId, d, "ventilation.room") }, enabled = d.usable, shape = Fz.shapes.button, modifier = Modifier.fillMaxWidth()) {
                                Icon(FzIcons.Square, null, Modifier.size(14.dp))
                                Spacer(Modifier.width(6.dp))
                                Text(stringResource(Res.string.ventilation_stop_boost))
                            }
                        } else {
                            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                Button(onClick = { hub.boost(serverId, d, "ventilation.room", 30) }, enabled = d.usable, shape = Fz.shapes.button, modifier = Modifier.weight(1f)) {
                                    Icon(FzIcons.Fan, null, Modifier.size(18.dp))
                                    Spacer(Modifier.width(6.dp))
                                    Text(stringResource(Res.string.ventilation_boost_30))
                                }
                                OutlinedButton(onClick = { hub.boost(serverId, d, "ventilation.room", 60) }, enabled = d.usable, shape = Fz.shapes.button) { Text(stringResource(Res.string.ventilation_1h)) }
                                OutlinedButton(onClick = { hub.boost(serverId, d, "ventilation.room", 120) }, enabled = d.usable, shape = Fz.shapes.button) { Text(stringResource(Res.string.ventilation_2h)) }
                            }
                        }
                        Caption(stringResource(Res.string.ventilation_profile))
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            VentilationRoomProfile.entries.forEach { p ->
                                FilterChip(
                                    selected = s.profile == p,
                                    enabled = d.usable,
                                    onClick = {
                                        if (s.profile != p) {
                                            val value = p.name.lowercase()
                                            hub.act(serverId, d, "ventilation.room", "set_profile", buildJsonObject { put("profile", value) }, optimistic = buildJsonObject { put("profile", value) })
                                        }
                                    },
                                    label = { Text(profileLabel(p)) },
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

/** The level of the house: the air flow of every valve over their nominal flows. */
fun ventilationLevel(rooms: List<VentilationRoomState>): Int? {
    val nominal = rooms.sumOf { it.nominal ?: 0.0 }
    if (nominal <= 0) return null
    return (rooms.sumOf { it.flow } / nominal * 100).toInt()
}

/** The house as its ventilation unit sees it: its level, its flow, what asks for air, and a boost. */
@Composable
internal fun VentilationHouseCard(serverId: String, unit: DeviceView, valves: List<DeviceView>) {
    val hub = koinInject<Hub>()
    val u = unit.stateAs(VentilationUnitState.serializer()) ?: VentilationUnitState()
    val level = ventilationLevel(valves.map { it.valve() })
    val flow = rememberHistory(hub, serverId, unit.id)?.series?.get("flow").orEmpty()
    SectionCard {
        Caption(stringResource(Res.string.ventilation_level))
        Text(level?.let { stringResource(Res.string.common_percent, it) } ?: "–", fontSize = 40.sp, fontWeight = FontWeight.SemiBold)
        Caption(
            listOfNotNull(
                stringResource(Res.string.ventilation_flow, u.flow.toInt()),
                u.power?.let { "${it.toInt()} W" },
                stringResource(if (u.boost) Res.string.ventilation_boosting else Res.string.ventilation_auto),
            ).joinToString(" · "),
        )
        u.worstRoom?.let { Text(stringResource(Res.string.ventilation_worst, it, causeText(u.mainPollutant)), fontSize = 14.sp) }
        if (!u.sensors) Caption(stringResource(Res.string.ventilation_no_key))
        LevelBar(level ?: 0)
        if (flow.size > 1) Sparkline(flow, Fz.colors.air, Modifier.fillMaxWidth().height(56.dp))
        if (u.boost) {
            OutlinedButton(onClick = { hub.stopBoost(serverId, unit, "ventilation.unit") }, enabled = unit.usable, shape = Fz.shapes.button) {
                Icon(FzIcons.Square, null, Modifier.size(14.dp))
                Spacer(Modifier.width(6.dp))
                Text(stringResource(Res.string.ventilation_stop_boosts))
            }
        } else {
            Button(onClick = { hub.boost(serverId, unit, "ventilation.unit", 60) }, enabled = unit.usable, shape = Fz.shapes.button) {
                Icon(FzIcons.Fan, null, Modifier.size(18.dp))
                Spacer(Modifier.width(6.dp))
                Text(stringResource(Res.string.ventilation_boost_house))
            }
        }
    }
}
