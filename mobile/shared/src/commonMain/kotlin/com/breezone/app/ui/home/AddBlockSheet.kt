package com.breezone.app.ui.home

import androidx.compose.foundation.clickable
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.breezone.app.data.BlockSize
import com.breezone.app.data.Charted
import com.breezone.app.data.DashboardBlock
import com.breezone.app.data.TraitIds
import com.breezone.app.data.block
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
import org.jetbrains.compose.resources.stringResource

/** An entry of the sheet: a card to add at once, or a choice that leads further. */
private data class Entry(val key: String, val icon: ImageVector, val label: String, val sub: String? = null, val pick: () -> Unit)

/**
 * The cards that can be added to the home, then what the chosen one shows (a room, a device, a
 * measure…). [onAdd] receives the new card.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddBlockSheet(ctx: HomeContext, onDismiss: () -> Unit, onAdd: (DashboardBlock) -> Unit) {
    var kind by remember { mutableStateOf<BlockKind?>(null) }
    var chartDevice by remember { mutableStateOf<Long?>(null) }
    var charted by remember { mutableStateOf<List<Charted>?>(null) }
    LaunchedEffect(kind) {
        if ((kind == BlockKind.Chart || kind == BlockKind.VentilationRooms) && charted == null) charted = runCatching { ctx.hub.api(ctx.serverId)?.charted() }.getOrNull().orEmpty()
    }
    val noRoom = stringResource(Res.string.block_no_room)
    val allZones = stringResource(Res.string.block_zones_all)
    val shown = ctx.devices.filter { it.shownOnCards() }

    val entries: List<Entry> = when (val k = kind) {
        null -> BlockKind.entries.filter { it.offered(ctx) }.map { kk ->
            Entry(kk.name, kk.icon, stringResource(kk.label), stringResource(kk.hint())) {
                if (kk.choice == BlockChoice.None) onAdd(block(kk.type, kk.size)) else kind = kk
            }
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
    }

    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.padding(horizontal = 16.dp).padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (kind != null) {
                    IconButton(onClick = { if (chartDevice != null) chartDevice = null else kind = null }) {
                        Icon(FzIcons.ChevronLeft, stringResource(Res.string.common_back))
                    }
                }
                Text(
                    kind?.let { stringResource(it.label) } ?: stringResource(Res.string.home_add_card),
                    style = MaterialTheme.typography.titleLarge,
                    modifier = Modifier.padding(start = if (kind == null) 4.dp else 0.dp),
                )
            }
            if (kind != null && entries.isEmpty()) {
                Muted(stringResource(if ((kind == BlockKind.Chart || kind == BlockKind.VentilationRooms) && charted == null) Res.string.block_loading else Res.string.block_nothing_to_choose), Modifier.padding(8.dp))
            }
            LazyColumn(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                items(entries, key = { it.key }) { e ->
                    Row(
                        Modifier.fillMaxWidth().clickable(onClick = e.pick).padding(horizontal = 4.dp, vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        IconBadge(e.icon, MaterialTheme.colorScheme.primary, size = 36.dp)
                        Spacer(Modifier.width(12.dp))
                        Column(Modifier.weight(1f)) {
                            Text(e.label, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            e.sub?.takeIf(String::isNotBlank)?.let { Caption(it, maxLines = 2) }
                        }
                        Icon(FzIcons.ChevronRight, null, Modifier.size(18.dp), tint = Fz.colors.muted)
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
    BlockKind.VentilationHouse -> Res.string.block_ventilation_house_hint
    BlockKind.VentilationRooms -> Res.string.block_ventilation_rooms_hint
    BlockKind.LightPanel -> Res.string.block_light_panel_hint
    BlockKind.PriceNow -> Res.string.block_price_now_hint
    BlockKind.Spacer -> Res.string.block_spacer_hint
}
