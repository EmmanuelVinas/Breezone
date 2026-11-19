package com.breezone.app.ui.home

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
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
import com.breezone.app.data.DashboardBlock
import com.breezone.app.data.Periods
import com.breezone.app.data.shownOnCards
import com.breezone.app.data.withConfig
import com.breezone.app.data.withoutConfig
import com.breezone.app.resources.*
import com.breezone.app.ui.components.IconBadge
import com.breezone.app.ui.components.periodLabel
import com.breezone.app.ui.screens.Caption
import com.breezone.app.ui.screens.Muted
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive
import org.jetbrains.compose.resources.stringResource

/** The cards that have something to set once added. */
val BlockKind.settable get() = this in setOf(BlockKind.Pages, BlockKind.Room, BlockKind.Kind, BlockKind.Scenes, BlockKind.Chart, BlockKind.Device, BlockKind.LightPanel, BlockKind.VentilationRooms)

/**
 * What a card shows, set after it was added: the devices of a room or of a type, the scenes, the
 * measures and period of a chart. [onChange] receives the card as it is now.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun BlockSettingsSheet(initial: DashboardBlock, ctx: HomeContext, onDismiss: () -> Unit, onChange: (DashboardBlock) -> Unit) {
    var b by remember(initial.id) { mutableStateOf(initial) }
    var fields by remember { mutableStateOf<List<String>?>(null) }
    var charted by remember { mutableStateOf<List<com.breezone.app.data.Charted>?>(null) }
    LaunchedEffect(b.long("device_id")) {
        charted = runCatching { ctx.hub.api(ctx.serverId)?.charted() }.getOrNull().orEmpty()
        val id = b.long("device_id") ?: return@LaunchedEffect
        if (BlockKind.of(b) in setOf(BlockKind.Chart, BlockKind.Device)) fields = charted?.firstOrNull { it.deviceId == id }?.fields
    }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.padding(horizontal = 16.dp).padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(blockTitle(b, ctx), style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(start = 4.dp))
            when (BlockKind.of(b)) {
                BlockKind.Room, BlockKind.Kind -> {
                    val kind = DeviceKind.byId(b.string("type"))
                    val members = ctx.devices.filter { it.shownOnCards() }
                        .filter { if (b.has("type")) it.deviceKind == kind else it.roomId == b.long("room_id") }
                        .sortedWith(compareBy({ it.deviceKind.ordinal }, { it.name }))
                    val hidden = b.longs("hidden").toSet()
                    Caption(stringResource(Res.string.block_settings_devices), Modifier.padding(start = 4.dp))
                    Checklist(members.map { Triple(it.id.toString(), it.deviceKind.icon, it.name) }, members.filter { it.id !in hidden }.map { it.id.toString() }.toSet()) { shown ->
                        b = b.withConfig("hidden", JsonArray(members.filter { it.id.toString() !in shown }.map { JsonPrimitive(it.id) }))
                    }
                }
                BlockKind.Pages -> {
                    val pages = shortcutPages(ctx)
                    val chosen = b.strings("pages")
                    Caption(stringResource(Res.string.block_settings_pages), Modifier.padding(start = 4.dp))
                    val labels = pages.map { stringResource(it.label) }
                    Checklist(pages.mapIndexed { i, p -> Triple(p.name, p.icon, labels[i]) }, (chosen.ifEmpty { pages.map { it.name } }).toSet()) { next ->
                        // Kept in the order of the bar; all ticked is the same as none chosen.
                        val list = pages.map { it.name }.filter { it in next }
                        b = b.withConfig("pages", JsonArray((if (list.size == pages.size) emptyList() else list).map { JsonPrimitive(it) }))
                    }
                }
                BlockKind.Scenes -> {
                    val routines = ctx.routines.orEmpty()
                    Caption(stringResource(Res.string.block_settings_scenes), Modifier.padding(start = 4.dp))
                    if (routines.isEmpty()) Muted(stringResource(Res.string.block_scenes_empty))
                    Checklist(routines.map { Triple(it.id.toString(), com.breezone.app.ui.icons.FzIcons.Sparkles, it.name) }, b.longs("routine_ids").map { it.toString() }.toSet()) { chosen ->
                        b = b.withConfig("routine_ids", JsonArray(routines.filter { it.id.toString() in chosen }.map { JsonPrimitive(it.id) }))
                    }
                }
                BlockKind.Chart -> {
                    val hours = b.long("hours")?.toInt() ?: Periods.DAY
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        listOf(Periods.DAY, Periods.WEEK).forEach { h ->
                            FilterChip(selected = hours == h, onClick = { b = b.withConfig("hours", JsonPrimitive(h)) }, label = { Text(periodLabel(h)) })
                        }
                    }
                    val chosen = b.strings("fields").ifEmpty { listOfNotNull(b.string("field")) }.toSet()
                    Caption(stringResource(Res.string.block_settings_measures), Modifier.padding(start = 4.dp))
                    val labels = fields.orEmpty().map { fieldLabel(it) }
                    Checklist(fields.orEmpty().mapIndexed { i, f -> Triple(f, com.breezone.app.ui.icons.FzIcons.ChartLine, labels[i]) }, chosen) { next ->
                        // A chart keeps at least one curve.
                        if (next.isNotEmpty()) b = b.withConfig("fields", JsonArray(fields.orEmpty().filter { it in next }.map { JsonPrimitive(it) }))
                    }
                }
                BlockKind.Device -> {
                    // A measure with a history can show its curve rather than its controls.
                    val options = fields.orEmpty()
                    if (options.isEmpty()) Muted(stringResource(Res.string.block_settings_none))
                    else {
                        Caption(stringResource(Res.string.block_settings_curve), Modifier.padding(start = 4.dp))
                        val curve = b.string("curve")
                        androidx.compose.foundation.layout.FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                            FilterChip(selected = curve == null, onClick = { b = b.withoutConfig("curve") }, label = { Text(stringResource(Res.string.block_curve_none)) })
                            options.forEach { f ->
                                FilterChip(selected = curve == f, onClick = { b = b.withConfig("curve", JsonPrimitive(f)) }, label = { Text(fieldLabel(f)) })
                            }
                        }
                    }
                }
                BlockKind.LightPanel -> {
                    Caption(stringResource(Res.string.block_settings_show), Modifier.padding(start = 4.dp))
                    val shown = setOfNotNull("temperature".takeIf { b.bool("temperature") != false }, "colors".takeIf { b.bool("colors") != false })
                    Checklist(
                        listOf(
                            Triple("temperature", com.breezone.app.ui.icons.FzIcons.Sun, stringResource(Res.string.light_white)),
                            Triple("colors", com.breezone.app.ui.icons.FzIcons.Palette, stringResource(Res.string.light_colors)),
                        ),
                        shown,
                    ) { next ->
                        b = b.withConfig("temperature", JsonPrimitive("temperature" in next)).withConfig("colors", JsonPrimitive("colors" in next))
                    }
                }
                BlockKind.VentilationRooms -> {
                    val hours = b.long("hours")?.toInt() ?: Periods.DAY
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        listOf(Periods.DAY, Periods.WEEK).forEach { h ->
                            FilterChip(selected = hours == h, onClick = { b = b.withConfig("hours", JsonPrimitive(h)) }, label = { Text(periodLabel(h)) })
                        }
                    }
                    Caption(stringResource(Res.string.block_settings_measure), Modifier.padding(start = 4.dp))
                    val field = b.string("field") ?: "humidity"
                    androidx.compose.foundation.layout.FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        ventilationFields(ctx, charted.orEmpty()).forEach { f ->
                            FilterChip(selected = field == f, onClick = { b = b.withConfig("field", JsonPrimitive(f)) }, label = { Text(fieldLabel(f)) })
                        }
                    }
                }
                else -> Muted(stringResource(Res.string.block_settings_none))
            }
            Button(onClick = { onChange(b) }, modifier = Modifier.fillMaxWidth()) { Text(stringResource(Res.string.block_settings_apply)) }
        }
    }
}

/** Entries to tick: (key, icon, name); [onChange] receives the keys ticked. */
@Composable
private fun Checklist(entries: List<Triple<String, ImageVector, String>>, checked: Set<String>, onChange: (Set<String>) -> Unit) {
    LazyColumn(Modifier.fillMaxWidth()) {
        items(entries, key = { it.first }) { (key, icon, name) ->
            val on = key in checked
            Row(
                Modifier.fillMaxWidth().clickable { onChange(if (on) checked - key else checked + key) }.padding(horizontal = 4.dp, vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                IconBadge(icon, MaterialTheme.colorScheme.primary, active = on, size = 32.dp)
                Spacer(Modifier.width(12.dp))
                Text(name, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                Checkbox(checked = on, onCheckedChange = { onChange(if (on) checked - key else checked + key) })
            }
        }
    }
}
