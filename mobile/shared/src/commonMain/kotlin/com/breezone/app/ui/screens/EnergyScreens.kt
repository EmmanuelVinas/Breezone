package com.breezone.app.ui.screens

import androidx.compose.foundation.background
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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Switch
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
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.ChargerShown
import com.breezone.app.data.DeviceView
import com.breezone.app.data.EnergyDay
import com.breezone.app.data.EnergyLive
import com.breezone.app.data.EnergyPowerState
import com.breezone.app.data.EvChargerState
import com.breezone.app.data.EvChargerStatus
import com.breezone.app.data.Hub
import com.breezone.app.data.Periods
import com.breezone.app.data.ServerDevices
import com.breezone.app.data.SolarInverterState
import com.breezone.app.data.SolarPanelState
import com.breezone.app.data.TraitIds
import com.breezone.app.data.shown
import com.breezone.app.resources.*
import com.breezone.app.ui.components.CommandIndicator
import com.breezone.app.ui.components.DeviceGroupCard
import com.breezone.app.ui.components.FzSlider
import com.breezone.app.ui.components.IconBadge
import com.breezone.app.ui.components.PeriodChips
import com.breezone.app.ui.components.PowerDayChart
import com.breezone.app.ui.components.PowerSeries
import com.breezone.app.ui.components.Sparkline
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.theme.Fz
import kotlinx.coroutines.delay
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject
import kotlin.math.abs
import kotlin.math.roundToInt

/** "2,4 kW" or "640 W". */
fun formatPower(watts: Double?): String {
    if (watts == null) return "–"
    val w = abs(watts)
    return if (w >= 1000) "${((w / 100).roundToInt() / 10.0).toString().replace('.', ',')} kW" else "${w.roundToInt()} W"
}

/** "18,2 kWh". */
fun formatEnergy(kwh: Double?): String {
    if (kwh == null) return "–"
    val v = if (kwh >= 100) kwh.roundToInt().toString() else ((kwh * 10).roundToInt() / 10.0).toString().replace('.', ',')
    return "$v kWh"
}

/** Reads the energy of the home now (every 5 s) and of today (every minute) while shown. */
@Composable
internal fun rememberEnergy(hub: Hub, serverId: String): Pair<EnergyLive?, EnergyDay?> {
    var live by remember { mutableStateOf<EnergyLive?>(null) }
    var day by remember { mutableStateOf<EnergyDay?>(null) }
    LaunchedEffect(serverId) {
        var n = 0
        while (true) {
            val api = hub.api(serverId)
            runCatching { api?.energyLive() }.getOrNull()?.let { live = it }
            if (n % 12 == 0) runCatching { api?.energyDay() }.getOrNull()?.let { day = it }
            n++
            delay(5_000)
        }
    }
    return live to day
}

@Composable
internal fun FlowTile(icon: ImageVector, label: String, value: String, sub: String?, color: Color, modifier: Modifier = Modifier) {
    Column(
        modifier.clip(RoundedCornerShape(18.dp)).background(color.copy(alpha = 0.12f)).padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, null, Modifier.size(14.dp), tint = color)
            Spacer(Modifier.width(6.dp))
            Text(label, fontSize = 12.sp, color = color, maxLines = 1)
        }
        Text(value, fontSize = 20.sp, fontWeight = FontWeight.SemiBold)
        if (sub != null) Text(sub, fontSize = 12.sp, color = Fz.colors.muted, maxLines = 1)
    }
}

@Composable
private fun Stat(label: String, value: String, color: Color = MaterialTheme.colorScheme.onSurface, modifier: Modifier = Modifier) {
    Column(modifier) {
        Caption(label)
        Text(value, fontWeight = FontWeight.SemiBold, color = color)
    }
}

/** Tiles two by two. */
@Composable
private fun TwoByTwo(tiles: List<@Composable (Modifier) -> Unit>) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        tiles.chunked(2).forEach { row ->
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                row.forEach { it(Modifier.weight(1f)) }
                if (row.size == 1) Spacer(Modifier.weight(1f))
            }
        }
    }
}

@Composable
fun EnergyScreen(serverId: String, onBack: (() -> Unit)? = null) {
    val hub = koinInject<Hub>()
    val (live, day) = rememberEnergy(hub, serverId)
    val roles = live?.roles
    Scaffold(topBar = { FzTopBar(stringResource(Res.string.house_energy), null, onBack) }) { padding ->
        LazyColumn(
            Modifier.fillMaxSize(),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = padding.calculateTopPadding() + 4.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (live != null && roles?.any == false) {
                item(key = "empty", contentType = "note") { SectionCard { Muted(stringResource(Res.string.energy_empty)) } }
            }
            if (live != null && roles?.any == true) {
                item(key = "now", contentType = "now") { EnergyNowCard(live) }
                if (day != null) item(key = "today", contentType = "today") { EnergyTodayCard(day) }
                if (live.hasUses()) item(key = "uses", contentType = "uses") { EnergyUsesCard(live) }
            }
        }
    }
}

/** What the home draws and makes now: a tile per flow it has a measure of. */
@Composable
internal fun EnergyNowCard(live: EnergyLive, titled: Boolean = true) {
    val roles = live.roles
    val sun = Fz.colors.warning
    val solarShare = if ((live.solar ?: 0.0) > 0 && (live.home ?: 0.0) > 0) ((minOf(live.solar!!, live.home!!) / live.home) * 100).roundToInt() else null
    val tiles = buildList<@Composable (Modifier) -> Unit> {
        if (roles.solar) add { m -> FlowTile(FzIcons.Sun, stringResource(Res.string.energy_solar), formatPower(live.solar), null, sun, m) }
        add { m -> FlowTile(FzIcons.House, stringResource(Res.string.energy_home), formatPower(live.home), solarShare?.let { stringResource(Res.string.energy_solar_share, it) }, MaterialTheme.colorScheme.onSurface, m) }
        if (roles.battery) add { m ->
            val b = live.battery ?: 0.0
            FlowTile(FzIcons.BatteryMedium, stringResource(Res.string.energy_battery), formatPower(live.battery), stringResource(if (b >= 0) Res.string.energy_battery_charging else Res.string.energy_battery_discharging), Fz.colors.success, m)
        }
        if (roles.grid) add { m ->
            val g = live.grid ?: 0.0
            FlowTile(FzIcons.Zap, stringResource(Res.string.energy_grid), formatPower(live.grid), stringResource(if (g >= 0) Res.string.energy_grid_buys else Res.string.energy_grid_sells), Fz.colors.muted, m)
        }
    }
    SectionCard {
        if (titled) SectionTitle(stringResource(Res.string.energy_now))
        TwoByTwo(tiles)
    }
}

/** The day's curves of solar and home, with what was made, used and drawn. */
@Composable
internal fun EnergyTodayCard(day: EnergyDay) {
    val sun = Fz.colors.warning
    val t = day.totals
    val start = day.points.firstOrNull()?.ts ?: 0L
    SectionCard {
        SectionTitle(stringResource(Res.string.energy_today))
        PowerDayChart(
            listOf(
                PowerSeries(day.points.mapNotNull { p -> p.solar?.let { p.ts to it } }, sun, area = true),
                PowerSeries(day.points.mapNotNull { p -> p.home?.let { p.ts to it } }, MaterialTheme.colorScheme.onSurface, area = false),
            ),
            start,
            Modifier.fillMaxWidth().height(170.dp),
        )
        Row(Modifier.fillMaxWidth()) {
            if (t.production != null) Stat(stringResource(Res.string.energy_produced), formatEnergy(t.production), sun, Modifier.weight(1f))
            Stat(stringResource(Res.string.energy_consumed), formatEnergy(t.consumption), modifier = Modifier.weight(1f))
            if (t.production != null && t.production > 0 && t.selfConsumed != null) {
                Stat(stringResource(Res.string.energy_self), stringResource(Res.string.common_percent, (t.selfConsumed / t.production * 100).roundToInt()), MaterialTheme.colorScheme.primary, Modifier.weight(1f))
            } else if (t.imported != null) {
                Stat(stringResource(Res.string.energy_imported), formatEnergy(t.imported), modifier = Modifier.weight(1f))
            }
        }
    }
}

internal fun EnergyLive.hasUses() = consumers.any { (it.power ?: 0.0) > 0 } || (other ?: 0.0) > 0

/** What uses the power now, by measure, and what no measure accounts for. */
@Composable
internal fun EnergyUsesCard(live: EnergyLive) {
    val consumers = live.consumers.filter { (it.power ?: 0.0) > 0 }
    DeviceGroupCard(stringResource(Res.string.energy_uses), null) {
        val other = stringResource(Res.string.energy_other)
        (consumers.map { it.name to (it.power ?: 0.0) } + listOfNotNull(live.other?.takeIf { it > 0 }?.let { other to it })).forEach { (name, w) ->
            Row(Modifier.fillMaxWidth().padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(name, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(formatPower(w), fontWeight = FontWeight.Medium)
            }
        }
    }
}

@Composable
fun SolarScreen(serverId: String, onBack: (() -> Unit)? = null) {
    val hub = koinInject<Hub>()
    val all by hub.devices.collectAsState()
    LaunchedEffect(serverId) { hub.followDevices(serverId, everyMs = 10_000) }
    val (live, day) = rememberEnergy(hub, serverId)
    val data = all[serverId] ?: ServerDevices()
    val inverters = data.withTrait(TraitIds.INVERTER)
    val panels = data.withTrait(TraitIds.PANEL)
    Scaffold(topBar = { FzTopBar(stringResource(Res.string.house_solar), null, onBack) }) { padding ->
        LazyColumn(
            Modifier.fillMaxSize(),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = padding.calculateTopPadding() + 4.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item(key = "now", contentType = "now") { SolarNowCard(live, day, inverters) }
            inverters.forEach { inv ->
                item(key = "inv-${inv.id}", contentType = "inverter") { InverterCard(inv, panels) }
            }
        }
    }
}

/** What the panels make now, and their curve of the day. */
@Composable
internal fun SolarNowCard(live: EnergyLive?, day: EnergyDay?, inverters: List<DeviceView>) {
    val sun = Fz.colors.warning
    SectionCard {
        Caption(stringResource(Res.string.solar_now))
        Text(formatPower(live?.solar ?: inverters.sumOf { it.stateAs(EnergyPowerState.serializer())?.power ?: 0.0 }), fontSize = 40.sp, fontWeight = FontWeight.SemiBold, color = sun)
        day?.totals?.production?.let { Caption(stringResource(Res.string.solar_today, formatEnergy(it))) }
        if (day != null) {
            PowerDayChart(
                listOf(PowerSeries(day.points.mapNotNull { p -> p.solar?.let { p.ts to it } }, sun, area = true)),
                day.points.firstOrNull()?.ts ?: 0L,
                Modifier.fillMaxWidth().height(150.dp),
            )
        }
    }
}

/** A micro-inverter and its panels, each with what it makes now and today. */
@Composable
internal fun InverterCard(inv: DeviceView, panels: List<DeviceView>) {
    val sun = Fz.colors.warning
    val s = inv.stateAs(SolarInverterState.serializer())
    val power = inv.stateAs(EnergyPowerState.serializer())?.power
    val mine = panels.filter { p -> p.stateAs(SolarPanelState.serializer())?.inverter == inv.reference.toString().trim('"') || panels.size == (s?.panels ?: -1L).toInt() }
    DeviceGroupCard(inv.name, listOfNotNull(power?.let(::formatPower), s?.energyToday?.let { stringResource(Res.string.solar_today, formatEnergy(it)) }).joinToString(" · ")) {
        (mine.ifEmpty { panels }).forEach { p ->
            val ps = p.stateAs(SolarPanelState.serializer())
            Row(Modifier.fillMaxWidth().padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                IconBadge(FzIcons.Sun, sun, active = (ps?.power ?: 0.0) > 0, size = 34.dp)
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text(p.name, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    ps?.energyToday?.let { Caption(stringResource(Res.string.solar_today, formatEnergy(it))) }
                }
                Text(formatPower(ps?.power), fontWeight = FontWeight.Medium)
            }
        }
        if (s?.temperature != null) Caption(stringResource(Res.string.solar_temperature, s.temperature.roundToInt()))
    }
}

@Composable
fun chargerLabel(shown: ChargerShown): String = stringResource(
    when (shown) {
        ChargerShown.Charging -> Res.string.charger_charging
        ChargerShown.Fault -> Res.string.charger_fault
        ChargerShown.Paused -> Res.string.charger_paused
        ChargerShown.Plugged -> Res.string.charger_plugged
        ChargerShown.Available -> Res.string.charger_available
    },
)

@Composable
fun ChargingScreen(serverId: String, onBack: (() -> Unit)? = null) {
    val hub = koinInject<Hub>()
    val all by hub.devices.collectAsState()
    val status by hub.deviceStatus.collectAsState()
    LaunchedEffect(serverId) { hub.followDevices(serverId, everyMs = 5_000) }
    val data = all[serverId] ?: ServerDevices()
    val charger: DeviceView? = data.withTrait(TraitIds.CHARGER).firstOrNull()
    Scaffold(topBar = {
        FzTopBar(stringResource(Res.string.house_charging), charger?.name, onBack) {
            charger?.let { CommandIndicator(status["$serverId/${it.id}"], Modifier.padding(end = 16.dp)) }
        }
    }) { padding ->
        LazyColumn(
            Modifier.fillMaxSize(),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = padding.calculateTopPadding() + 4.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (data.loaded && charger == null) {
                item(key = "empty", contentType = "note") { SectionCard { Muted(stringResource(Res.string.charging_empty)) } }
            }
            if (charger != null) {
                val s = charger.stateAs(EvChargerState.serializer()) ?: EvChargerState()
                val act: (String, JsonObject, JsonObject?) -> Unit = { action, params, optimistic -> hub.act(serverId, charger, "ev.charger", action, params, optimistic) }
                item(key = "hero", contentType = "hero") { ChargerHero(serverId, charger) }
                item(key = "history", contentType = "history") { ChargerPowerCard(serverId, charger) }
                item(key = "settings", contentType = "settings") {
                    SectionCard {
                        SectionTitle(stringResource(Res.string.charger_settings))
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Column(Modifier.weight(1f)) {
                                Text(stringResource(Res.string.charger_auto_limit))
                                Caption(stringResource(Res.string.charger_auto_limit_hint))
                            }
                            Switch(checked = s.autoLimit == true, enabled = charger.usable, onCheckedChange = { on ->
                                act("set_auto_limit", buildJsonObject { put("on", on) }, buildJsonObject { put("auto_limit", on) })
                            })
                        }
                        val min = (s.limitMin ?: 6).toFloat()
                        val max = (s.limitMax ?: 32).toFloat().coerceAtLeast(min + 1)
                        if (s.autoLimit != true) {
                            Row {
                                Text(stringResource(Res.string.charger_limit), modifier = Modifier.weight(1f))
                                Text(stringResource(Res.string.charger_amps, (s.limit ?: min.toLong()).toInt()), fontWeight = FontWeight.Medium)
                            }
                            FzSlider(
                                value = (s.limit ?: min.toLong()).toFloat(),
                                onCommit = { v -> act("set_limit", buildJsonObject { put("limit", v.roundToInt()) }, buildJsonObject { put("limit", v.roundToInt()) }) },
                                range = min..max,
                                enabled = charger.usable,
                                colors = SliderDefaults.colors(),
                            )
                        }
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(stringResource(Res.string.charger_locked), modifier = Modifier.weight(1f))
                            Switch(checked = s.locked, enabled = charger.usable, onCheckedChange = { locked ->
                                act("lock", buildJsonObject { put("locked", locked) }, buildJsonObject { put("locked", locked) })
                            })
                        }
                    }
                }
            }
        }
    }
}

/** A charger now: what it does, the power it draws, its goal, and pause or start. */
@Composable
internal fun ChargerHero(serverId: String, charger: DeviceView) {
    val hub = koinInject<Hub>()
    val s = charger.stateAs(EvChargerState.serializer()) ?: EvChargerState()
    val power = charger.stateAs(EnergyPowerState.serializer())?.power
    val act: (String, JsonObject, JsonObject?) -> Unit = { action, params, optimistic -> hub.act(serverId, charger, "ev.charger", action, params, optimistic) }
    val charging = s.status == EvChargerStatus.Charging
    SectionCard {
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconBadge(FzIcons.PlugZap, MaterialTheme.colorScheme.primary, active = charging, size = 48.dp)
            Spacer(Modifier.width(12.dp))
            Column {
                Text(chargerLabel(s.shown), style = MaterialTheme.typography.titleMedium)
                if (s.vehicle == true && s.sessionSecs > 0) Caption(stringResource(Res.string.charger_session, formatEnergy(s.sessionEnergy), s.sessionSecs.toInt() / 3600, s.sessionSecs.toInt() % 3600 / 60))
            }
        }
        Text(
            formatPower(power ?: 0.0),
            fontSize = 48.sp,
            fontWeight = FontWeight.SemiBold,
            color = if (charging) MaterialTheme.colorScheme.primary else Fz.colors.muted,
            modifier = Modifier.align(Alignment.CenterHorizontally),
        )
        s.energyGoal?.let { goal ->
            Box(Modifier.fillMaxWidth().height(10.dp).clip(CircleShape).background(MaterialTheme.colorScheme.surfaceVariant)) {
                Box(Modifier.fillMaxHeight().fillMaxWidth((s.sessionEnergy / goal).toFloat().coerceIn(0f, 1f)).clip(CircleShape).background(MaterialTheme.colorScheme.primary))
            }
            Caption(stringResource(Res.string.charger_goal, formatEnergy(goal)))
        }
        if (s.vehicle == true) {
            if (charging || !s.paused) {
                OutlinedButton(
                    onClick = { act("stop", JsonObject(emptyMap()), buildJsonObject { put("paused", true) }) },
                    enabled = charger.usable,
                    shape = Fz.shapes.button,
                    modifier = Modifier.fillMaxWidth(),
                ) { Text(stringResource(Res.string.charger_pause)) }
            } else {
                Button(
                    onClick = { act("start", JsonObject(emptyMap()), buildJsonObject { put("paused", false) }) },
                    enabled = charger.usable,
                    shape = Fz.shapes.button,
                    modifier = Modifier.fillMaxWidth(),
                ) { Text(stringResource(Res.string.charger_start)) }
            }
        }
    }
}

/** The power a charger drew over a period (none until it has a history). */
@Composable
internal fun ChargerPowerCard(serverId: String, charger: DeviceView) {
    val hub = koinInject<Hub>()
    var history by remember { mutableStateOf<List<List<Double>>>(emptyList()) }
    var oldest by remember { mutableStateOf<Long?>(null) }
    var hours by remember { mutableIntStateOf(Periods.DAY) }
    LaunchedEffect(serverId, charger.id, hours) {
        val read = runCatching { hub.api(serverId)?.deviceHistoryOver(charger.id, hours) }.getOrNull() ?: return@LaunchedEffect
        history = read.series["power"].orEmpty()
        oldest = read.oldest
    }
    if (history.size < 2) return
    SectionCard {
        SectionTitle(stringResource(Res.string.charger_power))
        PeriodChips(hours, oldest) { hours = it }
        Sparkline(history, MaterialTheme.colorScheme.primary, Modifier.fillMaxWidth().height(72.dp))
    }
}
