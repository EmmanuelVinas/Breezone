package com.breezone.app.ui.home

import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.breezone.app.data.BlockSize
import com.breezone.app.data.Charted
import com.breezone.app.data.DashboardBlock
import com.breezone.app.data.TraitIds
import com.breezone.app.data.block
import com.breezone.app.data.MobilePages
import com.breezone.app.data.groupConfig
import com.breezone.app.data.shownOnCards
import com.breezone.app.data.typeConfig
import com.breezone.app.resources.*
import com.breezone.app.ui.components.IconBadge
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.screens.Caption
import com.breezone.app.ui.screens.Muted
import com.breezone.app.ui.theme.Fz
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive
import org.jetbrains.compose.resources.StringResource
import org.jetbrains.compose.resources.pluralStringResource
import org.jetbrains.compose.resources.stringResource

/** An entry of the sheet: a card to add at once, or a choice that leads further. */
private data class Entry(val key: String, val icon: ImageVector, val label: String, val sub: String? = null, val group: Group? = null, val pick: () -> Unit)

/** The sections of the results of a search. */
private enum class Group(val label: StringResource) {
    Cards(Res.string.block_group_cards),
    Devices(Res.string.block_group_devices),
    Charts(Res.string.block_group_charts),
}

/** The filters of the sheet: the cards and the devices of a part of the home. */
private enum class Category(val label: StringResource, val kinds: Set<BlockKind>, val devices: Set<DeviceKind>, val page: String) {
    Climate(Res.string.house_climate, setOf(BlockKind.Zones, BlockKind.ClimateZones, BlockKind.ZoneHistory, BlockKind.AirQuality), setOf(DeviceKind.Climate, DeviceKind.Sensor), MobilePages.CLIMATE),
    Energy(
        Res.string.house_energy,
        setOf(
            BlockKind.Energy, BlockKind.EnergyNow, BlockKind.EnergyToday, BlockKind.EnergyUses, BlockKind.EnergyKpis, BlockKind.EnergyDays,
            BlockKind.EnergyMeasures, BlockKind.EnergySurplus, BlockKind.Linky, BlockKind.PriceNow, BlockKind.PriceToday,
        ),
        setOf(DeviceKind.Energy, DeviceKind.Battery, DeviceKind.Switch),
        MobilePages.ENERGY,
    ),
    Lights(Res.string.house_lights, setOf(BlockKind.LightPanel, BlockKind.LightScenes, BlockKind.LightRoom), setOf(DeviceKind.Light), MobilePages.LIGHTS),
    Shutters(Res.string.house_shutters, setOf(BlockKind.ShutterRoom), setOf(DeviceKind.Shutter, DeviceKind.Window, DeviceKind.Door), MobilePages.SHUTTERS),
    Solar(Res.string.house_solar, setOf(BlockKind.SolarToday, BlockKind.SolarDays, BlockKind.SolarPanels, BlockKind.SolarForecast, BlockKind.EnergySurplus), setOf(DeviceKind.Solar), MobilePages.SOLAR),
    Charging(Res.string.house_charging, setOf(BlockKind.ChargerPage, BlockKind.Charger, BlockKind.ChargerCurve), setOf(DeviceKind.Charger), MobilePages.CHARGING),
    Ventilation(
        Res.string.house_ventilation,
        setOf(BlockKind.VentilationHouse, BlockKind.VentilationValves, BlockKind.VentilationRooms),
        setOf(DeviceKind.Ventilation, DeviceKind.RoomAir),
        MobilePages.VENTILATION,
    ),
}

private val ACCENTS = mapOf('à' to 'a', 'â' to 'a', 'ä' to 'a', 'á' to 'a', 'ç' to 'c', 'é' to 'e', 'è' to 'e', 'ê' to 'e', 'ë' to 'e', 'î' to 'i', 'ï' to 'i', 'í' to 'i', 'ô' to 'o', 'ö' to 'o', 'ó' to 'o', 'ù' to 'u', 'û' to 'u', 'ü' to 'u', 'ú' to 'u', 'ñ' to 'n', 'ÿ' to 'y', 'œ' to 'o', 'æ' to 'a')

/** Lower case, without accents, one character for one (so that a match can be highlighted). */
private fun folded(text: String) = text.lowercase().map { ACCENTS[it] ?: it }.joinToString("")

/** Every word of the search is in the text. */
private fun matches(query: String, vararg texts: String?): Boolean {
    val words = folded(query).split(' ').filter { it.isNotBlank() }
    val hay = texts.filterNotNull().joinToString(" ") { folded(it) }
    return words.all { it in hay }
}

/** The text with the words of the search highlighted. */
@Composable
private fun highlighted(text: String, query: String): AnnotatedString {
    val words = folded(query).split(' ').filter { it.isNotBlank() }
    if (words.isEmpty()) return AnnotatedString(text)
    val hay = folded(text)
    val style = SpanStyle(background = MaterialTheme.colorScheme.primary.copy(alpha = 0.3f))
    return buildAnnotatedString {
        append(text)
        words.forEach { w ->
            var at = hay.indexOf(w)
            while (at >= 0) {
                addStyle(style, at, at + w.length)
                at = hay.indexOf(w, at + w.length)
            }
        }
    }
}

/**
 * The cards that can be added to the home, then what the chosen one shows (a room, a device, a
 * measure…). [onAdd] receives the new card.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddBlockSheet(ctx: HomeContext, onDismiss: () -> Unit, first: String? = null, onAdd: (DashboardBlock) -> Unit) {
    var kind by remember { mutableStateOf<BlockKind?>(null) }
    var chartDevice by remember { mutableStateOf<Long?>(null) }
    var charted by remember { mutableStateOf<List<Charted>?>(null) }
    var query by remember { mutableStateOf("") }
    // On a page of the app, its own cards first.
    var filter by remember { mutableStateOf(Category.entries.firstOrNull { it.page == first }) }
    val searching = kind == null && (query.isNotBlank() || filter != null)
    LaunchedEffect(kind, searching) {
        if ((kind == BlockKind.Chart || kind == BlockKind.VentilationRooms || searching) && charted == null) charted = runCatching { ctx.hub.api(ctx.serverId)?.charted() }.getOrNull().orEmpty()
    }
    val noRoom = stringResource(Res.string.block_no_room)
    val allZones = stringResource(Res.string.block_zones_all)
    val shown = ctx.devices.filter { it.shownOnCards() }

    var subCount = 0
    val offered = BlockKind.entries.filter { it.offered(ctx) }
    val categories = Category.entries.filter { c -> offered.any { it in c.kinds } || shown.any { it.deviceKind in c.devices } }
    val kindEntries = offered.map { kk ->
        Entry(kk.name, kk.icon, stringResource(kk.label), stringResource(kk.hint()), Group.Cards) {
            query = ""
            if (kk.choice == BlockChoice.None) onAdd(block(kk.type, kk.size)) else kind = kk
        }
    }
    val roomCount = shown.groupingBy { it.roomId }.eachCount()
    val roomEntries = ctx.rooms.sortedBy { it.position }.filter { r -> (roomCount[r.id] ?: 0) > 0 }.map { r ->
        Entry("sr${r.id}", FzIcons.House, stringResource(Res.string.block_room_named, r.name), pluralStringResource(Res.plurals.block_room_devices, roomCount[r.id] ?: 0, roomCount[r.id] ?: 0), Group.Cards) {
            onAdd(block(BlockKind.Room.type, BlockKind.Room.size, groupConfig(r.id)))
        }
    }
    val deviceEntries = shown.sortedWith(compareBy({ it.deviceKind.ordinal }, { it.name })).map { d ->
        val room = ctx.rooms.firstOrNull { it.id == d.roomId }?.name
        Entry("sd${d.id}", d.deviceKind.icon, d.name, listOfNotNull(stringResource(d.deviceKind.label), room).joinToString(" · "), Group.Devices) {
            onAdd(block(BlockKind.Device.type, BlockKind.Device.size, mapOf("device_id" to JsonPrimitive(d.id))))
        }
    }
    val chartEntries = charted.orEmpty().flatMap { c ->
        val d = ctx.devices.firstOrNull { it.id == c.deviceId } ?: return@flatMap emptyList()
        c.fields.map { f ->
            Entry("sc${d.id}-$f", FzIcons.ChartLine, "${fieldLabel(f)} · ${d.name}", stringResource(Res.string.block_chart_day), Group.Charts) {
                onAdd(block(BlockKind.Chart.type, BlockKind.Chart.size, mapOf("device_id" to JsonPrimitive(d.id), "fields" to JsonArray(listOf(JsonPrimitive(f))), "hours" to JsonPrimitive(24))))
            }
        }
    }
    val deviceOf = { e: Entry -> e.key.removePrefix("sd").substringBefore('-').toLongOrNull() }
    val chartDeviceOf = { e: Entry -> e.key.removePrefix("sc").substringBefore('-').toLongOrNull() }
    val kindOf = { id: Long? -> ctx.devices.firstOrNull { it.id == id }?.deviceKind }

    val entries: List<Entry> = when (val k = kind) {
        null -> if (!searching) kindEntries.map { it.copy(group = null) } else {
            val c = filter
            kindEntries.filter { e -> (c == null || BlockKind.valueOf(e.key) in c.kinds) && matches(query, e.label, e.sub) } +
                (if (query.isNotBlank()) roomEntries.filter { matches(query, it.label) } else emptyList()) +
                deviceEntries.filter { e -> (c == null || kindOf(deviceOf(e)) in c.devices) && matches(query, e.label, e.sub) } +
                (if (query.isNotBlank()) chartEntries.filter { e -> (c == null || kindOf(chartDeviceOf(e)) in c.devices) && matches(query, e.label) } else emptyList())
        }
        // All the pages in one card, or one page alone as a half card.
        BlockKind.Pages -> listOf(Entry("p-all", FzIcons.LayoutGrid, stringResource(Res.string.block_pages_all)) { onAdd(block(k.type, k.size)) }) +
            shortcutPages(ctx).map { p ->
                Entry("p-${p.name}", p.icon, stringResource(p.label)) { onAdd(block(k.type, BlockSize.Half, mapOf("pages" to JsonArray(listOf(JsonPrimitive(p.name)))))) }
            }
        BlockKind.Energy -> {
            val roles = ctx.live?.roles
            EnergyFlow.entries.filter { f ->
                roles == null || when (f) {
                    EnergyFlow.Solar -> roles.solar
                    EnergyFlow.Battery -> roles.battery
                    EnergyFlow.Grid -> roles.grid
                    EnergyFlow.Home -> true
                }
            }.map { f -> Entry(f.id, f.icon, stringResource(f.label)) { onAdd(block(k.type, k.size, mapOf("flow" to JsonPrimitive(f.id)))) } }
        }
        BlockKind.Room -> ctx.rooms.sortedBy { it.position }.filter { r -> shown.any { it.roomId == r.id } }
            .map { r -> Entry("r${r.id}", FzIcons.House, r.name) { onAdd(block(k.type, k.size, groupConfig(r.id))) } } +
            listOfNotNull(Entry("r-none", FzIcons.Box, noRoom) { onAdd(block(k.type, k.size, groupConfig(null))) }.takeIf { shown.any { it.roomId == null } })
        BlockKind.Kind -> DeviceKind.entries.filter { dk -> shown.any { it.deviceKind == dk } }
            .map { dk -> Entry(dk.id, dk.icon, stringResource(dk.label)) { onAdd(block(k.type, k.size, typeConfig(dk.id))) } }
        BlockKind.Device -> shown.sortedWith(compareBy({ it.deviceKind.ordinal }, { it.name }))
            .map { d -> Entry("d${d.id}", d.deviceKind.icon, d.name, ctx.rooms.firstOrNull { it.id == d.roomId }?.name) { onAdd(block(k.type, k.size, mapOf("device_id" to JsonPrimitive(d.id)))) } }
        BlockKind.Chart -> {
            val withHistory = charted.orEmpty().filter { c -> c.fields.isNotEmpty() && ctx.devices.any { it.id == c.deviceId } }
            val device = chartDevice
            if (device == null) {
                withHistory.mapNotNull { c ->
                    val d = ctx.devices.firstOrNull { it.id == c.deviceId } ?: return@mapNotNull null
                    Entry("c${d.id}", d.deviceKind.icon, d.name, c.fields.map { fieldLabel(it) }.joinToString(" · ")) { chartDevice = d.id }
                }
            } else {
                withHistory.firstOrNull { it.deviceId == device }?.fields.orEmpty().map { f ->
                    Entry("f$f", FzIcons.ChartLine, fieldLabel(f)) {
                        onAdd(block(k.type, k.size, mapOf("device_id" to JsonPrimitive(device), "fields" to JsonArray(listOf(JsonPrimitive(f))), "hours" to JsonPrimitive(24))))
                    }
                }
            }
        }
        BlockKind.ZoneHistory, BlockKind.AirQuality -> shown.filter { d -> ctx.zoneOf(d)?.let { k != BlockKind.AirQuality || it.airQuality != null } == true }
            .map { d -> Entry("z${d.id}", FzIcons.Thermometer, d.name) { onAdd(block(k.type, k.size, mapOf("device_id" to JsonPrimitive(d.id)))) } }
        BlockKind.LightPanel -> {
            val lights = ctx.server.withTrait(TraitIds.LIGHT)
            ctx.rooms.sortedBy { it.position }.filter { r -> lights.any { it.roomId == r.id } }.map { r ->
                Entry("lr${r.id}", FzIcons.House, stringResource(Res.string.block_room_lights, r.name)) { onAdd(block(k.type, k.size, mapOf("room_id" to JsonPrimitive(r.id)))) }
            } + lights.sortedBy { it.name }.map { d ->
                Entry("l${d.id}", FzIcons.Lightbulb, d.name, ctx.rooms.firstOrNull { it.id == d.roomId }?.name) { onAdd(block(k.type, k.size, mapOf("device_id" to JsonPrimitive(d.id)))) }
            }
        }
        BlockKind.VentilationRooms -> ventilationFields(ctx, charted.orEmpty()).map { f ->
            Entry("v$f", FzIcons.ChartLine, fieldLabel(f)) { onAdd(block(k.type, k.size, mapOf("field" to JsonPrimitive(f), "hours" to JsonPrimitive(24)))) }
        }
        BlockKind.Zones -> listOf(Entry("z-all", FzIcons.Thermometer, allZones) { onAdd(block(k.type, k.size)) }) +
            zoneGroups(ctx.installation).filterNotNull().map { g ->
                Entry("z-$g", FzIcons.Thermometer, g) { onAdd(block(k.type, k.size, mapOf("group" to JsonPrimitive(g)))) }
            }
        else -> emptyList()
    }.also { subCount = if (kind != null) it.size else 0 }.let { list -> if (kind != null && query.isNotBlank()) list.filter { matches(query, it.label, it.sub) } else list }

    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.padding(horizontal = 16.dp).padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (kind != null) {
                    IconButton(onClick = {
                        query = ""
                        if (chartDevice != null) chartDevice = null else kind = null
                    }) {
                        Icon(FzIcons.ChevronLeft, stringResource(Res.string.common_back))
                    }
                }
                Text(
                    kind?.let { stringResource(it.label) } ?: stringResource(Res.string.home_add_card),
                    style = MaterialTheme.typography.titleLarge,
                    modifier = Modifier.padding(start = if (kind == null) 4.dp else 0.dp),
                )
            }
            if (kind == null || subCount > 8 || query.isNotBlank()) {
                OutlinedTextField(
                    value = query,
                    onValueChange = { query = it },
                    singleLine = true,
                    placeholder = {
                        Text(
                            when {
                                kind != null -> stringResource(Res.string.block_search_list)
                                filter != null -> stringResource(Res.string.block_search_in, stringResource(filter!!.label))
                                else -> stringResource(Res.string.block_search_hint)
                            },
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    },
                    leadingIcon = { Icon(FzIcons.Search, null, Modifier.size(18.dp)) },
                    trailingIcon = if (query.isNotEmpty()) ({ IconButton(onClick = { query = "" }) { Icon(FzIcons.X, stringResource(Res.string.common_clear)) } }) else null,
                    shape = Fz.shapes.button,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            if (kind == null && categories.size > 1) {
                Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    FilterChip(selected = filter == null, onClick = { filter = null }, label = { Text(stringResource(Res.string.block_filter_all)) })
                    categories.forEach { c -> FilterChip(selected = filter == c, onClick = { filter = if (filter == c) null else c }, label = { Text(stringResource(c.label)) }) }
                }
            }
            if (searching && entries.isEmpty()) Muted(stringResource(Res.string.block_search_none), Modifier.padding(8.dp))
            if (kind != null && entries.isEmpty()) {
                Muted(stringResource(if ((kind == BlockKind.Chart || kind == BlockKind.VentilationRooms) && charted == null) Res.string.block_loading else Res.string.block_nothing_to_choose), Modifier.padding(8.dp))
            }
            LazyColumn(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                val grouped = entries.groupBy { it.group }
                grouped.forEach { (group, list) ->
                    if (group != null && searching) {
                        item(key = "h-${group.name}", contentType = "header") {
                            Caption(stringResource(group.label).uppercase(), Modifier.padding(start = 4.dp, top = 8.dp))
                        }
                    }
                    items(list, key = { it.key }, contentType = { "entry" }) { e ->
                    Row(
                        Modifier.fillMaxWidth().clickable(onClick = e.pick).padding(horizontal = 4.dp, vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        IconBadge(e.icon, MaterialTheme.colorScheme.primary, size = 36.dp)
                        Spacer(Modifier.width(12.dp))
                        Column(Modifier.weight(1f)) {
                            Text(highlighted(e.label, query), style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            e.sub?.takeIf(String::isNotBlank)?.let { Text(highlighted(it, query), style = MaterialTheme.typography.bodySmall, color = Fz.colors.muted, maxLines = 2) }
                        }
                        Icon(FzIcons.ChevronRight, null, Modifier.size(18.dp), tint = Fz.colors.muted)
                    }
                    }
                }
            }
        }
    }
}

/** The measures of the ventilated rooms that have a history, to compare them on one chart. */
internal fun ventilationFields(ctx: HomeContext, charted: List<Charted>): List<String> {
    val rooms = ctx.server.withTrait(TraitIds.VENTILATION_ROOM).map { it.id }.toSet()
    val has = charted.filter { it.deviceId in rooms }.flatMap { it.fields }.toSet()
    return listOf("humidity", "flow", "co2", "temperature", "voc", "level").filter { it in has }
}

/** What a kind of card shows, in a few words. */
private fun BlockKind.hint() = when (this) {
    BlockKind.Pages -> Res.string.block_pages_hint
    BlockKind.Weather -> Res.string.block_weather_hint
    BlockKind.Presence -> Res.string.block_presence_hint
    BlockKind.Energy -> Res.string.block_energy_hint
    BlockKind.EnergyNow -> Res.string.block_energy_now_hint
    BlockKind.EnergyToday -> Res.string.block_energy_today_hint
    BlockKind.EnergyUses -> Res.string.block_energy_uses_hint
    BlockKind.Zones -> Res.string.block_zones_hint
    BlockKind.Room -> Res.string.block_room_hint
    BlockKind.Kind -> Res.string.block_kind_hint
    BlockKind.Device -> Res.string.block_device_hint
    BlockKind.Chart -> Res.string.block_chart_hint
    BlockKind.Scenes -> Res.string.block_scenes_hint
    BlockKind.ZoneHistory -> Res.string.block_zone_history_hint
    BlockKind.AirQuality -> Res.string.block_air_quality_hint
    BlockKind.Charger -> Res.string.block_charger_hint
    BlockKind.ChargerCurve -> Res.string.block_charger_curve_hint
    BlockKind.SolarToday -> Res.string.block_solar_today_hint
    BlockKind.SolarPanels -> Res.string.block_solar_panels_hint
    BlockKind.SolarForecast -> Res.string.block_solar_forecast_hint
    BlockKind.VentilationHouse -> Res.string.block_ventilation_house_hint
    BlockKind.VentilationRooms -> Res.string.block_ventilation_rooms_hint
    BlockKind.LightPanel -> Res.string.block_light_panel_hint
    BlockKind.PriceNow -> Res.string.block_price_now_hint
    BlockKind.PriceToday -> Res.string.block_price_today_hint
    BlockKind.Spacer -> Res.string.block_spacer_hint
    BlockKind.EnergyKpis -> Res.string.block_energy_kpis_hint
    BlockKind.EnergyDays -> Res.string.block_energy_days_hint
    BlockKind.EnergyMeasures -> Res.string.block_energy_measures_hint
    BlockKind.EnergySurplus -> Res.string.block_energy_surplus_hint
    BlockKind.Linky -> Res.string.block_linky_hint
    BlockKind.SolarDays -> Res.string.block_solar_days_hint
    BlockKind.LightScenes -> Res.string.block_light_scenes_hint
    BlockKind.LightRoom -> Res.string.block_light_room_hint
    BlockKind.ShutterRoom -> Res.string.block_shutter_room_hint
    BlockKind.VentilationValves -> Res.string.block_ventilation_valves_hint
    BlockKind.ClimateZones -> Res.string.block_climate_zones_hint
    BlockKind.ChargerPage -> Res.string.block_charger_page_hint
}
