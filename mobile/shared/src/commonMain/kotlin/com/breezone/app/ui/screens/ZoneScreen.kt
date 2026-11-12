package com.breezone.app.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
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
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.AirQuality
import com.breezone.app.data.DayStats
import com.breezone.app.data.Hub
import com.breezone.app.data.installation
import com.breezone.app.data.zone
import com.breezone.app.data.Mode
import com.breezone.app.data.Reading
import com.breezone.app.data.Zone
import com.breezone.app.data.ZoneCommand
import com.breezone.app.resources.*
import com.breezone.app.ui.color
import com.breezone.app.ui.airQualityColor
import com.breezone.app.ui.airQualityLabel
import com.breezone.app.ui.components.AirQualityDaysChart
import com.breezone.app.ui.components.DailyComfortChart
import com.breezone.app.ui.components.DailyTemperatureChart
import com.breezone.app.ui.components.HistoryChart
import com.breezone.app.ui.components.HistorySeries
import com.breezone.app.ui.components.ZoneState
import com.breezone.app.ui.components.labelRes
import com.breezone.app.ui.components.zoneStates
import com.breezone.app.ui.components.SetpointDial
import com.breezone.app.ui.components.rememberSetpointDraft
import com.breezone.app.ui.formatTemp
import com.breezone.app.ui.icon
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.label
import com.breezone.app.ui.theme.Fz
import com.breezone.app.ui.theme.TempStyle
import com.breezone.app.ui.tone
import kotlinx.coroutines.delay
import kotlin.math.roundToInt
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject

@Composable
fun ZoneScreen(serverId: String, installationId: Long, systemId: Long, zoneId: Long, onBack: () -> Unit) {
    val hub = koinInject<Hub>()
    val servers by hub.servers.collectAsState()
    val zone = servers.installation(serverId, installationId)?.zone(systemId, zoneId)

    Scaffold(topBar = { FzTopBar(zone?.name ?: stringResource(Res.string.zone_title), zone?.group, onBack) }) { padding ->
        if (zone == null) {
            Muted(stringResource(Res.string.zone_not_found), Modifier.padding(padding).padding(24.dp))
            return@Scaffold
        }
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            ZoneControls(zone) { cmd -> hub.control(serverId, zone, cmd) }
            if (zone.modes.isNotEmpty()) {
                var pendingMode by remember { mutableStateOf<Mode?>(null) }
                ModeCard(zone) { pendingMode = it }
                pendingMode?.let { mode ->
                    ModeConfirmation(mode, onConfirm = { hub.control(serverId, zone, ZoneCommand(mode = mode)) }, onDismiss = { pendingMode = null })
                }
            }
            if (zone.speeds.isNotEmpty()) {
                val auto = stringResource(Res.string.common_auto)
                SectionCard {
                    SectionTitle(stringResource(Res.string.zone_fan_title), FzIcons.Fan)
                    Segments(zone.speeds.map { it to (if (it == 0L) auto else "$it") }, zone.speed) {
                        hub.control(serverId, zone, ZoneCommand(speed = it))
                    }
                }
            }
            if (zone.sleep != null) {
                val off = stringResource(Res.string.zone_sleep_off)
                val minutes = listOf(30L, 60L, 90L).map { it to stringResource(Res.string.zone_sleep_minutes, it.toInt()) }
                SectionCard {
                    SectionTitle(stringResource(Res.string.zone_sleep_title), FzIcons.Moon)
                    Segments(listOf(0L to off) + minutes, zone.sleep) {
                        hub.control(serverId, zone, ZoneCommand(sleep = it))
                    }
                }
            }
            if (zone.errors.isNotEmpty()) {
                SectionCard {
                    SectionTitle(stringResource(Res.string.zone_alerts_title), FzIcons.TriangleAlert)
                    Muted(zone.errors.joinToString(" · "))
                }
            }
            History(hub, serverId, zone)
            DailyComfort(hub, serverId, zone)
            zone.airQuality?.let { AirQualitySection(hub, serverId, zone, it) }
            Spacer(Modifier.height(24.dp))
        }
    }
}

@Composable
private fun ZoneControls(zone: Zone, send: (ZoneCommand) -> Unit) {
    val tone = zone.tone()
    val draft = rememberSetpointDraft(zone) { send(ZoneCommand(setpoint = it)) }
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(30.dp))
            .background(Brush.verticalGradient(listOf(tone.copy(alpha = 0.16f), Fz.colors.card)))
            .padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        SetpointDial(
            zone = zone,
            // Read in the dial's draw phase: dragging does not recompose this screen.
            value = { draft.value },
            color = if (zone.on) tone else MaterialTheme.colorScheme.primary,
            onChange = { draft.draft = it },
            onCommit = { v -> if (!v.isNaN()) draft.draft = v },
            modifier = Modifier.widthIn(max = 320.dp).fillMaxWidth(),
        )
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
            FilledTonalIconButton(onClick = { draft.nudge(-zone.step) }, modifier = Modifier.size(52.dp)) {
                Icon(FzIcons.Minus, stringResource(Res.string.common_lower))
            }
            Button(
                onClick = { send(ZoneCommand(on = !zone.on)) },
                modifier = Modifier.height(52.dp).widthIn(min = 150.dp),
                shape = RoundedCornerShape(18.dp),
                colors = if (zone.on) {
                    ButtonDefaults.buttonColors()
                } else {
                    ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.surfaceVariant, contentColor = MaterialTheme.colorScheme.onSurface)
                },
            ) {
                Icon(FzIcons.Power, null, Modifier.size(18.dp))
                Spacer(Modifier.size(8.dp))
                Text(stringResource(if (zone.on) Res.string.zone_on else Res.string.zone_off))
            }
            FilledTonalIconButton(onClick = { draft.nudge(zone.step) }, modifier = Modifier.size(52.dp)) {
                Icon(FzIcons.Plus, stringResource(Res.string.common_raise))
            }
        }
        Spacer(Modifier.height(16.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Tile(stringResource(Res.string.zone_temperature), formatTemp(zone.roomTemp), Modifier.weight(1f))
            Tile(
                stringResource(Res.string.zone_humidity_label),
                zone.humidity?.let { stringResource(Res.string.zone_humidity, it.toInt()) } ?: "–",
                Modifier.weight(1f),
            )
        }
    }
}

@Composable
private fun Tile(label: String, value: String, modifier: Modifier) {
    Column(modifier.clip(RoundedCornerShape(18.dp)).background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.6f)).padding(12.dp)) {
        Caption(label)
        Text(value, style = TempStyle, fontSize = 20.sp)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun ModeCard(zone: Zone, onMode: (Mode) -> Unit) {
    SectionCard {
        SectionTitle(stringResource(Res.string.zone_mode_title))
        Muted(stringResource(Res.string.zone_mode_hint))
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            zone.modes.forEach { mode ->
                val selected = zone.mode == mode
                val color = mode.color()
                Column(
                    Modifier
                        .size(width = 92.dp, height = 74.dp)
                        .clip(RoundedCornerShape(18.dp))
                        .background(if (selected) color.copy(alpha = 0.16f) else MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f))
                        .border(if (selected) 1.5.dp else 0.dp, if (selected) color else Color.Transparent, RoundedCornerShape(18.dp))
                        .clickable { if (!selected) onMode(mode) }
                        .padding(8.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                ) {
                    Icon(mode.icon, null, tint = if (selected) color else Fz.colors.muted)
                    Spacer(Modifier.height(4.dp))
                    Text(mode.label(), fontSize = 12.sp, color = if (selected) color else MaterialTheme.colorScheme.onSurface)
                }
            }
        }
    }
}

@Composable
private fun Segments(options: List<Pair<Long, String>>, selected: Long?, onSelect: (Long) -> Unit) {
    SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
        options.forEachIndexed { i, (value, label) ->
            SegmentedButton(
                selected = selected == value,
                onClick = { if (selected != value) onSelect(value) },
                shape = SegmentedButtonDefaults.itemShape(i, options.size),
                icon = {},
            ) { Text(label, fontSize = 13.sp) }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun History(hub: Hub, serverId: String, zone: Zone) {
    var hours by remember { mutableIntStateOf(24) }
    var readings by remember { mutableStateOf<List<Reading>?>(null) }
    var hidden by remember { mutableStateOf(emptySet<HistorySeries>()) }
    LaunchedEffect(zone.installationId, zone.systemId, zone.zoneId, hours) {
        while (true) {
            readings = runCatching { hub.api(serverId)?.history(zone, hours) }.getOrNull() ?: readings
            delay(60_000)
        }
    }
    SectionCard {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.weight(1f)) { SectionTitle(stringResource(Res.string.zone_history_title), FzIcons.Activity) }
            listOf(24 to Res.string.zone_history_24h, 168 to Res.string.zone_history_7d, 720 to Res.string.zone_comfort_30d).forEach { (h, label) ->
                FilterChip(selected = hours == h, onClick = { hours = h }, label = { Text(stringResource(label)) }, modifier = Modifier.padding(start = 6.dp))
            }
        }
        val data = readings
        when {
            data == null -> Muted(stringResource(Res.string.zone_history_loading))
            data.size < 2 -> Muted(stringResource(Res.string.zone_history_empty))
            else -> {
                Caption(stringResource(Res.string.zone_history_hint))
                HistoryChart(data, hours, hidden)
                val toggle = { s: HistorySeries -> hidden = if (s in hidden) hidden - s else hidden + s }
                val states = remember(data) { zoneStates(data).filter { it != null && it != ZoneState.Comfort }.toSet() }
                FlowRow(horizontalArrangement = Arrangement.spacedBy(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    val stateColors = mapOf(ZoneState.Heating to Fz.colors.heat, ZoneState.Cooling to Fz.colors.cool, ZoneState.Comfort to Fz.colors.success)
                    ZoneState.entries.filter { it in states }.forEach { state ->
                        Legend(stateColors.getValue(state).copy(alpha = 0.5f), stringResource(state.labelRes), HistorySeries.Periods !in hidden) {
                            toggle(HistorySeries.Periods)
                        }
                    }
                    Legend(Fz.colors.heat, stringResource(Res.string.zone_temperature), HistorySeries.Temperature !in hidden) { toggle(HistorySeries.Temperature) }
                    Legend(MaterialTheme.colorScheme.primary, stringResource(Res.string.zone_setpoint), HistorySeries.Setpoint !in hidden) { toggle(HistorySeries.Setpoint) }
                    if (data.any { it.humidity != null }) {
                        Legend(Fz.colors.cool, stringResource(Res.string.zone_humidity_label), HistorySeries.Humidity !in hidden) { toggle(HistorySeries.Humidity) }
                    }
                    if (zone.airQuality != null && data.any { (it.airQuality ?: 0) in 1..3 }) {
                        Legend(Fz.colors.success, stringResource(Res.string.air_quality_title), HistorySeries.Air !in hidden) { toggle(HistorySeries.Air) }
                    }
                }
                if (states.isNotEmpty() && data.none { it.demandSecs != null }) Caption(stringResource(Res.string.history_states_estimated))
            }
        }
    }
}

@Composable
private fun duration(secs: Long): String {
    val minutes = (secs + 30) / 60
    return if (minutes < 60) {
        stringResource(Res.string.zone_sleep_minutes, minutes.toInt())
    } else {
        stringResource(Res.string.zone_comfort_hours, (minutes / 60).toInt(), (minutes % 60).toString().padStart(2, '0'))
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun DailyComfort(hub: Hub, serverId: String, zone: Zone) {
    var days by remember { mutableIntStateOf(7) }
    var stats by remember { mutableStateOf<List<DayStats>?>(null) }
    LaunchedEffect(zone.installationId, zone.systemId, zone.zoneId, days) {
        while (true) {
            stats = runCatching { hub.api(serverId)?.daily(zone, days) }.getOrNull() ?: stats
            delay(300_000)
        }
    }
    SectionCard {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.weight(1f)) { SectionTitle(stringResource(Res.string.zone_comfort_title), FzIcons.Thermometer) }
            listOf(7 to Res.string.zone_comfort_7d, 30 to Res.string.zone_comfort_30d).forEach { (d, label) ->
                FilterChip(selected = days == d, onClick = { days = d }, label = { Text(stringResource(label)) }, modifier = Modifier.padding(start = 6.dp))
            }
        }
        Caption(stringResource(Res.string.zone_comfort_hint))
        val data = stats
        when {
            data == null -> Muted(stringResource(Res.string.zone_history_loading))
            data.isEmpty() -> Muted(stringResource(Res.string.zone_history_empty))
            else -> {
                val on = data.sumOf { it.onSecs }
                val comfortable = data.sumOf { it.comfortSecs }
                val withDemand = data.filter { it.demandSecs != null }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Tile(
                        stringResource(Res.string.zone_comfort_share),
                        if (on > 0) stringResource(Res.string.zone_comfort_percent, (comfortable * 100.0 / on).roundToInt()) else "–",
                        Modifier.weight(1f),
                    )
                    Tile(stringResource(Res.string.zone_comfort_on), duration(on / data.size), Modifier.weight(1f))
                    if (withDemand.isNotEmpty()) {
                        Tile(stringResource(Res.string.zone_comfort_demand), duration(withDemand.sumOf { it.demandSecs ?: 0 } / withDemand.size), Modifier.weight(1f))
                    }
                }
                DailyTemperatureChart(data)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                    Legend(Fz.colors.heat.copy(alpha = 0.4f), stringResource(Res.string.zone_comfort_legend_range))
                    Legend(Fz.colors.heat, stringResource(Res.string.zone_comfort_legend_avg))
                    if (data.any { it.avgHumidity != null }) Legend(Fz.colors.cool, stringResource(Res.string.zone_humidity_label))
                }
                DailyComfortChart(data)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                    Legend(MaterialTheme.colorScheme.primary, stringResource(Res.string.zone_comfort_share))
                    if (withDemand.isNotEmpty()) Legend(Fz.colors.heat, stringResource(Res.string.zone_demand))
                }
            }
        }
    }
}

@Composable
private fun AirQualitySection(hub: Hub, serverId: String, zone: Zone, air: AirQuality) {
    var days by remember { mutableIntStateOf(7) }
    var stats by remember { mutableStateOf<List<DayStats>?>(null) }
    LaunchedEffect(zone.installationId, zone.systemId, zone.zoneId, days) {
        while (true) {
            stats = runCatching { hub.api(serverId)?.daily(zone, days) }.getOrNull() ?: stats
            delay(300_000)
        }
    }
    // Shown only when the sensor measures something: now, or in the last 30 days.
    var measuredThisMonth by remember(zone.installationId, zone.systemId, zone.zoneId) { mutableStateOf(false) }
    LaunchedEffect(zone.installationId, zone.systemId, zone.zoneId) {
        measuredThisMonth = runCatching { hub.api(serverId)?.daily(zone, 30) }.getOrNull().orEmpty()
            .any { d -> d.airQualitySecs?.let { it.good + it.regular + it.bad > 0 } == true }
    }
    if (air.level == null && !measuredThisMonth) return
    val secs = stats.orEmpty().mapNotNull { it.airQualitySecs }
    val measured = secs.sumOf { it.good + it.regular + it.bad }
    val goodShare = if (measured > 0) secs.sumOf { it.good }.toDouble() / measured else null
    SectionCard {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.weight(1f)) { SectionTitle(stringResource(Res.string.air_quality_title), FzIcons.Leaf) }
            listOf(7 to Res.string.zone_comfort_7d, 30 to Res.string.zone_comfort_30d).forEach { (d, label) ->
                FilterChip(selected = days == d, onClick = { days = d }, label = { Text(stringResource(label)) }, modifier = Modifier.padding(start = 6.dp))
            }
        }
        Caption(stringResource(Res.string.air_quality_hint_days))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Column(Modifier.weight(1f).clip(RoundedCornerShape(18.dp)).background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.6f)).padding(12.dp)) {
                Caption(stringResource(Res.string.air_quality_now))
                Text(airQualityLabel(air.level), color = airQualityColor(air.level), fontSize = 18.sp)
                val mode = when (air.mode) {
                    "off" -> Res.string.air_quality_mode_off
                    "on" -> Res.string.air_quality_mode_on
                    "auto" -> Res.string.air_quality_mode_auto
                    else -> null
                }
                if (mode != null) Caption(stringResource(mode))
            }
            Tile(
                stringResource(Res.string.air_quality_good_share),
                goodShare?.let { stringResource(Res.string.zone_comfort_percent, (it * 100).roundToInt()) } ?: "–",
                Modifier.weight(1f),
            )
        }
        val data = stats
        if (data != null && measured > 0) {
            AirQualityDaysChart(data)
            Row(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                listOf("good", "regular", "bad").forEach { Legend(airQualityColor(it), airQualityLabel(it)) }
            }
        }
    }
}

@Composable
private fun Legend(color: Color, label: String, shown: Boolean = true, onToggle: (() -> Unit)? = null) {
    Row(
        Modifier
            .alpha(if (shown) 1f else 0.4f)
            .then(if (onToggle != null) Modifier.clip(RoundedCornerShape(6.dp)).clickable(onClick = onToggle) else Modifier),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(8.dp).clip(RoundedCornerShape(4.dp)).background(color))
        Spacer(Modifier.size(5.dp))
        Text(label, fontSize = 11.sp, color = Fz.colors.muted)
    }
}
