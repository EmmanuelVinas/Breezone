package com.breezone.app.ui.home

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.BlockSize
import com.breezone.app.data.BlockTypes
import com.breezone.app.data.ChargerShown
import com.breezone.app.data.CommandStatus
import com.breezone.app.data.CoverPositionState
import com.breezone.app.data.DashboardBlock
import com.breezone.app.data.DeviceHistory
import com.breezone.app.data.DeviceView
import com.breezone.app.data.EnergyDay
import com.breezone.app.data.EnergyLive
import com.breezone.app.data.EnergyPowerState
import com.breezone.app.data.EvChargerState
import com.breezone.app.data.Hub
import com.breezone.app.data.Installation
import com.breezone.app.data.LightState
import com.breezone.app.data.Presence
import com.breezone.app.data.Room
import com.breezone.app.data.Routine
import com.breezone.app.data.ServerDevices
import com.breezone.app.data.TraitIds
import com.breezone.app.data.VentilationRoomState
import com.breezone.app.data.WeatherView
import com.breezone.app.data.Zone
import com.breezone.app.data.ZoneCommand
import com.breezone.app.data.commandKey
import com.breezone.app.data.shown
import com.breezone.app.data.shownOnCards
import com.breezone.app.resources.*
import com.breezone.app.ui.components.DeviceGroupCard
import com.breezone.app.ui.components.Sparkline
import com.breezone.app.ui.components.TypeTile
import com.breezone.app.ui.components.WeatherCard
import com.breezone.app.ui.components.ZoneCard
import com.breezone.app.ui.formatTemp
import com.breezone.app.ui.groupZones
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.screens.Caption
import com.breezone.app.ui.screens.EnergyNowCard
import com.breezone.app.ui.screens.EnergyTodayCard
import com.breezone.app.ui.screens.EnergyUsesCard
import com.breezone.app.ui.screens.Muted
import com.breezone.app.ui.screens.SectionCard
import com.breezone.app.ui.screens.chargerLabel
import com.breezone.app.ui.screens.formatEnergy
import com.breezone.app.ui.screens.formatPower
import com.breezone.app.ui.screens.hasUses
import com.breezone.app.ui.screens.AirQualitySection
import com.breezone.app.ui.screens.ChargerHero
import com.breezone.app.ui.screens.ChargerPowerCard
import com.breezone.app.ui.screens.InverterCard
import com.breezone.app.ui.screens.SolarNowCard
import com.breezone.app.ui.screens.VentilationHouseCard
import com.breezone.app.ui.screens.ZoneHistorySection
import com.breezone.app.ui.screens.ventilationLevel
import com.breezone.app.ui.theme.Fz
import kotlinx.coroutines.delay
import org.jetbrains.compose.resources.StringResource
import org.jetbrains.compose.resources.getPluralString
import org.jetbrains.compose.resources.getString
import org.jetbrains.compose.resources.pluralStringResource
import org.jetbrains.compose.resources.stringResource
import kotlin.math.roundToInt

/** Where a card leads: a page of the bar, a device's own screen, a zone. */
class HomeNav(
    val openPage: (AppPage) -> Unit,
    val openDevice: (DeviceView) -> Unit,
    val openZone: (Zone) -> Unit,
)

/** What the cards of an installation read: read once for the whole home, shared by its cards. */
data class HomeContext(
    val hub: Hub,
    val serverId: String,
    val installation: Installation?,
    /** The devices of this installation: its zones, and the modules' when they go with it. */
    val devices: List<DeviceView>,
    val rooms: List<Room>,
    val server: ServerDevices,
    val pages: List<AppPage>,
    val live: EnergyLive?,
    val day: EnergyDay?,
    val weather: WeatherView?,
    val routines: List<Routine>?,
    val deviceStatus: Map<String, CommandStatus>,
    val commandStatus: Map<String, CommandStatus>,
    val nav: HomeNav,
)

/** An energy flow of the home: its name, its icon, its colour. */
enum class EnergyFlow(val id: String, val label: StringResource, val icon: ImageVector) {
    Solar("solar", Res.string.energy_solar, FzIcons.Sun),
    Home("home", Res.string.energy_home, FzIcons.House),
    Battery("battery", Res.string.energy_battery, FzIcons.BatteryMedium),
    Grid("grid", Res.string.energy_grid, FzIcons.Zap),
    ;

    companion object {
        fun of(id: String?) = entries.firstOrNull { it.id == id }
    }
}

/**
 * A kind of card: its name and icon in the list of cards to add, the size it starts with, and
 * whether it needs a choice (a room, a device…) before it is added. What it shows is [BlockContent].
 */
enum class BlockKind(
    val type: String,
    val label: StringResource,
    val icon: ImageVector,
    val size: BlockSize,
    val choice: BlockChoice = BlockChoice.None,
) {
    Pages(BlockTypes.PAGES, Res.string.block_pages, FzIcons.LayoutGrid, BlockSize.Full, BlockChoice.Pages),
    Weather(BlockTypes.WEATHER, Res.string.block_weather, FzIcons.CloudSun, BlockSize.Full),
    Presence(BlockTypes.PRESENCE, Res.string.block_presence, FzIcons.Users, BlockSize.Full),
    Energy(BlockTypes.ENERGY, Res.string.block_energy, FzIcons.Zap, BlockSize.Half, BlockChoice.Flow),
    EnergyNow(BlockTypes.ENERGY_FLOW, Res.string.block_energy_now, FzIcons.Activity, BlockSize.Full),
    EnergyToday(BlockTypes.ENERGY_CURVE, Res.string.block_energy_today, FzIcons.ChartLine, BlockSize.Full),
    EnergyUses(BlockTypes.ENERGY_BREAKDOWN, Res.string.block_energy_uses, FzIcons.PlugZap, BlockSize.Full),
    Zones(BlockTypes.ZONES, Res.string.block_zones, FzIcons.Thermometer, BlockSize.Full, BlockChoice.ZoneGroup),
    Room(BlockTypes.GROUP, Res.string.block_room, FzIcons.House, BlockSize.Full, BlockChoice.Room),
    Kind(BlockTypes.GROUP, Res.string.block_kind, FzIcons.Box, BlockSize.Full, BlockChoice.Kind),
    Device(BlockTypes.DEVICE, Res.string.block_device, FzIcons.Power, BlockSize.Full, BlockChoice.Device),
    Chart(BlockTypes.CHART, Res.string.block_chart, FzIcons.ChartLine, BlockSize.Full, BlockChoice.Chart),
    Scenes(BlockTypes.SCENES, Res.string.block_scenes, FzIcons.Sparkles, BlockSize.Full),
    ZoneHistory(BlockTypes.ZONE_HISTORY, Res.string.block_zone_history, FzIcons.ChartLine, BlockSize.Full, BlockChoice.Zone),
    AirQuality(BlockTypes.AIR_QUALITY, Res.string.block_air_quality, FzIcons.Wind, BlockSize.Full, BlockChoice.Zone),
    Charger(BlockTypes.CHARGER, Res.string.block_charger, FzIcons.PlugZap, BlockSize.Full),
    ChargerCurve(BlockTypes.CHARGER_CURVE, Res.string.block_charger_curve, FzIcons.ChartLine, BlockSize.Full),
    SolarToday(BlockTypes.SOLAR_TODAY, Res.string.block_solar_today, FzIcons.Sun, BlockSize.Full),
    SolarPanels(BlockTypes.SOLAR_PANELS, Res.string.block_solar_panels, FzIcons.SolarPanel, BlockSize.Full),
    VentilationHouse(BlockTypes.VENTILATION_HOUSE, Res.string.block_ventilation_house, FzIcons.Fan, BlockSize.Full),
    Spacer(BlockTypes.SPACER, Res.string.block_spacer, FzIcons.Square, BlockSize.Half),
    ;

    /** Worth offering for this home: an energy card needs a measure, a zone card zones… */
    fun offered(ctx: HomeContext): Boolean = when (this) {
        Energy, EnergyNow, EnergyToday, EnergyUses -> ctx.live?.roles?.any == true || ctx.server.modules.any { it.role != null }
        Zones -> ctx.installation?.zones?.isNotEmpty() == true
        Room, Kind, Device -> ctx.devices.isNotEmpty()
        Chart -> ctx.devices.isNotEmpty()
        ZoneHistory -> ctx.installation?.zones?.isNotEmpty() == true
        AirQuality -> ctx.installation?.zones?.any { it.airQuality != null } == true
        Charger, ChargerCurve -> ctx.server.withTrait(TraitIds.CHARGER).isNotEmpty() && AppPage.Charging in ctx.pages
        SolarToday, SolarPanels -> AppPage.Solar in ctx.pages
        VentilationHouse -> ctx.server.withTrait(TraitIds.VENTILATION_UNIT).isNotEmpty() && AppPage.Ventilation in ctx.pages
        else -> true
    }

    companion object {
        /** The kind of a card as saved (a group is a room's or a kind's). */
        fun of(b: DashboardBlock): BlockKind? = when {
            b.type == BlockTypes.GROUP -> if (b.has("type")) Kind else Room
            else -> entries.firstOrNull { it.type == b.type }
        }
    }
}

/** What has to be chosen before a card is added. */
enum class BlockChoice { None, Flow, Room, Kind, Device, Chart, ZoneGroup, Zone, Pages }

/** The title a card shows while the home is being arranged, so that each can be told apart. */
@Composable
fun blockTitle(b: DashboardBlock, ctx: HomeContext): String {
    val kind = BlockKind.of(b) ?: return stringResource(Res.string.block_unknown)
    return when (kind) {
        BlockKind.Energy -> EnergyFlow.of(b.string("flow"))?.let { stringResource(it.label) } ?: stringResource(kind.label)
        BlockKind.Room -> ctx.rooms.firstOrNull { it.id == b.long("room_id") }?.name ?: stringResource(Res.string.block_no_room)
        BlockKind.Kind -> DeviceKind.byId(b.string("type"))?.let { stringResource(it.label) } ?: stringResource(kind.label)
        BlockKind.Device, BlockKind.Chart -> ctx.devices.firstOrNull { it.id == b.long("device_id") }?.name ?: stringResource(kind.label)
        BlockKind.Zones -> b.string("group") ?: stringResource(kind.label)
        BlockKind.Pages -> b.strings("pages").mapNotNull { id -> AppPage.entries.firstOrNull { it.name == id } }.takeIf { it.isNotEmpty() }
            ?.map { stringResource(it.label) }?.joinToString(" · ") ?: stringResource(kind.label)
        BlockKind.ZoneHistory, BlockKind.AirQuality -> listOfNotNull(stringResource(kind.label), ctx.devices.firstOrNull { it.id == b.long("device_id") }?.name).joinToString(" · ")
        else -> stringResource(kind.label)
    }
}

/** A card of the home; one of a kind this version does not know shows nothing. */
@Composable
fun BlockContent(b: DashboardBlock, ctx: HomeContext, editing: Boolean) {
    when (BlockKind.of(b)) {
        BlockKind.Pages -> PagesBlock(b.strings("pages"), ctx)
        BlockKind.Weather -> if (ctx.weather?.weather != null || editing) WeatherCard(ctx.weather)
        BlockKind.Presence -> PresenceBlock(ctx)
        BlockKind.Energy -> EnergyBlock(EnergyFlow.of(b.string("flow")) ?: EnergyFlow.Solar, ctx)
        BlockKind.EnergyNow -> ctx.live?.takeIf { it.roles.any }?.let { EnergyNowCard(it) } ?: Waiting(b, ctx, editing)
        BlockKind.EnergyToday -> ctx.day?.let { EnergyTodayCard(it) } ?: Waiting(b, ctx, editing)
        BlockKind.EnergyUses -> ctx.live?.takeIf { it.hasUses() }?.let { EnergyUsesCard(it) } ?: Waiting(b, ctx, editing)
        BlockKind.Zones -> ZonesBlock(b.string("group"), ctx)
        BlockKind.Room, BlockKind.Kind -> GroupBlock(b, ctx)
        BlockKind.Device -> DeviceBlock(b.long("device_id"), ctx)
        BlockKind.Chart -> ChartBlock(b.long("device_id"), b.strings("fields").ifEmpty { listOfNotNull(b.string("field")) }, b.long("hours")?.toInt() ?: 24, ctx)
        BlockKind.Scenes -> ScenesBlock(b.longs("routine_ids"), ctx)
        BlockKind.ZoneHistory -> zoneBlock(b, ctx)?.let { ZoneHistorySection(ctx.hub, ctx.serverId, it) } ?: Gone()
        BlockKind.AirQuality -> zoneBlock(b, ctx)?.let { z -> z.airQuality?.let { AirQualitySection(ctx.hub, ctx.serverId, z, it) } } ?: Waiting(b, ctx, editing)
        BlockKind.Charger -> charger(b, ctx)?.let { ChargerHero(ctx.serverId, it) } ?: Gone()
        BlockKind.ChargerCurve -> charger(b, ctx)?.let { ChargerPowerCard(ctx.serverId, it) } ?: Gone()
        BlockKind.SolarToday -> SolarNowCard(ctx.live, ctx.day, ctx.server.withTrait(TraitIds.INVERTER))
        BlockKind.SolarPanels -> ctx.server.withTrait(TraitIds.INVERTER).forEach { InverterCard(it, ctx.server.withTrait(TraitIds.PANEL)) }
        BlockKind.VentilationHouse -> ctx.server.withTrait(TraitIds.VENTILATION_UNIT).firstOrNull()?.let { unit ->
            VentilationHouseCard(ctx.serverId, unit, ctx.server.withTrait(TraitIds.VENTILATION_ROOM).filter { it.sourceId == unit.sourceId })
        } ?: Gone()
        // An empty cell, to leave room: seen only while arranging.
        BlockKind.Spacer -> if (editing) SectionCard { Muted(stringResource(Res.string.block_spacer)) }
        null -> if (editing) Waiting(b, ctx, true)
    }
}

/** The zone a card shows (its device). */
private fun zoneBlock(b: DashboardBlock, ctx: HomeContext): Zone? = ctx.devices.firstOrNull { it.id == b.long("device_id") }?.let { ctx.zoneOf(it) }

/** The charger a card shows: the one chosen, else the first. */
private fun charger(b: DashboardBlock, ctx: HomeContext): DeviceView? {
    val chargers = ctx.server.withTrait(TraitIds.CHARGER)
    return chargers.firstOrNull { it.id == b.long("device_id") } ?: chargers.firstOrNull()
}

@Composable
private fun Gone() = SectionCard { Muted(stringResource(Res.string.block_device_gone)) }

/** A card with nothing to show yet: only while arranging, so that it can be told apart. */
@Composable
private fun Waiting(b: DashboardBlock, ctx: HomeContext, editing: Boolean) {
    if (!editing) return
    SectionCard { Muted(blockTitle(b, ctx)) }
}

/** Tiles of the pages of the installation, with what each does now. */
@Composable
private fun PagesBlock(chosen: List<String>, ctx: HomeContext) {
    val all = shortcutPages(ctx)
    // The pages chosen, in their order; none chosen: all of them.
    val tiles = if (chosen.isEmpty()) all else chosen.mapNotNull { id -> all.firstOrNull { it.name == id } }
    if (tiles.isEmpty()) return
    if (tiles.size == 1) {
        val page = tiles.single()
        val (sub, color, icon) = pageSummary(page, ctx)
        TypeTile(icon, stringResource(page.label), sub, color, onClick = { ctx.nav.openPage(page) }, modifier = Modifier.fillMaxWidth())
        return
    }
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        tiles.chunked(2).forEach { row ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                row.forEach { page ->
                    val (sub, color, icon) = pageSummary(page, ctx)
                    TypeTile(icon, stringResource(page.label), sub, color, onClick = { ctx.nav.openPage(page) }, modifier = Modifier.weight(1f))
                }
                if (row.size == 1) Spacer(Modifier.weight(1f))
            }
        }
    }
}

/** The pages a shortcut can lead to: those of the installation, but the home and the lists. */
fun shortcutPages(ctx: HomeContext) = ctx.pages.filter { it != AppPage.Home && it != AppPage.Settings && it != AppPage.Routines && it != AppPage.Schedules }

/** What a page's tile says now, and how it looks. */
@Composable
private fun pageSummary(page: AppPage, ctx: HomeContext): Triple<String, Color, ImageVector> {
    val d = ctx.server
    return when (page) {
        AppPage.Climate -> {
            val zones = ctx.installation?.zones.orEmpty()
            val on = zones.count { it.on }
            Triple(pluralStringResource(Res.plurals.installation_zones_on, on, on, zones.size), Fz.colors.heat, page.icon)
        }
        AppPage.Lights -> {
            val lights = d.withTrait(TraitIds.LIGHT)
            val on = lights.count { it.available && it.stateAs(LightState.serializer())?.on == true }
            Triple(pluralStringResource(Res.plurals.lights_on_of, on, on, lights.size), Fz.colors.light, page.icon)
        }
        AppPage.Shutters -> {
            val covers = d.withTrait(TraitIds.COVER)
            val open = covers.count { it.available && (it.stateAs(CoverPositionState.serializer())?.position ?: 0) > 0 }
            Triple(pluralStringResource(Res.plurals.shutters_open_of, open, open, covers.size), Fz.colors.shutter, page.icon)
        }
        AppPage.Ventilation -> {
            val level = ventilationLevel(d.withTrait(TraitIds.VENTILATION_ROOM).mapNotNull { it.stateAs(VentilationRoomState.serializer()) })
            Triple(level?.let { stringResource(Res.string.ventilation_level_short, it) } ?: "–", Fz.colors.air, page.icon)
        }
        AppPage.Energy -> {
            val solar = d.modules.filter { it.role == "solar" }.sumOf { it.stateAs(EnergyPowerState.serializer())?.power ?: 0.0 }
            val grid = d.modules.filter { it.role == "grid" }.sumOf { it.stateAs(EnergyPowerState.serializer())?.power ?: 0.0 }
            val sub = when {
                solar > 0 -> stringResource(Res.string.energy_tile_solar, formatPower(solar))
                d.modules.any { it.role == "grid" } -> stringResource(Res.string.energy_tile_grid, formatPower(grid))
                else -> stringResource(Res.string.energy_tile)
            }
            Triple(sub, Fz.colors.warning, page.icon)
        }
        AppPage.Solar -> {
            val power = d.withTrait(TraitIds.INVERTER).sumOf { it.stateAs(EnergyPowerState.serializer())?.power ?: 0.0 }
            Triple(formatPower(power), Fz.colors.warning, page.icon)
        }
        AppPage.Charging -> {
            val c = d.withTrait(TraitIds.CHARGER).firstOrNull()
            val shown = c?.stateAs(EvChargerState.serializer())?.shown ?: ChargerShown.Available
            val power = c?.stateAs(EnergyPowerState.serializer())?.power ?: 0.0
            val sub = if (shown == ChargerShown.Charging) formatPower(power) else chargerLabel(shown)
            // At a glance: a car plugged in, charging, or nothing.
            when (shown) {
                ChargerShown.Charging -> Triple(sub, MaterialTheme.colorScheme.primary, FzIcons.PlugZap)
                ChargerShown.Paused, ChargerShown.Plugged -> Triple(sub, MaterialTheme.colorScheme.primary, FzIcons.Car)
                ChargerShown.Fault -> Triple(sub, MaterialTheme.colorScheme.error, FzIcons.PlugZap)
                ChargerShown.Available -> Triple(sub, Fz.colors.muted, FzIcons.Plug)
            }
        }
        else -> Triple("", page.color(), page.icon)
    }
}

/** One flow of the home now, with its curve of the day. */
@Composable
private fun EnergyBlock(flow: EnergyFlow, ctx: HomeContext) {
    val live = ctx.live
    val value = when (flow) {
        EnergyFlow.Solar -> live?.solar
        EnergyFlow.Home -> live?.home
        EnergyFlow.Battery -> live?.battery
        EnergyFlow.Grid -> live?.grid
    }
    val sub = when (flow) {
        EnergyFlow.Solar -> ctx.day?.totals?.production?.let { stringResource(Res.string.energy_today_value, formatEnergy(it)) }
        EnergyFlow.Home -> live?.let { l ->
            val s = l.solar ?: 0.0
            val h = l.home ?: 0.0
            if (s > 0 && h > 0) stringResource(Res.string.energy_solar_share, (minOf(s, h) / h * 100).roundToInt()) else null
        }
        EnergyFlow.Battery -> value?.let { stringResource(if (it >= 0) Res.string.energy_battery_charging else Res.string.energy_battery_discharging) }
        EnergyFlow.Grid -> value?.let { stringResource(if (it >= 0) Res.string.energy_grid_buys else Res.string.energy_grid_sells) }
    }
    val color = when (flow) {
        EnergyFlow.Solar -> Fz.colors.warning
        EnergyFlow.Battery -> Fz.colors.success
        EnergyFlow.Grid -> Fz.colors.cool
        EnergyFlow.Home -> MaterialTheme.colorScheme.primary
    }
    val points = ctx.day?.points.orEmpty().mapNotNull { p ->
        when (flow) {
            EnergyFlow.Solar -> p.solar
            EnergyFlow.Home -> p.home
            EnergyFlow.Battery -> p.battery
            EnergyFlow.Grid -> p.grid
        }?.let { listOf(p.ts.toDouble(), it) }
    }
    SectionCard(Modifier.clickable { ctx.nav.openPage(AppPage.Energy) }) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(flow.icon, null, Modifier.size(14.dp), tint = color)
            Spacer(Modifier.width(6.dp))
            Text(stringResource(flow.label), fontSize = 12.sp, color = color, maxLines = 1)
        }
        Text(formatPower(value), fontSize = 22.sp, fontWeight = FontWeight.SemiBold)
        if (sub != null) Caption(sub, maxLines = 1)
        if (points.size > 1) Sparkline(points, color, Modifier.fillMaxWidth().height(36.dp))
    }
}

/** The zones of a group of the installation (all without one), as on the climate page. */
@Composable
private fun ZonesBlock(group: String?, ctx: HomeContext) {
    val zones = ctx.installation?.zones.orEmpty().filter { group == null || it.group == group }
    if (zones.isEmpty()) return
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        zones.chunked(2).forEach { row ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                row.forEach { zone ->
                    ZoneCard(
                        zone = zone,
                        status = ctx.commandStatus[commandKey(ctx.serverId, zone)],
                        onToggle = { ctx.hub.control(ctx.serverId, zone, ZoneCommand(on = it)) },
                        onSetpoint = { ctx.hub.control(ctx.serverId, zone, ZoneCommand(setpoint = it)) },
                        onClick = { ctx.nav.openZone(zone) },
                        modifier = Modifier.weight(1f),
                    )
                }
                if (row.size == 1) Spacer(Modifier.weight(1f))
            }
        }
    }
}

/** The devices of a room, or of a kind, each driven from its line. */
@Composable
private fun GroupBlock(b: DashboardBlock, ctx: HomeContext) {
    val kind = DeviceKind.byId(b.string("type"))
    val hidden = b.longs("hidden").toSet()
    val list = ctx.devices
        .filter { if (b.has("type")) it.deviceKind == kind else it.roomId == b.long("room_id") }
        .filter { it.shownOnCards() && it.id !in hidden }
    val temps = list.mapNotNull { d -> ctx.zoneOf(d)?.roomTemp }
    val meta = when {
        temps.size > 1 -> stringResource(Res.string.block_average, formatTemp(temps.average()))
        temps.size == 1 -> formatTemp(temps.first())
        else -> pluralStringResource(Res.plurals.block_devices, list.size, list.size)
    }
    val page = kind?.page?.takeIf { it in ctx.pages }
    DeviceGroupCard(blockTitle(b, ctx), meta, trailing = {
        if (page != null) {
            Spacer(Modifier.width(4.dp))
            Icon(FzIcons.ChevronRight, null, Modifier.size(18.dp).clickable { ctx.nav.openPage(page) }, tint = Fz.colors.muted)
        }
    }) {
        if (list.isEmpty()) Muted(stringResource(Res.string.block_empty))
        list.forEach { d -> DeviceEntry(d, ctx) }
    }
}

/** A device alone: a zone as on the climate page, anything else on its line. */
@Composable
private fun DeviceBlock(id: Long?, ctx: HomeContext) {
    val d = ctx.devices.firstOrNull { it.id == id }
    if (d == null) {
        SectionCard { Muted(stringResource(Res.string.block_device_gone)) }
        return
    }
    val zone = ctx.zoneOf(d)
    if (zone != null) {
        ZoneCard(
            zone = zone,
            status = ctx.commandStatus[commandKey(ctx.serverId, zone)],
            onToggle = { ctx.hub.control(ctx.serverId, zone, ZoneCommand(on = it)) },
            onSetpoint = { ctx.hub.control(ctx.serverId, zone, ZoneCommand(setpoint = it)) },
            onClick = { ctx.nav.openZone(zone) },
            modifier = Modifier.fillMaxWidth(),
        )
        return
    }
    DeviceGroupCard(d.name, stringResource(d.deviceKind.label)) { DeviceEntry(d, ctx) }
}

/** The curves of a device over the last hours: its measures now, and how they went. */
@Composable
private fun ChartBlock(id: Long?, fields: List<String>, hours: Int, ctx: HomeContext) {
    val d = ctx.devices.firstOrNull { it.id == id }
    var history by remember(id, hours) { mutableStateOf<DeviceHistory?>(null) }
    LaunchedEffect(ctx.serverId, id, hours) {
        if (id == null) return@LaunchedEffect
        while (true) {
            runCatching { ctx.hub.api(ctx.serverId)?.deviceHistory(id, hours, 96) }.getOrNull()?.let { history = it }
            // History points are written every 5 minutes.
            delay(300_000)
        }
    }
    SectionCard {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(d?.name ?: stringResource(Res.string.block_device_gone), style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
            Caption(com.breezone.app.ui.components.periodLabel(hours))
        }
        val colors = listOf(MaterialTheme.colorScheme.primary, Fz.colors.warning, Fz.colors.cool, Fz.colors.success)
        fields.forEachIndexed { i, field ->
            val points = history?.series?.get(field).orEmpty()
            val color = colors[i % colors.size]
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(fieldLabel(field), fontSize = 12.sp, color = color, modifier = Modifier.weight(1f))
                points.lastOrNull()?.getOrNull(1)?.let { Text(fieldValue(field, it), fontWeight = FontWeight.SemiBold) }
            }
            if (points.size > 1) Sparkline(points, color, Modifier.fillMaxWidth().height(64.dp))
        }
        if (history != null && fields.all { history?.series?.get(it).isNullOrEmpty() }) Muted(stringResource(Res.string.block_chart_empty))
    }
}

/** The name of a measure kept in history. */
@Composable
fun fieldLabel(field: String): String = when (field) {
    "power" -> stringResource(Res.string.field_power)
    "energy", "energy_total", "session_energy" -> stringResource(Res.string.field_energy)
    "room_temp", "temperature" -> stringResource(Res.string.field_temperature)
    "setpoint" -> stringResource(Res.string.field_setpoint)
    "humidity" -> stringResource(Res.string.field_humidity)
    "co2" -> stringResource(Res.string.field_co2)
    "flow" -> stringResource(Res.string.field_flow)
    "level", "brightness", "position" -> stringResource(Res.string.field_level)
    else -> field.replace('_', ' ')
}

/** A value of a measure, with its unit. */
fun fieldValue(field: String, v: Double): String = when (field) {
    "power" -> formatPower(v)
    "energy", "energy_total", "session_energy" -> formatEnergy(v)
    "room_temp", "temperature", "setpoint" -> formatTemp(v)
    "humidity", "level", "brightness", "position" -> "${v.roundToInt()} %"
    "co2" -> "${v.roundToInt()} ppm"
    "flow" -> "${v.roundToInt()} m³/h"
    else -> ((v * 10).roundToInt() / 10.0).toString().replace('.', ',')
}

/** Scenes launched in one tap: those chosen, else those of this installation. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun ScenesBlock(ids: List<Long>, ctx: HomeContext) {
    val scope = rememberCoroutineScope()
    val list = ctx.routines.orEmpty().filter { r ->
        if (ids.isNotEmpty()) r.id in ids else r.actions.any { it.deviceId != null || it.installationId == ctx.installation?.id }
    }
    SectionCard {
        Text(stringResource(Res.string.block_scenes), style = MaterialTheme.typography.titleMedium)
        when {
            ctx.routines == null -> Unit
            list.isEmpty() -> Muted(stringResource(Res.string.block_scenes_empty))
            else -> FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                list.forEach { r ->
                    FilledTonalButton(
                        enabled = ctx.installation?.rights?.routines != false,
                        onClick = {
                            ctx.hub.launchAttempt(scope) {
                                val result = ctx.hub.api(ctx.serverId)?.runRoutine(r.id) ?: return@launchAttempt
                                val failed = result.results.count { !it.ok }
                                ctx.hub.notify(
                                    if (failed > 0) getPluralString(Res.plurals.routines_ran_with_failures, failed, r.name, failed)
                                    else getString(Res.string.routines_ran, r.name),
                                )
                            }
                        },
                    ) {
                        Icon(FzIcons.Play, null, Modifier.size(16.dp))
                        Spacer(Modifier.width(6.dp))
                        Text(r.name, maxLines = 1)
                    }
                }
            }
        }
    }
}

/** Whether this user is at home: the automations that depend on it follow. */
@Composable
private fun PresenceBlock(ctx: HomeContext) {
    val scope = rememberCoroutineScope()
    var presence by remember(ctx.serverId) { mutableStateOf<Presence?>(null) }
    LaunchedEffect(ctx.serverId) {
        while (true) {
            runCatching { ctx.hub.api(ctx.serverId)?.presence() }.getOrNull()?.let { presence = it }
            delay(60_000)
        }
    }
    SectionCard {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(FzIcons.House, null, Modifier.size(18.dp), tint = if (presence?.me == true) MaterialTheme.colorScheme.primary else Fz.colors.muted)
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Text(stringResource(Res.string.automations_home))
                presence?.someoneHome?.let { Caption(stringResource(if (it) Res.string.presence_someone else Res.string.presence_nobody)) }
            }
            Switch(
                checked = presence?.me == true,
                enabled = presence != null,
                onCheckedChange = { home -> ctx.hub.launchAttempt(scope) { ctx.hub.api(ctx.serverId)?.setPresence(home)?.let { presence = it } } },
            )
        }
    }
}

/** The groups of zones of the installation, to choose the one a zones card shows. */
fun zoneGroups(inst: Installation?): List<String?> = groupZones(inst?.zones.orEmpty()).map { it.first }
